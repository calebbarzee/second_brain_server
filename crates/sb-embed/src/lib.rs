pub mod chunker;
pub mod openai;
pub mod pipeline;
pub mod provider;
pub mod tei;

pub use chunker::{Chunker, ChunkerConfig};
pub use openai::OpenAiProvider;
pub use pipeline::EmbeddingPipeline;
pub use provider::EmbeddingProvider;
pub use tei::TeiProvider;

use std::sync::Arc;

/// Build an `EmbeddingPipeline` from a resolved config.
/// This is the single place that maps provider names to concrete types.
pub fn make_pipeline(cfg: &sb_core::config::ResolvedEmbeddingConfig) -> EmbeddingPipeline {
    let provider: Arc<dyn EmbeddingProvider> = match cfg.provider.as_str() {
        "openai" | "ollama" => Arc::new(OpenAiProvider::new(
            &cfg.url,
            &cfg.model,
            cfg.dimensions,
            std::env::var("EMBEDDING_API_KEY").ok(),
        )),
        _ => Arc::new(TeiProvider::new(&cfg.url, &cfg.model, cfg.dimensions)),
    };

    let chunker_config = ChunkerConfig {
        max_chunk_chars: cfg.max_chunk_chars,
        ..ChunkerConfig::default()
    };

    EmbeddingPipeline::new(provider, cfg.batch_size, chunker_config)
}
