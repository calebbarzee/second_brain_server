//! LLM provider trait for skill execution.
//!
//! Separate from EmbeddingProvider — this is for text generation (chat/completions).

use async_trait::async_trait;

/// A message in a chat conversation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
        }
    }
}

/// Trait for LLM text generation providers.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn provider_name(&self) -> &str;
    fn model_name(&self) -> &str;

    /// Single-turn completion.
    async fn complete(&self, prompt: &str) -> anyhow::Result<String>;

    /// Multi-turn chat completion.
    async fn chat(&self, messages: &[ChatMessage]) -> anyhow::Result<String>;
}
