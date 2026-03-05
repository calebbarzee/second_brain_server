use crate::provider::EmbeddingProvider;
use async_trait::async_trait;
use serde::Serialize;

/// Hugging Face Text Embeddings Inference provider.
/// Also works with any server implementing the TEI /embed endpoint
/// (including Ollama's /api/embeddings with adapter).
pub struct TeiProvider {
    client: reqwest::Client,
    base_url: String,
    model: String,
    dimensions: usize,
}

impl TeiProvider {
    pub fn new(base_url: &str, model: &str, dimensions: usize) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            dimensions,
        }
    }
}

#[derive(Debug, Serialize)]
struct TeiRequest {
    inputs: Vec<String>,
}

// TEI returns Vec<Vec<f32>> directly
type TeiResponse = Vec<Vec<f32>>;

#[async_trait]
impl EmbeddingProvider for TeiProvider {
    fn provider_name(&self) -> &str {
        "tei"
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    async fn embed_batch(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        let request = TeiRequest {
            inputs: texts.iter().map(|s| s.to_string()).collect(),
        };

        let response = self
            .client
            .post(format!("{}/embed", self.base_url))
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("TEI request failed ({}): {}", status, body);
        }

        let embeddings: TeiResponse = response.json().await?;

        if embeddings.len() != texts.len() {
            anyhow::bail!(
                "TEI returned {} embeddings for {} inputs",
                embeddings.len(),
                texts.len()
            );
        }

        Ok(embeddings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_tei_embed_single() {
        // Only runs if TEI is available
        let provider = TeiProvider::new("http://localhost:8090", "BAAI/bge-base-en-v1.5", 768);

        let result = provider.embed("Hello, world!").await;
        if let Ok(vec) = result {
            assert_eq!(vec.len(), 768);
            // Vectors should be non-zero
            assert!(vec.iter().any(|&v| v != 0.0));
        } else {
            eprintln!("Skipping TEI test (server not available): {:?}", result.err());
        }
    }

    #[tokio::test]
    async fn test_tei_embed_batch() {
        let provider = TeiProvider::new("http://localhost:8090", "BAAI/bge-base-en-v1.5", 768);

        let result = provider
            .embed_batch(&["First text", "Second text", "Third text"])
            .await;
        if let Ok(vecs) = result {
            assert_eq!(vecs.len(), 3);
            for vec in &vecs {
                assert_eq!(vec.len(), 768);
            }
        } else {
            eprintln!("Skipping TEI batch test: {:?}", result.err());
        }
    }

    #[tokio::test]
    async fn test_tei_empty_batch() {
        let provider = TeiProvider::new("http://localhost:8090", "BAAI/bge-base-en-v1.5", 768);
        let result = provider.embed_batch(&[]).await.unwrap();
        assert!(result.is_empty());
    }
}
