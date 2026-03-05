use async_trait::async_trait;

/// Trait for embedding providers. Implement this to add a new backend
/// (Ollama, OpenAI, etc).
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Human-readable name of the provider.
    fn provider_name(&self) -> &str;

    /// Model identifier (e.g. "nomic-embed-text", "text-embedding-3-small").
    fn model_name(&self) -> &str;

    /// Dimensionality of the output vectors.
    fn dimensions(&self) -> usize;

    /// Embed a batch of text strings, returning one vector per input.
    async fn embed_batch(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>>;

    /// Embed a single text string. Default implementation calls embed_batch.
    async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let results = self.embed_batch(&[text]).await?;
        results
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("embed_batch returned empty results"))
    }
}
