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
    /// Git branch tracked by the shared DB index (default: "main").
    /// Override via TRACKED_BRANCH env var.
    #[serde(default)]
    pub tracked_branch: Option<String>,
    /// Directory for session worktrees (default: sibling of notes path).
    /// Override via WORKTREE_DIR env var.
    #[serde(default)]
    pub worktree_dir: Option<PathBuf>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EmbeddingConfig {
    /// Named preset: "nomic", "nomic-moe", "qwen3", "all-minilm", "snowflake",
    /// "mxbai", "openai-small", "openai-large".
    /// When set, fills in provider/url/model/dimensions/max_chunk_chars defaults
    /// for that model. Explicit fields below override the preset.
    #[serde(default)]
    pub preset: Option<String>,
    /// Which provider to use: "ollama", "openai", "tei"
    #[serde(default)]
    pub provider: Option<String>,
    /// URL of the embedding server
    #[serde(default)]
    pub url: Option<String>,
    /// Model name (provider-specific)
    #[serde(default)]
    pub model: Option<String>,
    /// Embedding dimensions
    #[serde(default)]
    pub dimensions: Option<usize>,
    /// Batch size for embedding requests
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    /// Maximum characters per chunk
    #[serde(default)]
    pub max_chunk_chars: Option<usize>,
}

/// Resolved embedding config with no Option fields — ready to use.
#[derive(Debug, Clone)]
pub struct ResolvedEmbeddingConfig {
    pub provider: String,
    pub url: String,
    pub model: String,
    pub dimensions: usize,
    pub batch_size: usize,
    pub max_chunk_chars: usize,
}

/// Known model preset definitions.
struct Preset {
    provider: &'static str,
    url: &'static str,
    model: &'static str,
    dimensions: usize,
    max_chunk_chars: usize,
}

const PRESETS: &[(&str, Preset)] = &[
    (
        "nomic",
        Preset {
            provider: "ollama",
            url: "http://localhost:11434",
            model: "nomic-embed-text",
            dimensions: 768,
            max_chunk_chars: 2400,
        },
    ),
    (
        "nomic-moe",
        Preset {
            provider: "ollama",
            url: "http://localhost:11434",
            model: "nomic-embed-text-v2-moe",
            dimensions: 768,
            max_chunk_chars: 1200,
        },
    ),
    (
        "all-minilm",
        Preset {
            provider: "ollama",
            url: "http://localhost:11434",
            model: "all-minilm",
            dimensions: 384,
            max_chunk_chars: 1000,
        },
    ),
    (
        "snowflake",
        Preset {
            provider: "ollama",
            url: "http://localhost:11434",
            model: "snowflake-arctic-embed2",
            dimensions: 768,
            max_chunk_chars: 2400,
        },
    ),
    (
        "mxbai",
        Preset {
            provider: "ollama",
            url: "http://localhost:11434",
            model: "mxbai-embed-large",
            dimensions: 1024,
            max_chunk_chars: 1200,
        },
    ),
    (
        "qwen3",
        Preset {
            provider: "ollama",
            url: "http://localhost:11434",
            model: "qwen3-embedding",
            dimensions: 1024,
            max_chunk_chars: 3000,
        },
    ),
    (
        "openai-small",
        Preset {
            provider: "openai",
            url: "https://api.openai.com",
            model: "text-embedding-3-small",
            dimensions: 1536,
            max_chunk_chars: 2400,
        },
    ),
    (
        "openai-large",
        Preset {
            provider: "openai",
            url: "https://api.openai.com",
            model: "text-embedding-3-large",
            dimensions: 3072,
            max_chunk_chars: 2400,
        },
    ),
    (
        "tei",
        Preset {
            provider: "tei",
            url: "http://localhost:8090",
            model: "BAAI/bge-base-en-v1.5",
            dimensions: 768,
            max_chunk_chars: 1200,
        },
    ),
];

/// Default preset when nothing is configured.
const DEFAULT_PRESET: &str = "nomic";

fn lookup_preset(name: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|(n, _)| *n == name).map(|(_, p)| p)
}

impl EmbeddingConfig {
    /// Resolve optional/preset fields into a concrete config.
    /// Priority: explicit field > env var > preset > built-in default.
    pub fn resolve(&self) -> ResolvedEmbeddingConfig {
        let preset_name = self
            .preset
            .as_deref()
            .or_else(|| {
                std::env::var("EMBEDDING_PRESET")
                    .ok()
                    .as_deref()
                    .map(|_| unreachable!())
            })
            .unwrap_or(DEFAULT_PRESET);

        // Allow env var to set the preset too
        let preset_name = std::env::var("EMBEDDING_PRESET")
            .ok()
            .unwrap_or_else(|| preset_name.to_string());

        let preset = lookup_preset(&preset_name);
        let fallback = lookup_preset(DEFAULT_PRESET).unwrap();

        let resolve_str = |explicit: &Option<String>, env_key: &str, preset_val: &str| -> String {
            if let Some(v) = explicit {
                return v.clone();
            }
            if let Ok(v) = std::env::var(env_key) {
                return v;
            }
            preset_val.to_string()
        };

        let resolve_usize = |explicit: Option<usize>, env_key: &str, preset_val: usize| -> usize {
            if let Some(v) = explicit {
                return v;
            }
            if let Ok(v) = std::env::var(env_key)
                && let Ok(n) = v.parse()
            {
                return n;
            }
            preset_val
        };

        let p = preset.unwrap_or(fallback);

        ResolvedEmbeddingConfig {
            provider: resolve_str(&self.provider, "EMBEDDING_PROVIDER", p.provider),
            url: resolve_str(&self.url, "EMBEDDING_URL", p.url),
            model: resolve_str(&self.model, "EMBEDDING_MODEL", p.model),
            dimensions: resolve_usize(self.dimensions, "EMBEDDING_DIMS", p.dimensions),
            batch_size: self.batch_size,
            max_chunk_chars: resolve_usize(
                self.max_chunk_chars,
                "EMBEDDING_MAX_CHUNK_CHARS",
                p.max_chunk_chars,
            ),
        }
    }
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            preset: None,
            provider: None,
            url: None,
            model: None,
            dimensions: None,
            batch_size: default_batch_size(),
            max_chunk_chars: None,
        }
    }
}

/// Return available preset names (for CLI help / setup).
pub fn available_presets() -> Vec<&'static str> {
    PRESETS.iter().map(|(name, _)| *name).collect()
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

    /// Load configuration by trying, in order:
    /// 1. `second-brain.toml` in the current directory
    /// 2. `second-brain.toml` next to the running binary
    /// 3. Fallback to env vars / defaults
    pub fn from_env() -> anyhow::Result<Self> {
        // Try to find and load the TOML config file
        let toml_paths = [
            std::env::current_dir()
                .ok()
                .map(|p| p.join("second-brain.toml")),
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("second-brain.toml"))),
        ];

        for candidate in toml_paths.iter().flatten() {
            if candidate.is_file() {
                match Self::load(candidate) {
                    Ok(mut config) => {
                        // Allow DATABASE_URL env var to override the TOML value
                        if let Ok(url) = std::env::var("DATABASE_URL") {
                            config.database.url = url;
                        }
                        return Ok(config);
                    }
                    Err(e) => {
                        tracing::warn!("failed to load {}: {e}", candidate.display());
                    }
                }
            }
        }

        // Pure env-var fallback (no TOML found)
        let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgresql://secondbrain:secondbrain@localhost:5432/secondbrain".to_string()
        });

        Ok(Self {
            database: DatabaseConfig { url: database_url },
            notes: NotesConfig {
                paths: vec![],
                tracked_branch: None,
                worktree_dir: None,
            },
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
        assert_eq!(
            config.database.url,
            "postgresql://user:pass@localhost:5432/testdb"
        );
        assert_eq!(config.notes.paths.len(), 2);
        let resolved = config.embedding.resolve();
        assert_eq!(resolved.provider, "openai");
        assert_eq!(resolved.dimensions, 1536);
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
        let resolved = config.embedding.resolve();
        // Default preset is "nomic"
        assert_eq!(resolved.provider, "ollama");
        assert_eq!(resolved.url, "http://localhost:11434");
        assert_eq!(resolved.model, "nomic-embed-text");
        assert_eq!(resolved.dimensions, 768);
    }

    #[test]
    fn test_preset_override() {
        let toml_str = r#"
[database]
url = "postgresql://localhost/test"

[notes]
paths = []

[embedding]
preset = "qwen3"
dimensions = 512
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        let resolved = config.embedding.resolve();
        assert_eq!(resolved.model, "qwen3-embedding");
        assert_eq!(resolved.dimensions, 512); // explicit override
        assert_eq!(resolved.provider, "ollama");
    }

    #[test]
    fn test_preset_list() {
        let presets = super::available_presets();
        assert!(presets.contains(&"nomic"));
        assert!(presets.contains(&"qwen3"));
        assert!(presets.contains(&"openai-small"));
    }
}
