use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

// ── Notes ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Note {
    pub id: Uuid,
    pub file_path: String,
    pub title: String,
    pub content_hash: String,
    pub raw_content: String,
    pub frontmatter: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub synced_at: Option<DateTime<Utc>>,
    pub deleted: bool,
    // Phase 4: lifecycle + provenance
    pub lifecycle: String,
    pub source_project: Option<String>,
    pub source_path: Option<String>,
    pub source_branch: Option<String>,
    pub source_commit: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateNote {
    pub file_path: String,
    pub title: String,
    pub content_hash: String,
    pub raw_content: String,
    pub frontmatter: Option<serde_json::Value>,
}

// ── Chunks ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Chunk {
    pub id: Uuid,
    pub note_id: Uuid,
    pub chunk_index: i32,
    pub content: String,
    pub heading_context: Option<String>,
    pub token_count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateChunk {
    pub note_id: Uuid,
    pub chunk_index: i32,
    pub content: String,
    pub heading_context: Option<String>,
    pub token_count: i32,
}

// ── Tags ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Tag {
    pub id: Uuid,
    pub name: String,
}

// ── Links ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Link {
    pub id: Uuid,
    pub source_note_id: Uuid,
    pub target_note_id: Option<Uuid>,
    pub link_text: String,
    pub target_path: String,
    pub context: Option<String>,
}

// ── Projects ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub root_path: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
}

// ── Sync State ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SyncState {
    pub note_id: Uuid,
    pub file_hash: String,
    pub last_synced: DateTime<Utc>,
    pub sync_direction: String,
}

// ── Skill Runs ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SkillRun {
    pub id: Uuid,
    pub skill_name: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub status: String,
    pub input_params: Option<serde_json::Value>,
    pub output_summary: Option<String>,
}

// ── Tasks ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Task {
    pub id: Uuid,
    pub title: String,
    pub status: String,
    pub project_id: Option<Uuid>,
    pub due_date: Option<DateTime<Utc>>,
    pub created_by_skill: Option<String>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub source_note_id: Option<Uuid>,
}

// ── Parsed Markdown ────────────────────────────────────────────

/// Result of parsing a markdown file — not a DB model, but used
/// by the ingestion pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedNote {
    pub title: String,
    pub frontmatter: Option<serde_json::Value>,
    pub content: String,
    pub headings: Vec<Heading>,
    pub links: Vec<ParsedLink>,
    pub tasks: Vec<ParsedTask>,
}

/// A task extracted from markdown checkbox syntax.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedTask {
    pub title: String,
    pub completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Heading {
    pub level: u8,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedLink {
    pub link_text: String,
    pub target: String,
    pub is_wikilink: bool,
}
