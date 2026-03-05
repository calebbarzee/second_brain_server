//! SkillContext: shared state and helpers for skill execution.

use crate::llm::LlmProvider;
use sb_core::db::{embeddings, notes, projects, tasks};
use sb_core::models::{Note, Task};
use sb_core::Database;
use sb_embed::EmbeddingPipeline;
use std::path::PathBuf;
use std::sync::Arc;

/// Shared context passed to every skill execution.
pub struct SkillContext {
    pub db: Database,
    pub pipeline: Arc<EmbeddingPipeline>,
    pub llm: Option<Arc<dyn LlmProvider>>,
    pub notes_root: PathBuf,
}

/// Result type for LLM calls that may be deferred to the MCP client.
pub enum LlmResult {
    /// LLM produced a response directly
    Response(String),
    /// No LLM available — return the prompt for the MCP client to handle
    Deferred(String),
}

impl SkillContext {
    pub fn new(
        db: Database,
        pipeline: Arc<EmbeddingPipeline>,
        llm: Option<Arc<dyn LlmProvider>>,
        notes_root: PathBuf,
    ) -> Self {
        Self {
            db,
            pipeline,
            llm,
            notes_root,
        }
    }

    /// Get recently updated notes.
    pub async fn get_recent_notes(&self, limit: i64) -> anyhow::Result<Vec<Note>> {
        notes::list_notes(self.db.pool(), limit, 0).await
    }

    /// Get notes in a time range, optionally scoped to a project.
    pub async fn get_notes_in_range(
        &self,
        start: chrono::DateTime<chrono::Utc>,
        end: chrono::DateTime<chrono::Utc>,
        project_id: Option<uuid::Uuid>,
    ) -> anyhow::Result<Vec<Note>> {
        notes::get_notes_in_range(self.db.pool(), start, end, project_id).await
    }

    /// Get notes for a specific project.
    pub async fn get_project_notes(
        &self,
        project_id: uuid::Uuid,
        limit: i64,
    ) -> anyhow::Result<Vec<Note>> {
        projects::get_notes_for_project(self.db.pool(), project_id, limit).await
    }

    /// Get open tasks, optionally scoped to a project.
    pub async fn get_open_tasks(
        &self,
        project_id: Option<uuid::Uuid>,
    ) -> anyhow::Result<Vec<Task>> {
        match project_id {
            Some(pid) => tasks::get_open_tasks_for_project(self.db.pool(), pid).await,
            None => tasks::get_all_open_tasks(self.db.pool(), 100).await,
        }
    }

    /// Perform semantic search.
    pub async fn semantic_search(
        &self,
        query: &str,
        limit: i64,
    ) -> anyhow::Result<Vec<embeddings::SemanticSearchResult>> {
        let query_vector = self.pipeline.embed_query(query).await?;
        embeddings::semantic_search(self.db.pool(), &query_vector, limit).await
    }

    /// Call the LLM, or return a deferred prompt if no LLM is available.
    pub async fn llm_complete(&self, prompt: &str) -> LlmResult {
        match &self.llm {
            Some(provider) => match provider.complete(prompt).await {
                Ok(response) => LlmResult::Response(response),
                Err(e) => {
                    tracing::error!("LLM call failed: {e}");
                    LlmResult::Deferred(prompt.to_string())
                }
            },
            None => LlmResult::Deferred(prompt.to_string()),
        }
    }

    /// Look up a project by name, returning its ID.
    pub async fn resolve_project(
        &self,
        name: &str,
    ) -> anyhow::Result<Option<sb_core::models::Project>> {
        projects::get_project_by_name(self.db.pool(), name).await
    }
}
