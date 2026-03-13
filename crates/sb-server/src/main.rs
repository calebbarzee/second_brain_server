mod tools;

use anyhow::Result;
use clap::Parser;
use rmcp::ServiceExt;
use sb_skills::{SkillContext, SkillRegistry, SkillRunner};
use sb_sync::{FileWatcher, SyncProcessor, WatcherConfig};
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum Transport {
    Stdio,
    Http,
}

#[derive(Parser)]
#[command(name = "second-brain", about = "Second Brain MCP Server")]
struct Cli {
    /// Path to config file
    #[arg(short, long)]
    config: Option<String>,

    /// Transport: stdio (default, for local MCP) or http (network-accessible)
    #[arg(long, default_value = "stdio")]
    transport: Transport,

    /// Host to bind HTTP server to (default: 0.0.0.0)
    #[arg(long, default_value = "0.0.0.0")]
    host: String,

    /// Port for HTTP transport (default: 8080)
    #[arg(long, default_value_t = 8080)]
    port: u16,

    /// Directories to watch for markdown changes (comma-separated).
    /// Overrides config file paths. Also settable via WATCH_PATHS env var.
    #[arg(long)]
    watch: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Log to stderr for stdio (stdout = JSON-RPC), stdout for HTTP
    let log_writer: Box<dyn std::io::Write + Send> = if cli.transport == Transport::Stdio {
        Box::new(std::io::stderr())
    } else {
        Box::new(std::io::stdout())
    };

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
        .with_writer(std::sync::Mutex::new(log_writer))
        .with_ansi(cli.transport == Transport::Http)
        .init();

    // Load config
    let config = match &cli.config {
        Some(path) => sb_core::Config::load(std::path::Path::new(path))?,
        None => sb_core::Config::from_env()?,
    };

    tracing::info!("connecting to database");
    let db = sb_core::Database::connect(&config.database.url).await?;

    // Resolve embedding config (preset + env var overrides)
    let embed_cfg = config.embedding.resolve();

    tracing::info!(
        "embedding provider: {} at {}, model={}, dims={}",
        embed_cfg.provider,
        embed_cfg.url,
        embed_cfg.model,
        embed_cfg.dimensions
    );

    let embedding_dims = embed_cfg.dimensions;
    let pipeline = Arc::new(sb_embed::make_pipeline(&embed_cfg));

    // Ensure HNSW index matches configured dimensions
    if let Err(e) = sb_core::db::embeddings::ensure_vector_index(db.pool(), embedding_dims).await {
        tracing::warn!("could not create vector index (non-fatal): {e}");
    }

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

    match cli.transport {
        Transport::Stdio => run_stdio(db, pipeline, skill_runner, watch_paths).await,
        Transport::Http => {
            run_http(db, pipeline, skill_runner, watch_paths, &cli.host, cli.port).await
        }
    }
}

/// Run the MCP server over stdin/stdout (for local Claude Code integration).
async fn run_stdio(
    db: sb_core::Database,
    pipeline: Arc<sb_embed::EmbeddingPipeline>,
    skill_runner: Arc<SkillRunner>,
    watch_paths: Vec<PathBuf>,
) -> Result<()> {
    tracing::info!("starting MCP server on stdio (16 tools)");

    let server = tools::SecondBrainServer::new(db, pipeline, skill_runner, watch_paths);
    let service = server
        .serve(rmcp::transport::stdio())
        .await
        .inspect_err(|e| tracing::error!("serving error: {:?}", e))?;

    tracing::info!("MCP server running on stdio");
    service.waiting().await?;

    tracing::info!("MCP server shutting down");
    Ok(())
}

/// Run the MCP server over HTTP with Streamable HTTP transport (network-accessible).
async fn run_http(
    db: sb_core::Database,
    pipeline: Arc<sb_embed::EmbeddingPipeline>,
    skill_runner: Arc<SkillRunner>,
    watch_paths: Vec<PathBuf>,
    host: &str,
    port: u16,
) -> Result<()> {
    use rmcp::transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
    };
    use tokio_util::sync::CancellationToken;

    let ct = CancellationToken::new();
    let session_manager = Arc::new(LocalSessionManager::default());

    let service = StreamableHttpService::new(
        move || {
            Ok(tools::SecondBrainServer::new(
                db.clone(),
                pipeline.clone(),
                skill_runner.clone(),
                watch_paths.clone(),
            ))
        },
        session_manager,
        StreamableHttpServerConfig {
            stateful_mode: true,
            json_response: false,
            sse_keep_alive: Some(std::time::Duration::from_secs(15)),
            sse_retry: Some(std::time::Duration::from_secs(3)),
            cancellation_token: ct.child_token(),
        },
    );

    let app = axum::Router::new().nest_service("/mcp", service);

    let bind_addr = format!("{host}:{port}");
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;

    tracing::info!("MCP server listening on http://{bind_addr}/mcp (16 tools)");

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            tokio::signal::ctrl_c()
                .await
                .expect("failed to listen for ctrl-c");
            tracing::info!("shutting down HTTP server");
            ct.cancel();
        })
        .await?;

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
