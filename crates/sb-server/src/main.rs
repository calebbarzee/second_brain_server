mod tools;

use anyhow::Result;
use clap::Parser;
use rmcp::ServiceExt;
use sb_embed::{EmbeddingPipeline, TeiProvider};
use sb_skills::{SkillContext, SkillRegistry, SkillRunner};
use sb_sync::{FileWatcher, SyncProcessor, WatcherConfig};
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "second-brain", about = "Second Brain MCP Server")]
struct Cli {
    /// Path to config file
    #[arg(short, long)]
    config: Option<String>,

    /// Directories to watch for markdown changes (comma-separated).
    /// Overrides config file paths. Also settable via WATCH_PATHS env var.
    #[arg(long)]
    watch: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Log to stderr — stdout is the MCP stdio transport
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive("sb_core=info".parse()?)
                .add_directive("sb_server=info".parse()?)
                .add_directive("sb_embed=info".parse()?)
                .add_directive("sb_sync=info".parse()?)
                .add_directive("sb_skills=info".parse()?)
                .add_directive("rmcp=warn".parse()?),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let cli = Cli::parse();

    // Load config
    let config = match &cli.config {
        Some(path) => sb_core::Config::load(std::path::Path::new(path))?,
        None => sb_core::Config::from_env()?,
    };

    tracing::info!("connecting to database");
    let db = sb_core::Database::connect(&config.database.url).await?;

    // Set up embedding provider (env vars override config file)
    let embedding_url =
        std::env::var("EMBEDDING_URL").unwrap_or_else(|_| config.embedding.url.clone());
    let embedding_model =
        std::env::var("EMBEDDING_MODEL").unwrap_or_else(|_| config.embedding.model.clone());
    let embedding_dims: usize = std::env::var("EMBEDDING_DIMS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(config.embedding.dimensions);

    tracing::info!(
        "embedding provider: TEI at {}, model={}, dims={}",
        embedding_url,
        embedding_model,
        embedding_dims
    );

    let provider = Arc::new(TeiProvider::new(
        &embedding_url,
        &embedding_model,
        embedding_dims,
    ));
    let pipeline = Arc::new(EmbeddingPipeline::new(
        provider,
        config.embedding.batch_size,
    ));

    // Set up optional LLM provider
    let llm_provider: Option<Arc<dyn sb_skills::llm::LlmProvider>> =
        if let Ok(api_key) = std::env::var("ANTHROPIC_API_KEY") {
            let model = config.llm.as_ref().and_then(|c| c.model.clone());
            tracing::info!(
                "LLM provider: Anthropic (model: {})",
                model.as_deref().unwrap_or("default")
            );
            Some(Arc::new(sb_skills::llm_anthropic::AnthropicProvider::new(
                api_key, model,
            )))
        } else {
            tracing::info!(
                "no ANTHROPIC_API_KEY — skills will return deferred prompts for Claude to process"
            );
            None
        };

    // Determine notes root (first watch path or home/notes)
    let notes_root = config
        .notes
        .paths
        .first()
        .cloned()
        .unwrap_or_else(dirs_or_default);

    // Set up skill engine
    let skill_ctx = Arc::new(SkillContext::new(
        db.clone(),
        pipeline.clone(),
        llm_provider,
        notes_root,
    ));
    let registry = SkillRegistry::with_builtins();
    let skill_runner = Arc::new(SkillRunner::new(registry, skill_ctx));

    // Determine watch paths: CLI flag > env var > config file
    let watch_paths = resolve_watch_paths(&cli, &config);

    // Start file watcher if we have paths to watch
    if !watch_paths.is_empty() {
        tracing::info!(
            "starting file watcher for {} directories",
            watch_paths.len()
        );
        for p in &watch_paths {
            tracing::info!("  watching: {}", p.display());
        }

        let (watcher, rx) = FileWatcher::start(watch_paths.clone(), WatcherConfig::default())?;
        let processor = SyncProcessor::new(db.clone(), pipeline.clone());

        tokio::spawn(async move {
            processor.run(rx).await;
            drop(watcher);
        });
    } else {
        tracing::info!("no watch paths configured — file watcher disabled");
    }

    tracing::info!("starting MCP server (15 tools)");

    let server = tools::SecondBrainServer::new(db, pipeline, skill_runner, watch_paths.clone());
    let service = server
        .serve(rmcp::transport::stdio())
        .await
        .inspect_err(|e| tracing::error!("serving error: {:?}", e))?;

    tracing::info!("MCP server running on stdio");
    service.waiting().await?;

    tracing::info!("MCP server shutting down");
    Ok(())
}

/// Resolve watch paths from CLI, env, or config (in priority order).
fn resolve_watch_paths(cli: &Cli, config: &sb_core::Config) -> Vec<PathBuf> {
    if let Some(watch_str) = &cli.watch {
        return watch_str
            .split(',')
            .map(|s| PathBuf::from(s.trim()))
            .filter(|p| !p.as_os_str().is_empty())
            .collect();
    }

    if let Ok(watch_str) = std::env::var("WATCH_PATHS") {
        return watch_str
            .split(',')
            .map(|s| PathBuf::from(s.trim()))
            .filter(|p| !p.as_os_str().is_empty())
            .collect();
    }

    config.notes.paths.clone()
}

/// Get a sensible default notes root path.
/// Searches up to 2 subdirectories of $HOME for a "notes" directory.
fn dirs_or_default() -> PathBuf {
    let discovered = sb_core::file_search::discover_notes_dirs(2);
    if let Some(first) = discovered.into_iter().next() {
        return first;
    }
    // Final fallback
    if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join("notes")
    } else {
        PathBuf::from("./notes")
    }
}
