use anyhow::Result;
use clap::Parser;
use std::sync::Arc;

#[derive(Parser)]
#[command(
    name = "sb",
    about = "Second Brain CLI — search, ingest, and manage your knowledge base"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Ingest markdown files into the database (with optional embedding)
    Ingest {
        /// Path to a file or directory to ingest
        path: String,
        /// Skip embedding (just ingest text + links)
        #[arg(long)]
        no_embed: bool,
    },
    /// Full-text search over note titles and content
    Search {
        /// Search query
        query: String,
        /// Max results (default: 10)
        #[arg(short = 'n', long, default_value = "10")]
        limit: i64,
        /// Filter by lifecycle (active, volatile, enduring, archived)
        #[arg(long)]
        lifecycle: Option<String>,
    },
    /// Semantic similarity search (requires embeddings)
    Semantic {
        /// Natural language query
        query: String,
        /// Max results (default: 10)
        #[arg(short = 'n', long, default_value = "10")]
        limit: i64,
    },
    /// List notes (most recently updated first)
    List {
        /// Max results (default: 20)
        #[arg(short = 'n', long, default_value = "20")]
        limit: i64,
        /// Filter by lifecycle
        #[arg(long)]
        lifecycle: Option<String>,
        /// Filter by project name
        #[arg(long)]
        project: Option<String>,
    },
    /// Read a note's content
    Read {
        /// File path of the note
        path: String,
    },
    /// List all projects with note counts
    Projects,
    /// Run a skill (summarize, reflect, continue-work, connect-ideas, contextualize)
    Skill {
        /// Skill name
        name: String,
        /// Time period (today, this-week, last-week, this-month, YYYY-MM-DD)
        #[arg(long, default_value = "this-week")]
        period: String,
        /// Project name to scope to
        #[arg(long)]
        project: Option<String>,
        /// Allow destructive skills to write changes
        #[arg(long)]
        allow_writes: bool,
        /// Write output as a new note
        #[arg(long)]
        write_output: bool,
    },
    /// Classify a note's lifecycle
    Classify {
        /// File path of the note
        path: String,
        /// Lifecycle: active, volatile, enduring, archived
        lifecycle: String,
    },
    /// Show stats about the knowledge base
    Stats,
    /// Embed all unembedded notes
    Embed,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("sb=info".parse()?)
                .add_directive("sb_core=warn".parse()?)
                .add_directive("sb_embed=warn".parse()?)
                .add_directive("sb_skills=info".parse()?),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    let config = sb_core::Config::from_env()?;
    let db = sb_core::Database::connect(&config.database.url).await?;

    match cli.command {
        Commands::Ingest { path, no_embed } => cmd_ingest(&db, &config, &path, no_embed).await?,
        Commands::Search {
            query,
            limit,
            lifecycle,
        } => cmd_search(&db, &query, limit, lifecycle.as_deref()).await?,
        Commands::Semantic { query, limit } => cmd_semantic(&db, &config, &query, limit).await?,
        Commands::List {
            limit,
            lifecycle,
            project,
        } => cmd_list(&db, limit, lifecycle.as_deref(), project.as_deref()).await?,
        Commands::Read { path } => cmd_read(&db, &path).await?,
        Commands::Projects => cmd_projects(&db).await?,
        Commands::Skill {
            name,
            period,
            project,
            allow_writes,
            write_output,
        } => {
            cmd_skill(
                &db,
                &config,
                &name,
                &period,
                project.as_deref(),
                allow_writes,
                write_output,
            )
            .await?
        }
        Commands::Classify { path, lifecycle } => cmd_classify(&db, &path, &lifecycle).await?,
        Commands::Stats => cmd_stats(&db).await?,
        Commands::Embed => cmd_embed(&db, &config).await?,
    }

    Ok(())
}

// ── Commands ────────────────────────────────────────────────────

async fn cmd_ingest(
    db: &sb_core::Database,
    config: &sb_core::Config,
    path: &str,
    no_embed: bool,
) -> Result<()> {
    let p = std::path::Path::new(path);
    if !p.exists() {
        anyhow::bail!("path does not exist: {path}");
    }

    let stats = sb_core::ingest::ingest_directory(db, p).await?;
    println!(
        "Ingested: {} new, {} unchanged, {} links, {} errors",
        stats.ingested,
        stats.skipped,
        stats.links_stored,
        stats.errors.len()
    );

    if !no_embed && stats.ingested > 0 {
        let pipeline = make_pipeline(config);
        let embed_stats = pipeline.process_unembedded(db.pool()).await?;
        println!(
            "Embedded: {} notes, {} chunks, {} embeddings",
            embed_stats.notes_processed, embed_stats.chunks_created, embed_stats.embeddings_created
        );
    }

    for err in &stats.errors {
        eprintln!("  error: {err}");
    }

    Ok(())
}

async fn cmd_search(
    db: &sb_core::Database,
    query: &str,
    limit: i64,
    lifecycle: Option<&str>,
) -> Result<()> {
    let notes = if lifecycle.is_some() {
        sb_core::db::notes::search_notes_filtered(db.pool(), query, lifecycle, None, limit).await?
    } else {
        sb_core::db::notes::search_notes(db.pool(), query, limit).await?
    };

    if notes.is_empty() {
        println!("No results.");
        return Ok(());
    }

    for note in &notes {
        let lifecycle_tag = if note.lifecycle != "active" {
            format!(" [{}]", note.lifecycle)
        } else {
            String::new()
        };
        println!("  {}{} — {}", note.file_path, lifecycle_tag, note.title);
    }
    println!("\n{} results", notes.len());

    Ok(())
}

async fn cmd_semantic(
    db: &sb_core::Database,
    config: &sb_core::Config,
    query: &str,
    limit: i64,
) -> Result<()> {
    let pipeline = make_pipeline(config);
    let query_vec = pipeline.embed_query(query).await?;
    let results = sb_core::db::embeddings::semantic_search(db.pool(), &query_vec, limit).await?;

    if results.is_empty() {
        println!("No results. Are notes ingested and embedded?");
        return Ok(());
    }

    for (i, r) in results.iter().enumerate() {
        let section = r
            .heading_context
            .as_deref()
            .map(|s| format!(" > {s}"))
            .unwrap_or_default();
        println!(
            "  {}. [{:.3}] {}{} — {}",
            i + 1,
            r.similarity,
            r.note_title,
            section,
            truncate(&r.chunk_content, 120),
        );
    }

    Ok(())
}

async fn cmd_list(
    db: &sb_core::Database,
    limit: i64,
    lifecycle: Option<&str>,
    project: Option<&str>,
) -> Result<()> {
    let project_id = match project {
        Some(name) => sb_core::db::projects::get_project_by_name(db.pool(), name)
            .await?
            .map(|p| p.id),
        None => None,
    };

    let notes = if lifecycle.is_some() || project_id.is_some() {
        sb_core::db::notes::list_notes_filtered(db.pool(), lifecycle, project_id, limit, 0).await?
    } else {
        sb_core::db::notes::list_notes(db.pool(), limit, 0).await?
    };

    if notes.is_empty() {
        println!("No notes found.");
        return Ok(());
    }

    for note in &notes {
        let age = chrono::Utc::now() - note.updated_at;
        let age_str = if age.num_days() > 0 {
            format!("{}d ago", age.num_days())
        } else if age.num_hours() > 0 {
            format!("{}h ago", age.num_hours())
        } else {
            "just now".to_string()
        };
        let lifecycle_tag = if note.lifecycle != "active" {
            format!(" [{}]", note.lifecycle)
        } else {
            String::new()
        };
        println!(
            "  {:<50} {:>8}{}",
            truncate(&note.title, 50),
            age_str,
            lifecycle_tag,
        );
    }

    Ok(())
}

async fn cmd_read(db: &sb_core::Database, path: &str) -> Result<()> {
    let note = sb_core::db::notes::get_note_by_path(db.pool(), path).await?;
    match note {
        Some(n) => {
            println!("# {}", n.title);
            println!("path: {}", n.file_path);
            println!("lifecycle: {}", n.lifecycle);
            println!("updated: {}", n.updated_at.format("%Y-%m-%d %H:%M"));
            if let Some(proj) = &n.source_project {
                println!("project: {proj}");
            }
            println!("---\n{}", n.raw_content);
        }
        None => println!("Note not found: {path}"),
    }
    Ok(())
}

async fn cmd_projects(db: &sb_core::Database) -> Result<()> {
    let projects = sb_core::db::projects::list_projects_with_counts(db.pool()).await?;

    if projects.is_empty() {
        println!(
            "No projects. Projects are auto-detected from note filenames (e.g., <project_name>_foo.md)"
        );
        println!("or by running: sb skill contextualize --period this-month");
        return Ok(());
    }

    println!("Projects:");
    for p in &projects {
        println!("  {:<20} {} notes", p.project_name, p.note_count);
    }

    Ok(())
}

async fn cmd_skill(
    db: &sb_core::Database,
    config: &sb_core::Config,
    name: &str,
    period: &str,
    project: Option<&str>,
    allow_writes: bool,
    write_output: bool,
) -> Result<()> {
    let pipeline = Arc::new(make_pipeline(config));

    // Determine notes root — config paths first, then auto-discover
    let notes_root = config.notes.paths.first().cloned().unwrap_or_else(|| {
        let discovered = sb_core::file_search::discover_notes_dirs(2);
        discovered.into_iter().next().unwrap_or_else(|| {
            std::env::var("HOME")
                .map(|h| std::path::PathBuf::from(h).join("notes"))
                .unwrap_or_else(|_| std::path::PathBuf::from("./notes"))
        })
    });

    let ctx = Arc::new(sb_skills::SkillContext::new(
        db.clone(),
        pipeline,
        None, // No LLM in CLI — always deferred
        notes_root,
    ));
    let registry = sb_skills::SkillRegistry::with_builtins();
    let runner = sb_skills::SkillRunner::new(registry, ctx);

    let params = sb_skills::SkillParams {
        period: Some(period.to_string()),
        project: project.map(|s| s.to_string()),
        dry_run: false,
        allow_writes,
        write_output,
    };

    let output = runner.run(name, &params).await?;

    println!("{}", output.summary);

    if !output.notes_created.is_empty() {
        println!("\nNotes created:");
        for p in &output.notes_created {
            println!("  {p}");
        }
    }
    if !output.notes_modified.is_empty() {
        println!("\nNotes modified:");
        for p in &output.notes_modified {
            println!("  {p}");
        }
    }

    // Print structured context as compact JSON for piping
    if let Some(ctx) = &output.context {
        println!("\n{}", serde_json::to_string_pretty(ctx)?);
    }

    if let Some(changeset) = &output.changeset {
        println!("\nProposed changes:");
        println!("{}", serde_json::to_string_pretty(changeset)?);
    }

    Ok(())
}

async fn cmd_classify(db: &sb_core::Database, path: &str, lifecycle: &str) -> Result<()> {
    let lc = sb_core::lifecycle::Lifecycle::from_str(lifecycle).ok_or_else(|| {
        anyhow::anyhow!(
            "invalid lifecycle: {lifecycle} (use: active, volatile, enduring, archived)"
        )
    })?;

    let note = sb_core::db::notes::get_note_by_path(db.pool(), path).await?;
    let note = note.ok_or_else(|| anyhow::anyhow!("note not found: {path}"))?;

    let old = &note.lifecycle;

    if lc == sb_core::lifecycle::Lifecycle::Archived {
        let src = std::path::Path::new(&note.file_path);
        if src.exists() {
            let archive_dir = src
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .join("archive");
            let dest = archive_dir.join(src.file_name().unwrap_or_default());
            std::fs::create_dir_all(&archive_dir)?;
            std::fs::rename(src, &dest)?;
            sb_core::db::notes::update_file_path(db.pool(), note.id, &dest.to_string_lossy())
                .await?;
            println!("Archived: {} -> {}", note.file_path, dest.display());
        }
    }

    sb_core::db::notes::update_lifecycle(db.pool(), note.id, lc.as_str()).await?;
    println!("'{}': {} -> {}", note.title, old, lc);

    Ok(())
}

async fn cmd_stats(db: &sb_core::Database) -> Result<()> {
    let all_notes = sb_core::db::notes::list_notes(db.pool(), 10000, 0).await?;
    let total = all_notes.len();

    let mut by_lifecycle: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for note in &all_notes {
        *by_lifecycle.entry(note.lifecycle.clone()).or_insert(0) += 1;
    }

    let embedding_count = sb_core::db::embeddings::count_embeddings(db.pool())
        .await
        .unwrap_or(0);
    let projects = sb_core::db::projects::list_projects_with_counts(db.pool())
        .await
        .unwrap_or_default();

    println!("Second Brain Stats");
    println!("==================");
    println!("Notes:      {total}");
    for (lc, count) in &by_lifecycle {
        println!("  {lc:<12} {count}");
    }
    println!("Embeddings: {embedding_count}");
    println!("Projects:   {}", projects.len());
    for p in &projects {
        println!("  {:<16} {} notes", p.project_name, p.note_count);
    }

    Ok(())
}

async fn cmd_embed(db: &sb_core::Database, config: &sb_core::Config) -> Result<()> {
    let pipeline = make_pipeline(config);
    let stats = pipeline.process_unembedded(db.pool()).await?;
    println!(
        "Embedded: {} notes, {} chunks, {} embeddings",
        stats.notes_processed, stats.chunks_created, stats.embeddings_created
    );
    if !stats.errors.is_empty() {
        for err in &stats.errors {
            eprintln!("  error: {err}");
        }
    }
    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────────

fn make_pipeline(config: &sb_core::Config) -> sb_embed::EmbeddingPipeline {
    let url =
        std::env::var("EMBEDDING_URL").unwrap_or_else(|_| config.embedding.url.clone());
    let model = std::env::var("EMBEDDING_MODEL").unwrap_or_else(|_| config.embedding.model.clone());
    let dims: usize = std::env::var("EMBEDDING_DIMS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(config.embedding.dimensions);

    let provider = Arc::new(sb_embed::TeiProvider::new(&url, &model, dims));
    sb_embed::EmbeddingPipeline::new(provider, config.embedding.batch_size)
}

fn truncate(s: &str, max: usize) -> String {
    let s = s.replace('\n', " ");
    if s.len() <= max {
        s
    } else {
        format!("{}...", &s[..max])
    }
}
