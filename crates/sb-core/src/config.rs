use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub database: DatabaseConfig,
    pub notes: NotesConfig,
    #[serde(default)]
    pub embedding: EmbeddingConfig,
    #[serde(default)]
    pub projects: Vec<ProjectConfig>,
    pub llm: Option<LlmConfig>,
}

/// Configuration for a project directory to observe.
#[derive(Debug, Deserialize, Clone)]
pub struct ProjectConfig {
    pub name: String,
    pub path: PathBuf,
    #[serde(default = "default_branch")]
    pub branch: String,
    #[serde(default = "default_patterns")]
    pub patterns: Vec<String>,
    /// Subdirectory within KB to mirror project docs into
    pub mirror_to: Option<String>,
}

/// LLM configuration for autonomous skill execution.
#[derive(Debug, Deserialize, Clone)]
pub struct LlmConfig {
    pub provider: String,
    pub model: Option<String>,
    pub api_key: Option<String>,
}

fn default_branch() -> String {
    "main".to_string()
}

fn default_patterns() -> Vec<String> {
    vec![
        "docs/**/*.md".to_string(),
        "README.md".to_string(),
        "**/README.md".to_string(),
    ]
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig {
    pub url: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct NotesConfig {
    /// Directories to watch for markdown notes
    pub paths: Vec<PathBuf>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EmbeddingConfig {
    /// Which provider to use: "tei" or "openai"
    #[serde(default = "default_provider")]
    pub provider: String,
    /// URL of the embedding server (e.g. http://localhost:8090, https://my-remote-server.com)
    #[serde(default = "default_url")]
    pub url: String,
    /// Model name (provider-specific)
    #[serde(default = "default_model")]
    pub model: String,
    /// Embedding dimensions
    #[serde(default = "default_dimensions")]
    pub dimensions: usize,
    /// Batch size for embedding requests
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            url: default_url(),
            model: default_model(),
            dimensions: default_dimensions(),
            batch_size: default_batch_size(),
        }
    }
}

fn default_provider() -> String {
    "tei".to_string()
}
fn default_url() -> String {
    "http://localhost:8090".to_string()
}
fn default_model() -> String {
    "nomic-embed-text".to_string()
}
fn default_dimensions() -> usize {
    768
}
fn default_batch_size() -> usize {
    32
}

impl Config {
    pub fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    /// Load from DATABASE_URL env var with sensible defaults (for dev/testing)
    pub fn from_env() -> anyhow::Result<Self> {
        let database_url =
            std::env::var("DATABASE_URL").unwrap_or_else(|_| {
                "postgresql://secondbrain:secondbrain@localhost:5432/secondbrain".to_string()
            });

        Ok(Self {
            database: DatabaseConfig { url: database_url },
            notes: NotesConfig { paths: vec![] },
            embedding: EmbeddingConfig::default(),
            projects: vec![],
            llm: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_from_toml() {
        let toml_str = r#"
[database]
url = "postgresql://user:pass@localhost:5432/testdb"

[notes]
paths = ["/home/user/notes", "/home/user/projects"]

[embedding]
provider = "openai"
model = "text-embedding-3-small"
dimensions = 1536
batch_size = 64
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.database.url, "postgresql://user:pass@localhost:5432/testdb");
        assert_eq!(config.notes.paths.len(), 2);
        assert_eq!(config.embedding.provider, "openai");
        assert_eq!(config.embedding.dimensions, 1536);
    }

    #[test]
    fn test_config_defaults() {
        let toml_str = r#"
[database]
url = "postgresql://localhost/test"

[notes]
paths = []
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.embedding.provider, "tei");
        assert_eq!(config.embedding.url, "http://localhost:8090");
        assert_eq!(config.embedding.model, "nomic-embed-text");
        assert_eq!(config.embedding.dimensions, 768);
    }
}
