use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    schemars, tool, tool_handler, tool_router,
};
use sb_core::Database;
use sb_core::ingest::{self, IngestResult};
use sb_core::path_map::PathMapper;
use sb_core::worktree::{SessionInfo, WorktreeConfig};
use sb_embed::EmbeddingPipeline;
use sb_skills::SkillRunner;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;

// ── Tool parameter types ───────────────────────────────────────

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct NoteSearchParams {
    /// The search query (full-text search over note titles and content)
    pub query: String,
    /// Maximum number of results to return (default: 10)
    #[serde(default = "default_limit")]
    pub limit: Option<i64>,
    /// Filter by lifecycle: active, volatile, enduring, archived
    #[serde(default)]
    pub lifecycle: Option<String>,
    /// Filter by project name
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct NoteReadParams {
    /// File path of the note to read
    pub file_path: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct NoteListParams {
    /// Maximum number of results to return (default: 20)
    #[serde(default = "default_list_limit")]
    pub limit: Option<i64>,
    /// Number of results to skip (for pagination)
    #[serde(default)]
    pub offset: Option<i64>,
    /// Filter by lifecycle: active, volatile, enduring, archived
    #[serde(default)]
    pub lifecycle: Option<String>,
    /// Filter by project name
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct NoteIngestParams {
    /// Path to a file or directory to ingest
    pub path: String,
    /// Also generate embeddings for ingested notes (default: true)
    #[serde(default = "default_true")]
    pub embed: Option<bool>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SemanticSearchParams {
    /// Natural language query to search for semantically similar content
    pub query: String,
    /// Maximum number of results to return (default: 10)
    #[serde(default = "default_limit")]
    pub limit: Option<i64>,
    /// Filter by lifecycle: active, volatile, enduring, archived
    #[serde(default)]
    pub lifecycle: Option<String>,
    /// Filter by project name
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct FindRelatedParams {
    /// File path of the note to find related content for
    pub file_path: String,
    /// Maximum number of related results to return (default: 10)
    #[serde(default = "default_limit")]
    pub limit: Option<i64>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct EmbedNotesParams {
    /// Process all notes that don't have embeddings yet.
    /// If false, re-embed everything (default: true, only unembedded)
    #[serde(default = "default_true")]
    pub only_new: Option<bool>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct NoteCreateParams {
    /// File path where the new note should be created (must end in .md)
    pub file_path: String,
    /// The markdown content for the new note
    pub content: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct NoteUpdateParams {
    /// File path of the note to update
    pub file_path: String,
    /// The new markdown content (replaces the entire note)
    pub content: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct NoteGraphParams {
    /// File path of the note to get the link graph for
    pub file_path: String,
}

// ── Phase 4 tool params ───────────────────────────────────────

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RunSkillParams {
    /// Skill name: summarize, continue-work, reflect, connect-ideas, contextualize
    pub skill: String,
    /// Time period: today, yesterday, this-week, last-week, this-month, YYYY-MM-DD, or YYYY-MM-DD..YYYY-MM-DD
    #[serde(default)]
    pub period: Option<String>,
    /// Project name to scope the skill to
    #[serde(default)]
    pub project: Option<String>,
    /// Preview mode — return changeset without applying (default for destructive skills)
    #[serde(default)]
    pub dry_run: Option<bool>,
    /// Allow destructive skills to write changes
    #[serde(default)]
    pub allow_writes: Option<bool>,
    /// Write output as a new note (for summarize, reflect)
    #[serde(default)]
    pub write_output: Option<bool>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ProjectListParams {
    // No parameters needed — lists all projects
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ProjectContextParams {
    /// Project name to get context for
    pub project: String,
    /// Maximum number of recent notes to include (default: 20)
    #[serde(default = "default_list_limit")]
    pub limit: Option<i64>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct NoteClassifyParams {
    /// File path of the note to classify
    pub file_path: String,
    /// Lifecycle to set: active, volatile, enduring, archived
    pub lifecycle: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct FileSearchParams {
    /// Search query (searches file contents using ripgrep, or filenames if mode is "filename")
    pub query: String,
    /// Search mode: "content" (default, uses ripgrep) or "filename" (uses fd/find)
    #[serde(default = "default_search_mode")]
    pub mode: Option<String>,
    /// Maximum number of results (default: 20)
    #[serde(default = "default_list_limit")]
    pub limit: Option<i64>,
    /// Directories to search (defaults to configured watch paths)
    #[serde(default)]
    pub paths: Option<Vec<String>>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct NoteStampParams {
    /// File path of the note to stamp
    pub file_path: String,
    /// Who made the edit — your username (e.g. "calebbarzee") or "ai"
    pub editor: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SessionInitParams {
    /// Your username (e.g. "calebbarzee"). Used for branch prefix and edit attribution.
    pub username: String,
    /// Your email address (for git commits)
    pub email: String,
    /// Branch name to work on (default: "<username>/working"). Must be prefixed with your username.
    #[serde(default)]
    pub branch: Option<String>,
}

fn default_search_mode() -> Option<String> {
    Some("content".to_string())
}

fn default_limit() -> Option<i64> {
    Some(10)
}
fn default_list_limit() -> Option<i64> {
    Some(20)
}
fn default_true() -> Option<bool> {
    Some(true)
}

// ── MCP Server ─────────────────────────────────────────────────

#[derive(Clone)]
pub struct SecondBrainServer {
    tool_router: ToolRouter<Self>,
    db: Database,
    pipeline: Arc<EmbeddingPipeline>,
    skill_runner: Arc<SkillRunner>,
    notes_paths: Vec<std::path::PathBuf>,
    /// Per-session worktree state. Set by session_init, None until then.
    session: Arc<Mutex<Option<SessionInfo>>>,
    /// Worktree configuration (None in stdio mode).
    worktree_config: Option<WorktreeConfig>,
    /// PathMapper for the main repo (used for read-only operations and fallback).
    main_mapper: PathMapper,
}

#[tool_router]
impl SecondBrainServer {
    pub fn new(
        db: Database,
        pipeline: Arc<EmbeddingPipeline>,
        skill_runner: Arc<SkillRunner>,
        notes_paths: Vec<std::path::PathBuf>,
        worktree_config: Option<WorktreeConfig>,
        main_mapper: PathMapper,
    ) -> Self {
        Self {
            tool_router: Self::tool_router(),
            db,
            pipeline,
            skill_runner,
            notes_paths,
            session: Arc::new(Mutex::new(None)),
            worktree_config,
            main_mapper,
        }
    }

    // ── Phase 1 tools ──────────────────────────────────────────

    #[tool(
        description = "Search notes using full-text search over titles and content. Returns matching notes ranked by relevance. Use semantic_search for meaning-based search."
    )]
    async fn note_search(
        &self,
        Parameters(params): Parameters<NoteSearchParams>,
    ) -> Result<CallToolResult, McpError> {
        let limit = params.limit.unwrap_or(10);

        // Resolve optional project filter
        let project_id = resolve_project_id(&self.db, params.project.as_deref()).await;

        let notes = if params.lifecycle.is_some() || project_id.is_some() {
            sb_core::db::notes::search_notes_filtered(
                self.db.pool(),
                &params.query,
                params.lifecycle.as_deref(),
                project_id,
                limit,
            )
            .await
        } else {
            sb_core::db::notes::search_notes(self.db.pool(), &params.query, limit).await
        }
        .map_err(|e| McpError::internal_error(format!("search failed: {e}"), None))?;

        if notes.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No notes found matching your query.",
            )]));
        }

        let mut results = Vec::new();
        for note in &notes {
            results.push(serde_json::json!({
                "file_path": note.file_path,
                "title": note.title,
                "lifecycle": note.lifecycle,
                "updated_at": note.updated_at.to_rfc3339(),
            }));
        }

        let output = serde_json::to_string_pretty(&results)
            .map_err(|e| McpError::internal_error(format!("serialization failed: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Found {} notes:\n\n{}",
            notes.len(),
            output
        ))]))
    }

    #[tool(
        description = "Read the full content of a note by its file path. Returns the title, content, frontmatter, and metadata."
    )]
    async fn note_read(
        &self,
        Parameters(params): Parameters<NoteReadParams>,
    ) -> Result<CallToolResult, McpError> {
        let note = sb_core::db::notes::get_note_by_path(self.db.pool(), &params.file_path)
            .await
            .map_err(|e| McpError::internal_error(format!("read failed: {e}"), None))?;

        match note {
            Some(note) => {
                let result = serde_json::json!({
                    "title": note.title,
                    "file_path": note.file_path,
                    "content": note.raw_content,
                    "frontmatter": note.frontmatter,
                    "lifecycle": note.lifecycle,
                    "source_project": note.source_project,
                    "updated_at": note.updated_at.to_rfc3339(),
                    "created_at": note.created_at.to_rfc3339(),
                });
                let output = serde_json::to_string_pretty(&result).map_err(|e| {
                    McpError::internal_error(format!("serialization failed: {e}"), None)
                })?;
                Ok(CallToolResult::success(vec![Content::text(output)]))
            }
            None => Ok(CallToolResult::success(vec![Content::text(format!(
                "Note not found: {}",
                params.file_path
            ))])),
        }
    }

    #[tool(
        description = "List all notes in the database, ordered by most recently updated. Supports pagination via limit and offset."
    )]
    async fn note_list(
        &self,
        Parameters(params): Parameters<NoteListParams>,
    ) -> Result<CallToolResult, McpError> {
        let limit = params.limit.unwrap_or(20);
        let offset = params.offset.unwrap_or(0);

        let project_id = resolve_project_id(&self.db, params.project.as_deref()).await;

        let notes = if params.lifecycle.is_some() || project_id.is_some() {
            sb_core::db::notes::list_notes_filtered(
                self.db.pool(),
                params.lifecycle.as_deref(),
                project_id,
                limit,
                offset,
            )
            .await
        } else {
            sb_core::db::notes::list_notes(self.db.pool(), limit, offset).await
        }
        .map_err(|e| McpError::internal_error(format!("list failed: {e}"), None))?;

        if notes.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No notes in the database. Use note_ingest to add notes.",
            )]));
        }

        let mut results = Vec::new();
        for note in &notes {
            results.push(serde_json::json!({
                "file_path": note.file_path,
                "title": note.title,
                "lifecycle": note.lifecycle,
                "updated_at": note.updated_at.to_rfc3339(),
            }));
        }

        let output = serde_json::to_string_pretty(&results)
            .map_err(|e| McpError::internal_error(format!("serialization failed: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Listing {} notes (offset {}):\n\n{}",
            notes.len(),
            offset,
            output
        ))]))
    }

    #[tool(
        description = "Ingest markdown files into the database and generate embeddings. Provide a path to a single .md file or a directory to recursively ingest all markdown files. Files are content-hashed to skip unchanged notes. Also extracts and stores inter-note links."
    )]
    async fn note_ingest(
        &self,
        Parameters(params): Parameters<NoteIngestParams>,
    ) -> Result<CallToolResult, McpError> {
        let path = Path::new(&params.path);
        let should_embed = params.embed.unwrap_or(true);

        if !path.exists() {
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "Path does not exist: {}",
                params.path
            ))]));
        }

        let mapper = self.active_mapper();
        let stats = if path.is_file() {
            let mut stats = ingest::IngestStats::default();
            match ingest::ingest_file(&self.db, path, &mapper).await {
                Ok(IngestResult::Ingested(info)) => {
                    stats.ingested = 1;
                    stats.links_stored = info.links_stored;
                    stats.ingested_note_ids.push(info.note_id);
                }
                Ok(IngestResult::Skipped) => {
                    stats.skipped = 1;
                }
                Err(e) => {
                    stats.errors.push(format!("{}: {e}", path.display()));
                }
            }
            stats
        } else {
            ingest::ingest_directory(&self.db, path, &mapper)
                .await
                .map_err(|e| McpError::internal_error(format!("ingest failed: {e}"), None))?
        };

        let mut msg = format!(
            "Ingestion complete: {} ingested, {} skipped (unchanged), {} links stored",
            stats.ingested, stats.skipped, stats.links_stored
        );

        if should_embed && stats.ingested > 0 {
            match self.pipeline.process_unembedded(self.db.pool()).await {
                Ok(embed_stats) => {
                    msg.push_str(&format!(
                        "\nEmbedding: {} notes processed, {} chunks, {} embeddings created",
                        embed_stats.notes_processed,
                        embed_stats.chunks_created,
                        embed_stats.embeddings_created
                    ));
                    if !embed_stats.errors.is_empty() {
                        msg.push_str(&format!(
                            "\nEmbedding errors: {}",
                            embed_stats.errors.join("; ")
                        ));
                    }
                }
                Err(e) => {
                    msg.push_str(&format!("\nEmbedding failed: {e}"));
                }
            }
        }

        if !stats.errors.is_empty() {
            msg.push_str(&format!(
                "\n\nIngestion errors ({}):\n{}",
                stats.errors.len(),
                stats.errors.join("\n")
            ));
        }

        Ok(CallToolResult::success(vec![Content::text(msg)]))
    }

    // ── Phase 2 tools: Semantic Search ─────────────────────────

    #[tool(
        description = "Search notes using semantic similarity (vector embeddings). Finds content that is conceptually related to your query, even if it doesn't contain the exact words. Returns the most relevant chunks with their source notes and similarity scores."
    )]
    async fn semantic_search(
        &self,
        Parameters(params): Parameters<SemanticSearchParams>,
    ) -> Result<CallToolResult, McpError> {
        let limit = params.limit.unwrap_or(10);

        let query_vector = self
            .pipeline
            .embed_query(&params.query)
            .await
            .map_err(|e| McpError::internal_error(format!("query embedding failed: {e}"), None))?;

        let project_id = resolve_project_id(&self.db, params.project.as_deref()).await;

        let results = if params.lifecycle.is_some() || project_id.is_some() {
            sb_core::db::embeddings::semantic_search_filtered(
                self.db.pool(),
                &query_vector,
                params.lifecycle.as_deref(),
                project_id,
                limit,
            )
            .await
        } else {
            sb_core::db::embeddings::semantic_search(self.db.pool(), &query_vector, limit).await
        }
        .map_err(|e| McpError::internal_error(format!("semantic search failed: {e}"), None))?;

        if results.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No semantically similar content found. Have notes been ingested and embedded?",
            )]));
        }

        let mut output_items = Vec::new();
        for (i, r) in results.iter().enumerate() {
            output_items.push(serde_json::json!({
                "rank": i + 1,
                "similarity": format!("{:.3}", r.similarity),
                "note_title": r.note_title,
                "note_file_path": r.note_file_path,
                "section": r.heading_context,
                "content_preview": truncate(&r.chunk_content, 300),
            }));
        }

        let output = serde_json::to_string_pretty(&output_items)
            .map_err(|e| McpError::internal_error(format!("serialization failed: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Found {} semantically similar results:\n\n{}",
            results.len(),
            output
        ))]))
    }

    #[tool(
        description = "Find notes that are semantically related to a given note. Uses the average embedding of the note's chunks to find similar content in other notes."
    )]
    async fn find_related(
        &self,
        Parameters(params): Parameters<FindRelatedParams>,
    ) -> Result<CallToolResult, McpError> {
        let limit = params.limit.unwrap_or(10);

        let note = sb_core::db::notes::get_note_by_path(self.db.pool(), &params.file_path)
            .await
            .map_err(|e| McpError::internal_error(format!("lookup failed: {e}"), None))?;

        let note = match note {
            Some(n) => n,
            None => {
                return Ok(CallToolResult::success(vec![Content::text(format!(
                    "Note not found: {}",
                    params.file_path
                ))]));
            }
        };

        let results = sb_core::db::embeddings::find_related_notes(self.db.pool(), note.id, limit)
            .await
            .map_err(|e| McpError::internal_error(format!("find_related failed: {e}"), None))?;

        if results.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No related notes found. The note may not be embedded yet, or there are no other embedded notes.",
            )]));
        }

        let mut seen_notes = std::collections::HashSet::new();
        let mut unique_results = Vec::new();
        for r in &results {
            if seen_notes.insert(r.note_id) {
                unique_results.push(r);
            }
        }

        let mut output_items = Vec::new();
        for (i, r) in unique_results.iter().enumerate() {
            output_items.push(serde_json::json!({
                "rank": i + 1,
                "similarity": format!("{:.3}", r.similarity),
                "note_title": r.note_title,
                "note_file_path": r.note_file_path,
                "content_preview": truncate(&r.chunk_content, 200),
            }));
        }

        let output = serde_json::to_string_pretty(&output_items)
            .map_err(|e| McpError::internal_error(format!("serialization failed: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Notes related to '{}':\n\n{}",
            note.title, output
        ))]))
    }

    #[tool(
        description = "Generate embeddings for all notes that haven't been embedded yet. Run this after ingesting notes if embedding was skipped, or to catch up on any unembedded notes."
    )]
    async fn embed_notes(
        &self,
        Parameters(params): Parameters<EmbedNotesParams>,
    ) -> Result<CallToolResult, McpError> {
        let only_new = params.only_new.unwrap_or(true);

        let stats = if only_new {
            self.pipeline.process_unembedded(self.db.pool()).await
        } else {
            self.pipeline.process_all(self.db.pool()).await
        }
        .map_err(|e| McpError::internal_error(format!("embedding failed: {e}"), None))?;

        let total = sb_core::db::embeddings::count_embeddings(self.db.pool())
            .await
            .unwrap_or(0);

        let mode = if only_new {
            "new only"
        } else {
            "full re-embed"
        };
        let mut msg = format!(
            "Embedding complete ({}): {} notes processed, {} chunks created, {} embeddings generated\nTotal embeddings in database: {}",
            mode, stats.notes_processed, stats.chunks_created, stats.embeddings_created, total
        );

        if !stats.errors.is_empty() {
            msg.push_str(&format!("\nErrors: {}", stats.errors.join("; ")));
        }

        // Unload embedding model from memory after batch job completes
        if let Err(e) = self.pipeline.unload_model().await {
            tracing::warn!("failed to unload embedding model: {e}");
        }

        Ok(CallToolResult::success(vec![Content::text(msg)]))
    }

    // ── Phase 3 tools: Sync & Links ─────────────────────────────

    #[tool(
        description = "Create a new markdown note at the given file path. Writes the file to disk and ingests it into the database with embeddings. Parent directories are created automatically. The note is automatically tagged with AI-edit metadata in its frontmatter, and changes are committed to git if the notes directory is a git repo."
    )]
    async fn note_create(
        &self,
        Parameters(params): Parameters<NoteCreateParams>,
    ) -> Result<CallToolResult, McpError> {
        if !params.file_path.ends_with(".md") {
            return Ok(CallToolResult::error(vec![Content::text(
                "File path must end with .md",
            )]));
        }

        let session = self.require_session()?;
        let mapper = self.active_mapper();
        let path = mapper.to_absolute(&params.file_path);
        let path = path.as_path();

        if path.exists() {
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "File already exists: {}. Use note_update to modify it.",
                params.file_path
            ))]));
        }

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| McpError::internal_error(format!("mkdir failed: {e}"), None))?;
        }

        // Stamp AI-edit metadata into frontmatter
        let stamped_content = sb_core::markdown::stamp_edit(&params.content, "ai");

        std::fs::write(path, &stamped_content)
            .map_err(|e| McpError::internal_error(format!("write failed: {e}"), None))?;

        // Git auto-commit (only this file)
        let git_msg = self.git_commit_file(
            path,
            &format!("[second-brain] create: {}", params.file_path),
        );

        match ingest::ingest_file(&self.db, path, &mapper).await {
            Ok(IngestResult::Ingested(info)) => {
                let mut msg = format!(
                    "Created note '{}' at {}\nLinks stored: {}",
                    info.title, info.file_path, info.links_stored
                );

                if let Some(note) = sb_core::db::notes::get_note_by_id(self.db.pool(), info.note_id)
                    .await
                    .map_err(|e| McpError::internal_error(format!("lookup failed: {e}"), None))?
                {
                    match self.pipeline.process_note(self.db.pool(), &note).await {
                        Ok((chunks, embeddings)) => {
                            msg.push_str(&format!(
                                "\nEmbedded: {} chunks, {} embeddings",
                                chunks, embeddings
                            ));
                        }
                        Err(e) => {
                            msg.push_str(&format!("\nEmbedding failed: {e}"));
                        }
                    }
                }

                msg.push_str(&format!(
                    "\nFrontmatter: edited_by=ai tag added\nBranch: {}",
                    session.branch
                ));
                if let Some((branch, sha)) = git_msg {
                    msg.push_str(&format!("\nGit: committed to {branch} ({sha})"));
                }

                Ok(CallToolResult::success(vec![Content::text(msg)]))
            }
            Ok(IngestResult::Skipped) => Ok(CallToolResult::success(vec![Content::text(
                "Note created on disk but skipped ingestion (content hash matched).",
            )])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "File created but ingestion failed: {e}"
            ))])),
        }
    }

    #[tool(
        description = "Update an existing note's content. Shows a diff of changes, tags the note with AI-edit metadata in frontmatter, writes to disk, re-ingests into the database, and commits to git. The response includes a unified diff so you can verify the changes."
    )]
    async fn note_update(
        &self,
        Parameters(params): Parameters<NoteUpdateParams>,
    ) -> Result<CallToolResult, McpError> {
        let _session = self.require_session()?;
        let mapper = self.active_mapper();
        let path = mapper.to_absolute(&params.file_path);
        let path = path.as_path();

        if !path.exists() {
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "File not found: {}. Use note_create to create a new note.",
                params.file_path
            ))]));
        }

        // Read old content for diff
        let old_content = std::fs::read_to_string(path)
            .map_err(|e| McpError::internal_error(format!("read failed: {e}"), None))?;

        // Stamp AI-edit metadata into frontmatter
        let stamped_content = sb_core::markdown::stamp_edit(&params.content, "ai");

        // Compute a simple unified diff
        let diff = unified_diff(&old_content, &stamped_content, &params.file_path);

        std::fs::write(path, &stamped_content)
            .map_err(|e| McpError::internal_error(format!("write failed: {e}"), None))?;

        // Git auto-commit (only this file)
        let git_sha = self.git_commit_file(
            path,
            &format!("[second-brain] update: {}", params.file_path),
        );

        match ingest::ingest_file(&self.db, path, &mapper).await {
            Ok(IngestResult::Ingested(info)) => {
                let mut msg = format!(
                    "Updated note '{}' at {}\nLinks stored: {}",
                    info.title, info.file_path, info.links_stored
                );

                if let Some(note) = sb_core::db::notes::get_note_by_id(self.db.pool(), info.note_id)
                    .await
                    .map_err(|e| McpError::internal_error(format!("lookup failed: {e}"), None))?
                {
                    match self.pipeline.process_note(self.db.pool(), &note).await {
                        Ok((chunks, embeddings)) => {
                            msg.push_str(&format!(
                                "\nRe-embedded: {} chunks, {} embeddings",
                                chunks, embeddings
                            ));
                        }
                        Err(e) => {
                            msg.push_str(&format!("\nRe-embedding failed: {e}"));
                        }
                    }
                }

                msg.push_str("\nFrontmatter: edited_by=ai tag updated");
                if let Some((branch, sha)) = git_sha {
                    msg.push_str(&format!("\nGit: committed to {branch} ({sha})"));
                }
                if !diff.is_empty() {
                    msg.push_str(&format!("\n\n--- Diff ---\n{diff}"));
                }

                Ok(CallToolResult::success(vec![Content::text(msg)]))
            }
            Ok(IngestResult::Skipped) => Ok(CallToolResult::success(vec![Content::text(
                "Content unchanged — no update needed.",
            )])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "File updated but re-ingestion failed: {e}"
            ))])),
        }
    }

    #[tool(
        description = "Get the link graph for a note — all outbound links from the note and all inbound links (backlinks) to it. Useful for understanding how a note connects to other notes."
    )]
    async fn note_graph(
        &self,
        Parameters(params): Parameters<NoteGraphParams>,
    ) -> Result<CallToolResult, McpError> {
        let note = sb_core::db::notes::get_note_by_path(self.db.pool(), &params.file_path)
            .await
            .map_err(|e| McpError::internal_error(format!("lookup failed: {e}"), None))?;

        let note = match note {
            Some(n) => n,
            None => {
                return Ok(CallToolResult::success(vec![Content::text(format!(
                    "Note not found: {}",
                    params.file_path
                ))]));
            }
        };

        let graph = sb_core::db::links::get_link_graph(self.db.pool(), note.id)
            .await
            .map_err(|e| McpError::internal_error(format!("graph query failed: {e}"), None))?;

        if graph.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "No links found for '{}'. The note has no outbound or inbound links.",
                note.title
            ))]));
        }

        let mut outbound = Vec::new();
        let mut inbound = Vec::new();

        for entry in &graph {
            let item = serde_json::json!({
                "link_text": entry.link_text,
                "target_path": entry.target_path,
                "linked_note_title": entry.linked_note_title,
                "linked_note_path": entry.linked_note_path,
                "resolved": entry.linked_note_title.is_some(),
            });
            match entry.direction {
                "outbound" => outbound.push(item),
                "inbound" => inbound.push(item),
                _ => {}
            }
        }

        let result = serde_json::json!({
            "note_title": note.title,
            "note_file_path": note.file_path,
            "outbound_links": outbound,
            "inbound_links": inbound,
        });

        let output = serde_json::to_string_pretty(&result)
            .map_err(|e| McpError::internal_error(format!("serialization failed: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    // ── Phase 4 tools: Skills + Projects ───────────────────────

    #[tool(
        description = "Run a skill (composable workflow) by name. Available skills: summarize (activity summary), continue-work (resume project context), reflect (patterns and review), connect-ideas (cross-project connections), contextualize (auto-tag/link/classify). Skills gather context from the DB and return structured data for analysis. Destructive skills (contextualize) run in preview mode unless allow_writes is true."
    )]
    async fn run_skill(
        &self,
        Parameters(params): Parameters<RunSkillParams>,
    ) -> Result<CallToolResult, McpError> {
        let skill_params = sb_skills::SkillParams {
            period: params.period,
            project: params.project,
            dry_run: params.dry_run.unwrap_or(false),
            allow_writes: params.allow_writes.unwrap_or(false),
            write_output: params.write_output.unwrap_or(false),
        };

        let output = self
            .skill_runner
            .run(&params.skill, &skill_params)
            .await
            .map_err(|e| McpError::internal_error(format!("skill failed: {e}"), None))?;

        // Build response
        let mut parts = vec![output.summary.clone()];

        if !output.notes_created.is_empty() {
            parts.push(format!(
                "\nNotes created: {}",
                output.notes_created.join(", ")
            ));
        }
        if !output.notes_modified.is_empty() {
            parts.push(format!(
                "\nNotes modified: {}",
                output.notes_modified.join(", ")
            ));
        }
        if let Some(diff) = &output.git_diff
            && !diff.is_empty()
        {
            parts.push(format!("\nGit diff:\n```\n{}\n```", truncate(diff, 2000)));
        }

        // Include structured context for Claude
        if let Some(ctx) = &output.context {
            parts.push(format!(
                "\n\nStructured context:\n{}",
                serde_json::to_string_pretty(ctx).unwrap_or_default()
            ));
        }

        // Include deferred prompt
        if let Some(prompt) = &output.deferred_prompt {
            parts.push(format!("\n\n---\n{}", prompt));
        }

        // Include changeset for preview
        if let Some(changeset) = &output.changeset {
            parts.push(format!(
                "\n\nProposed changes (preview):\n{}",
                serde_json::to_string_pretty(changeset).unwrap_or_default()
            ));
        }

        Ok(CallToolResult::success(vec![Content::text(
            parts.join("\n"),
        )]))
    }

    #[tool(
        description = "List all projects with note counts. Projects are detected from note filenames and configured project directories."
    )]
    async fn project_list(
        &self,
        Parameters(_params): Parameters<ProjectListParams>,
    ) -> Result<CallToolResult, McpError> {
        let projects = sb_core::db::projects::list_projects_with_counts(self.db.pool())
            .await
            .map_err(|e| McpError::internal_error(format!("project list failed: {e}"), None))?;

        if projects.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No projects found. Projects are auto-detected from note filenames (e.g., <project_name>_foo.md) or configured in second-brain.toml.",
            )]));
        }

        let items: Vec<_> = projects
            .iter()
            .map(|p| {
                serde_json::json!({
                    "name": p.project_name,
                    "note_count": p.note_count,
                })
            })
            .collect();

        let output = serde_json::to_string_pretty(&items)
            .map_err(|e| McpError::internal_error(format!("serialization failed: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Projects ({}):\n\n{}",
            projects.len(),
            output
        ))]))
    }

    #[tool(
        description = "Get comprehensive context for a project: recent notes, open tasks, lifecycle breakdown, and status overview."
    )]
    async fn project_context(
        &self,
        Parameters(params): Parameters<ProjectContextParams>,
    ) -> Result<CallToolResult, McpError> {
        let project = sb_core::db::projects::get_project_by_name(self.db.pool(), &params.project)
            .await
            .map_err(|e| McpError::internal_error(format!("project lookup failed: {e}"), None))?;

        let project = match project {
            Some(p) => p,
            None => {
                return Ok(CallToolResult::success(vec![Content::text(format!(
                    "Project not found: {}",
                    params.project
                ))]));
            }
        };

        let limit = params.limit.unwrap_or(20);
        let notes = sb_core::db::projects::get_notes_for_project(self.db.pool(), project.id, limit)
            .await
            .map_err(|e| McpError::internal_error(format!("notes query failed: {e}"), None))?;

        let open_tasks = sb_core::db::tasks::get_open_tasks_for_project(self.db.pool(), project.id)
            .await
            .map_err(|e| McpError::internal_error(format!("tasks query failed: {e}"), None))?;

        // Count by lifecycle
        let mut lifecycle_counts: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();
        for note in &notes {
            *lifecycle_counts.entry(&note.lifecycle).or_insert(0) += 1;
        }

        let result = serde_json::json!({
            "project": {
                "name": project.name,
                "root_path": project.root_path,
                "description": project.description,
            },
            "stats": {
                "total_notes": notes.len(),
                "open_tasks": open_tasks.len(),
                "lifecycle_breakdown": lifecycle_counts,
            },
            "recent_notes": notes.iter().take(10).map(|n| serde_json::json!({
                "title": n.title,
                "file_path": n.file_path,
                "lifecycle": n.lifecycle,
                "updated_at": n.updated_at.to_rfc3339(),
            })).collect::<Vec<_>>(),
            "open_tasks": open_tasks.iter().map(|t| serde_json::json!({
                "title": t.title,
                "status": t.status,
                "created_at": t.created_at.to_rfc3339(),
            })).collect::<Vec<_>>(),
        });

        let output = serde_json::to_string_pretty(&result)
            .map_err(|e| McpError::internal_error(format!("serialization failed: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    #[tool(
        description = "Search note files directly on the filesystem using ripgrep (content) or fd (filenames). Works even for files not yet ingested into the database. Use this as a fallback when DB search returns nothing, or when you know notes exist on disk but haven't been indexed. Requires no database or embedding server."
    )]
    async fn file_search(
        &self,
        Parameters(params): Parameters<FileSearchParams>,
    ) -> Result<CallToolResult, McpError> {
        let limit = params.limit.unwrap_or(20) as usize;
        let mode = params.mode.as_deref().unwrap_or("content");

        // Use provided paths, or fall back to active mapper root
        let search_dirs: Vec<std::path::PathBuf> = if let Some(paths) = &params.paths {
            paths.iter().map(std::path::PathBuf::from).collect()
        } else {
            vec![self.active_mapper().root().to_path_buf()]
        };

        if search_dirs.is_empty() {
            return Ok(CallToolResult::error(vec![Content::text(
                "No search directories configured. Set WATCH_PATHS or notes.paths in config.",
            )]));
        }

        let results = match mode {
            "filename" => sb_core::file_search::search_filename(&search_dirs, &params.query, limit)
                .map_err(|e| {
                    McpError::internal_error(format!("filename search failed: {e}"), None)
                })?,
            _ => sb_core::file_search::search_content(&search_dirs, &params.query, limit).map_err(
                |e| McpError::internal_error(format!("content search failed: {e}"), None),
            )?,
        };

        if results.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "No files found matching '{}' (mode: {mode})",
                params.query
            ))]));
        }

        let items: Vec<_> = results
            .iter()
            .map(|r| {
                let mut item = serde_json::json!({
                    "file_path": r.file_path.to_string_lossy(),
                    "matched_text": r.matched_text,
                });
                if let Some(line) = r.line_number {
                    item["line_number"] = serde_json::json!(line);
                }
                item
            })
            .collect();

        let output = serde_json::to_string_pretty(&items)
            .map_err(|e| McpError::internal_error(format!("serialization failed: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Found {} matches (mode: {mode}):\n\n{}",
            results.len(),
            output
        ))]))
    }

    #[tool(
        description = "Manually set a note's lifecycle classification: active (default), volatile (brain dumps, TODOs), enduring (reference docs), or archived (moves file to archive/ subdirectory)."
    )]
    async fn note_classify(
        &self,
        Parameters(params): Parameters<NoteClassifyParams>,
    ) -> Result<CallToolResult, McpError> {
        let _session = self.require_session()?;
        let lifecycle =
            sb_core::lifecycle::Lifecycle::parse(&params.lifecycle).ok_or_else(|| {
                McpError::invalid_params(
                    format!(
                        "invalid lifecycle '{}': must be active, volatile, enduring, or archived",
                        params.lifecycle
                    ),
                    None,
                )
            })?;

        let note = sb_core::db::notes::get_note_by_path(self.db.pool(), &params.file_path)
            .await
            .map_err(|e| McpError::internal_error(format!("lookup failed: {e}"), None))?;

        let note = match note {
            Some(n) => n,
            None => {
                return Ok(CallToolResult::success(vec![Content::text(format!(
                    "Note not found: {}",
                    params.file_path
                ))]));
            }
        };

        // Special handling for archiving: move file to archive/ subdirectory
        if lifecycle == sb_core::lifecycle::Lifecycle::Archived {
            let src_path = Path::new(&note.file_path);
            if src_path.exists() {
                let archive_dir = src_path.parent().unwrap_or(Path::new(".")).join("archive");
                let filename = src_path.file_name().unwrap_or_default();
                let dest_path = archive_dir.join(filename);

                std::fs::create_dir_all(&archive_dir)
                    .map_err(|e| McpError::internal_error(format!("mkdir failed: {e}"), None))?;
                std::fs::rename(src_path, &dest_path)
                    .map_err(|e| McpError::internal_error(format!("move failed: {e}"), None))?;

                // Update file path in DB
                let new_path = dest_path.to_string_lossy().to_string();
                sb_core::db::notes::update_file_path(self.db.pool(), note.id, &new_path)
                    .await
                    .map_err(|e| {
                        McpError::internal_error(format!("path update failed: {e}"), None)
                    })?;

                sb_core::db::notes::update_lifecycle(self.db.pool(), note.id, lifecycle.as_str())
                    .await
                    .map_err(|e| {
                        McpError::internal_error(format!("lifecycle update failed: {e}"), None)
                    })?;

                return Ok(CallToolResult::success(vec![Content::text(format!(
                    "Archived '{}': moved to {} and marked as archived",
                    note.title, new_path
                ))]));
            }
        }

        // Normal lifecycle update
        sb_core::db::notes::update_lifecycle(self.db.pool(), note.id, lifecycle.as_str())
            .await
            .map_err(|e| McpError::internal_error(format!("lifecycle update failed: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Updated '{}' lifecycle: {} → {}",
            note.title, note.lifecycle, lifecycle
        ))]))
    }

    #[tool(
        description = "Stamp a note's frontmatter with edit metadata. Use this to record who last edited a note and when. Sets `edited_by` and `last_<editor>_edit` timestamp in YAML frontmatter. Previous editor timestamps are preserved, so you can see the full edit history."
    )]
    async fn note_stamp(
        &self,
        Parameters(params): Parameters<NoteStampParams>,
    ) -> Result<CallToolResult, McpError> {
        // Validate editor to prevent YAML frontmatter injection
        if params.editor.is_empty()
            || params.editor.len() > 50
            || !params
                .editor
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            return Ok(CallToolResult::error(vec![Content::text(
                "Invalid editor: must be 1-50 alphanumeric characters, hyphens, or underscores",
            )]));
        }

        let _session = self.require_session()?;
        let mapper = self.active_mapper();
        let path = mapper.to_absolute(&params.file_path);
        let path = path.as_path();

        if !path.exists() {
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "File not found: {}",
                params.file_path
            ))]));
        }

        let old_content = std::fs::read_to_string(path)
            .map_err(|e| McpError::internal_error(format!("read failed: {e}"), None))?;

        let stamped = sb_core::markdown::stamp_edit(&old_content, &params.editor);

        std::fs::write(path, &stamped)
            .map_err(|e| McpError::internal_error(format!("write failed: {e}"), None))?;

        // Re-ingest so the DB reflects the updated frontmatter
        if let Err(e) = ingest::ingest_file(&self.db, path, &self.active_mapper()).await {
            tracing::warn!("re-ingest after stamp failed: {e}");
        }

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Stamped '{}' with edited_by={}, last_{}_edit=<now>",
            params.file_path, params.editor, params.editor
        ))]))
    }

    #[tool(
        description = "Initialize your editing session with an isolated git worktree. Must be called before using write tools (note_create, note_update, note_stamp). Read-only tools (search, list, read) work without a session. Creates or checks out a branch named '<username>/working' by default."
    )]
    async fn session_init(
        &self,
        Parameters(params): Parameters<SessionInitParams>,
    ) -> Result<CallToolResult, McpError> {
        // Check if session already active
        {
            let session = self.session.lock().unwrap();
            if let Some(existing) = session.as_ref() {
                return Ok(CallToolResult::success(vec![Content::text(format!(
                    "Session already active:\n  User: {} <{}>\n  Branch: {}\n  Worktree: {}",
                    existing.username,
                    existing.email,
                    existing.branch,
                    existing.worktree_path.display()
                ))]));
            }
        }

        let config = self.worktree_config.as_ref().ok_or_else(|| {
            McpError::internal_error(
                "Worktree support not available (stdio mode or not configured).",
                None,
            )
        })?;

        let session_id = format!("{}-{}", params.username, uuid::Uuid::new_v4().as_simple());

        let info = sb_core::worktree::create_worktree(
            config,
            &session_id,
            &params.username,
            &params.email,
            params.branch.as_deref(),
        )
        .map_err(|e| McpError::internal_error(format!("worktree creation failed: {e}"), None))?;

        let msg = format!(
            "Session initialized:\n  User: {} <{}>\n  Branch: {}\n  Worktree: {}\n\nWrite tools (note_create, note_update, note_stamp) are now enabled.",
            info.username,
            info.email,
            info.branch,
            info.worktree_path.display()
        );

        *self.session.lock().unwrap() = Some(info);

        Ok(CallToolResult::success(vec![Content::text(msg)]))
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

/// Resolve a project name to its UUID, if provided.
async fn resolve_project_id(db: &Database, name: Option<&str>) -> Option<uuid::Uuid> {
    match name {
        Some(n) => sb_core::db::projects::get_project_by_name(db.pool(), n)
            .await
            .ok()
            .flatten()
            .map(|p| p.id),
        None => None,
    }
}

/// Default AI commit author name. Override via AI_GIT_NAME env var.
const DEFAULT_AI_NAME: &str = "claude-ai";
/// Default AI commit author email. Override via AI_GIT_EMAIL env var.
const DEFAULT_AI_EMAIL: &str = "ai@second-brain.local";

impl SecondBrainServer {
    /// Returns the PathMapper for the active context:
    /// session worktree if initialized, main repo otherwise.
    fn active_mapper(&self) -> PathMapper {
        let session = self.session.lock().unwrap();
        match session.as_ref() {
            Some(info) => PathMapper::new(info.worktree_path.clone()),
            None => self.main_mapper.clone(),
        }
    }

    /// Returns session info or an error for tools that require an active session.
    fn require_session(&self) -> Result<SessionInfo, McpError> {
        let session = self.session.lock().unwrap();
        session.clone().ok_or_else(|| {
            McpError::invalid_params(
                "No active session. Call session_init first with your username and email.",
                None,
            )
        })
    }

    /// Commit a single AI-authored file to the session's branch.
    fn git_commit_file(&self, file_path: &Path, message: &str) -> Option<(String, String)> {
        let session = self.session.lock().unwrap();

        let (notes_root, repo_owner) = match session.as_ref() {
            Some(info) => (info.worktree_path.clone(), info.username.clone()),
            None => {
                let root = self.notes_paths.first()?.clone();
                let owner = sb_skills::git_ops::git_username(&root).ok()?;
                (root, owner)
            }
        };

        if !sb_skills::git_ops::is_git_repo(&notes_root) {
            return None;
        }

        let ai_name = std::env::var("AI_GIT_NAME").unwrap_or_else(|_| DEFAULT_AI_NAME.to_string());
        let ai_email =
            std::env::var("AI_GIT_EMAIL").unwrap_or_else(|_| DEFAULT_AI_EMAIL.to_string());

        match sb_skills::git_ops::commit_file(
            &notes_root,
            file_path,
            message,
            &repo_owner,
            &ai_name,
            &ai_email,
        ) {
            Ok(result) => result,
            Err(e) => {
                tracing::warn!("git commit skipped: {e}");
                None
            }
        }
    }
}

impl Drop for SecondBrainServer {
    fn drop(&mut self) {
        // Only clean up if we're the last holder of the session Arc
        if Arc::strong_count(&self.session) == 1
            && let Some(config) = &self.worktree_config
        {
            let session = self.session.lock().unwrap();
            if let Some(info) = session.as_ref() {
                tracing::info!("cleaning up worktree for session {}", info.session_id);
                if let Err(e) = sb_core::worktree::remove_worktree(config, &info.session_id) {
                    tracing::error!("worktree cleanup failed: {e}");
                }
            }
        }
    }
}

/// Produce a simple unified diff between two strings.
fn unified_diff(old: &str, new: &str, path: &str) -> String {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    if old_lines == new_lines {
        return String::new();
    }

    let mut out = format!("--- a/{path}\n+++ b/{path}\n");

    // Simple line-by-line diff: show removed and added lines.
    // Walk both sides, reporting contiguous changed hunks.
    let max = old_lines.len().max(new_lines.len());
    let mut i = 0;
    while i < max {
        let old_line = old_lines.get(i).copied();
        let new_line = new_lines.get(i).copied();

        if old_line == new_line {
            i += 1;
            continue;
        }

        // Found a difference — emit a hunk
        let hunk_start = i;
        // Scan forward to find the end of the changed region
        while i < max {
            let ol = old_lines.get(i).copied();
            let nl = new_lines.get(i).copied();
            if ol == nl {
                break;
            }
            i += 1;
        }

        out.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            hunk_start + 1,
            i - hunk_start,
            hunk_start + 1,
            i - hunk_start,
        ));

        for j in hunk_start..i {
            if let Some(ol) = old_lines.get(j)
                && new_lines.get(j) != Some(ol)
            {
                out.push_str(&format!("-{ol}\n"));
            }
            if let Some(nl) = new_lines.get(j)
                && old_lines.get(j) != Some(nl)
            {
                out.push_str(&format!("+{nl}\n"));
            }
        }
    }

    out
}

// ── ServerHandler impl ─────────────────────────────────────────

#[tool_handler]
impl ServerHandler for SecondBrainServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::default(),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "second-brain".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                title: None,
                description: Some(
                    "Personal knowledge base MCP server with semantic search and skill engine"
                        .to_string(),
                ),
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "Second Brain MCP Server — search, read, create, update, and ingest your \
                 personal markdown notes. Supports full-text search (note_search), semantic \
                 vector search (semantic_search), and link graph traversal (note_graph). \
                 Notes are stored in PostgreSQL with pgvector embeddings. Use note_create \
                 and note_update to write notes that are automatically indexed and embedded. \
                 Phase 4 adds: run_skill (composable workflows: summarize, reflect, \
                 continue-work, connect-ideas, contextualize), project_list, project_context, \
                 and note_classify for lifecycle management. \
                 file_search provides a DB-free fallback using ripgrep/fd for files not yet ingested."
                    .to_string(),
            ),
        }
    }
}
