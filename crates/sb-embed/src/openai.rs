use crate::provider::EmbeddingProvider;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// OpenAI-compatible embedding provider.
/// Works with OpenAI API, Ollama's OpenAI-compatible endpoint,
/// and any service implementing the /v1/embeddings API.
pub struct OpenAiProvider {
    client: reqwest::Client,
    base_url: String,
    model: String,
    dimensions: usize,
    api_key: Option<String>,
}

impl OpenAiProvider {
    pub fn new(base_url: &str, model: &str, dimensions: usize, api_key: Option<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            dimensions,
            api_key,
        }
    }

    /// Create a provider pointing at OpenAI's API.
    pub fn openai(model: &str, dimensions: usize, api_key: String) -> Self {
        Self::new(
            "https://api.openai.com",
            model,
            dimensions,
            Some(api_key),
        )
    }

    /// Create a provider pointing at a local Ollama instance.
    pub fn ollama(model: &str, dimensions: usize) -> Self {
        Self::new(
            "http://localhost:11434",
            model,
            dimensions,
            None,
        )
    }
}

#[derive(Debug, Serialize)]
struct OpenAiEmbeddingRequest {
    input: Vec<String>,
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    dimensions: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct OpenAiEmbeddingResponse {
    data: Vec<OpenAiEmbeddingData>,
}

#[derive(Debug, Deserialize)]
struct OpenAiEmbeddingData {
    embedding: Vec<f32>,
}

#[async_trait]
impl EmbeddingProvider for OpenAiProvider {
    fn provider_name(&self) -> &str {
        "openai-compatible"
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

        let request = OpenAiEmbeddingRequest {
            input: texts.iter().map(|s| s.to_string()).collect(),
            model: self.model.clone(),
            dimensions: Some(self.dimensions),
        };

        let mut req = self
            .client
            .post(format!("{}/v1/embeddings", self.base_url))
            .json(&request);

        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }

        let response = req.send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("OpenAI embeddings request failed ({}): {}", status, body);
        }

        let result: OpenAiEmbeddingResponse = response.json().await?;

        // OpenAI may return results out of order, but typically they're in order
        let embeddings: Vec<Vec<f32>> = result.data.into_iter().map(|d| d.embedding).collect();

        if embeddings.len() != texts.len() {
            anyhow::bail!(
                "OpenAI returned {} embeddings for {} inputs",
                embeddings.len(),
                texts.len()
            );
        }

        Ok(embeddings)
    }

    async fn unload_model(&self) -> anyhow::Result<()> {
        // Ollama supports unloading via keep_alive: 0 on any endpoint.
        // OpenAI cloud doesn't need this (stateless), so we detect Ollama by
        // checking that the base URL is a local address.
        let is_local = self.base_url.contains("localhost")
            || self.base_url.contains("127.0.0.1")
            || self.base_url.contains("[::1]");
        if !is_local {
            return Ok(());
        }

        let payload = serde_json::json!({
            "model": self.model,
            "keep_alive": 0,
        });

        let res = self
            .client
            .post(format!("{}/api/generate", self.base_url))
            .json(&payload)
            .send()
            .await;

        match res {
            Ok(r) if r.status().is_success() => {
                tracing::info!("unloaded model '{}' from Ollama", self.model);
            }
            Ok(r) => {
                tracing::warn!(
                    "Ollama unload returned {}: {}",
                    r.status(),
                    r.text().await.unwrap_or_default()
                );
            }
            Err(e) => {
                tracing::warn!("failed to unload Ollama model: {e}");
            }
        }

        Ok(())
    }
}
