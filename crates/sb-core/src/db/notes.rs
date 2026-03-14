use crate::models::{CreateNote, Note};
use sqlx::PgPool;
use uuid::Uuid;

/// Explicit column list for Note queries — excludes `search_vector` (tsvector)
/// which sqlx can't decode and would misalign positional mapping with `SELECT *`.
pub const NOTE_COLS: &str = "id, file_path, title, content_hash, raw_content, frontmatter, \
    created_at, updated_at, synced_at, deleted, lifecycle, \
    source_project, source_path, source_branch, source_commit";

/// Prefixed version for JOINs (n.id, n.file_path, ...)
pub const NOTE_COLS_N: &str = "n.id, n.file_path, n.title, n.content_hash, n.raw_content, n.frontmatter, \
    n.created_at, n.updated_at, n.synced_at, n.deleted, n.lifecycle, \
    n.source_project, n.source_path, n.source_branch, n.source_commit";

/// Insert a new note or update if the file_path already exists.
pub async fn upsert_note(pool: &PgPool, note: &CreateNote) -> anyhow::Result<Note> {
    let sql = format!(
        "INSERT INTO notes (id, file_path, title, content_hash, raw_content, frontmatter)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (file_path) DO UPDATE SET
             title = EXCLUDED.title,
             content_hash = EXCLUDED.content_hash,
             raw_content = EXCLUDED.raw_content,
             frontmatter = EXCLUDED.frontmatter,
             deleted = false,
             updated_at = NOW(),
             synced_at = NOW()
         RETURNING {NOTE_COLS}"
    );
    let row = sqlx::query_as::<_, Note>(&sql)
        .bind(Uuid::new_v4())
        .bind(&note.file_path)
        .bind(&note.title)
        .bind(&note.content_hash)
        .bind(&note.raw_content)
        .bind(&note.frontmatter)
        .fetch_one(pool)
        .await?;

    Ok(row)
}

/// Get a note by its file path.
pub async fn get_note_by_path(pool: &PgPool, file_path: &str) -> anyhow::Result<Option<Note>> {
    let sql = format!("SELECT {NOTE_COLS} FROM notes WHERE file_path = $1 AND deleted = false");
    let row = sqlx::query_as::<_, Note>(&sql)
        .bind(file_path)
        .fetch_optional(pool)
        .await?;

    Ok(row)
}

/// Get a note by ID.
pub async fn get_note_by_id(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Note>> {
    let sql = format!("SELECT {NOTE_COLS} FROM notes WHERE id = $1 AND deleted = false");
    let row = sqlx::query_as::<_, Note>(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await?;

    Ok(row)
}

/// List all non-deleted notes, ordered by updated_at descending.
pub async fn list_notes(pool: &PgPool, limit: i64, offset: i64) -> anyhow::Result<Vec<Note>> {
    let sql = format!(
        "SELECT {NOTE_COLS} FROM notes WHERE deleted = false ORDER BY updated_at DESC LIMIT $1 OFFSET $2"
    );
    let rows = sqlx::query_as::<_, Note>(&sql)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;

    Ok(rows)
}

/// Full-text search over note content.
pub async fn search_notes(pool: &PgPool, query: &str, limit: i64) -> anyhow::Result<Vec<Note>> {
    let sql = format!(
        "SELECT {NOTE_COLS} FROM notes
         WHERE deleted = false
           AND search_vector @@ plainto_tsquery('english', $1)
         ORDER BY ts_rank(search_vector, plainto_tsquery('english', $1)) DESC
         LIMIT $2"
    );
    let rows = sqlx::query_as::<_, Note>(&sql)
        .bind(query)
        .bind(limit)
        .fetch_all(pool)
        .await?;

    Ok(rows)
}

/// Soft-delete a note by file path.
pub async fn soft_delete_note(pool: &PgPool, file_path: &str) -> anyhow::Result<bool> {
    let result =
        sqlx::query("UPDATE notes SET deleted = true, updated_at = NOW() WHERE file_path = $1")
            .bind(file_path)
            .execute(pool)
            .await?;

    Ok(result.rows_affected() > 0)
}

/// Get notes updated within a time range, optionally filtered by project.
pub async fn get_notes_in_range(
    pool: &PgPool,
    start: chrono::DateTime<chrono::Utc>,
    end: chrono::DateTime<chrono::Utc>,
    project_id: Option<Uuid>,
) -> anyhow::Result<Vec<Note>> {
    let sql = format!(
        "SELECT {NOTE_COLS_N} FROM notes n
         LEFT JOIN note_projects np ON np.note_id = n.id
         WHERE n.deleted = false
           AND n.updated_at >= $1 AND n.updated_at < $2
           AND ($3::UUID IS NULL OR np.project_id = $3)
         ORDER BY n.updated_at DESC"
    );
    let rows = sqlx::query_as::<_, Note>(&sql)
        .bind(start)
        .bind(end)
        .bind(project_id)
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

/// Update a note's lifecycle classification.
pub async fn update_lifecycle(pool: &PgPool, note_id: Uuid, lifecycle: &str) -> anyhow::Result<()> {
    sqlx::query("UPDATE notes SET lifecycle = $2, updated_at = NOW() WHERE id = $1")
        .bind(note_id)
        .bind(lifecycle)
        .execute(pool)
        .await?;
    Ok(())
}

/// Get notes by lifecycle, optionally filtered by project.
pub async fn get_notes_by_lifecycle(
    pool: &PgPool,
    lifecycle: &str,
    project_id: Option<Uuid>,
    limit: i64,
) -> anyhow::Result<Vec<Note>> {
    let sql = format!(
        "SELECT {NOTE_COLS_N} FROM notes n
         LEFT JOIN note_projects np ON np.note_id = n.id
         WHERE n.deleted = false
           AND n.lifecycle = $1
           AND ($2::UUID IS NULL OR np.project_id = $2)
         ORDER BY n.updated_at DESC
         LIMIT $3"
    );
    let rows = sqlx::query_as::<_, Note>(&sql)
        .bind(lifecycle)
        .bind(project_id)
        .bind(limit)
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

/// Update provenance fields for mirrored notes.
pub async fn update_provenance(
    pool: &PgPool,
    note_id: Uuid,
    source_project: &str,
    source_path: &str,
    source_branch: &str,
    source_commit: Option<&str>,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        UPDATE notes SET
            source_project = $2,
            source_path = $3,
            source_branch = $4,
            source_commit = $5
        WHERE id = $1
        "#,
    )
    .bind(note_id)
    .bind(source_project)
    .bind(source_path)
    .bind(source_branch)
    .bind(source_commit)
    .execute(pool)
    .await?;
    Ok(())
}

/// Get mirrored notes for a project.
pub async fn get_mirrored_notes(pool: &PgPool, source_project: &str) -> anyhow::Result<Vec<Note>> {
    let sql = format!(
        "SELECT {NOTE_COLS} FROM notes WHERE source_project = $1 AND deleted = false ORDER BY file_path"
    );
    let rows = sqlx::query_as::<_, Note>(&sql)
        .bind(source_project)
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

/// Update the file_path of a note (used when archiving/moving).
pub async fn update_file_path(pool: &PgPool, note_id: Uuid, new_path: &str) -> anyhow::Result<()> {
    sqlx::query("UPDATE notes SET file_path = $2, updated_at = NOW() WHERE id = $1")
        .bind(note_id)
        .bind(new_path)
        .execute(pool)
        .await?;
    Ok(())
}

/// List notes with optional lifecycle and project filters.
pub async fn list_notes_filtered(
    pool: &PgPool,
    lifecycle: Option<&str>,
    project_id: Option<Uuid>,
    limit: i64,
    offset: i64,
) -> anyhow::Result<Vec<Note>> {
    let sql = format!(
        "SELECT {NOTE_COLS_N} FROM notes n
         LEFT JOIN note_projects np ON np.note_id = n.id
         WHERE n.deleted = false
           AND ($1::TEXT IS NULL OR n.lifecycle = $1)
           AND ($2::UUID IS NULL OR np.project_id = $2)
         GROUP BY {NOTE_COLS_N}
         ORDER BY n.updated_at DESC
         LIMIT $3 OFFSET $4"
    );
    let rows = sqlx::query_as::<_, Note>(&sql)
        .bind(lifecycle)
        .bind(project_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

/// Full-text search with optional lifecycle and project filters.
pub async fn search_notes_filtered(
    pool: &PgPool,
    query: &str,
    lifecycle: Option<&str>,
    project_id: Option<Uuid>,
    limit: i64,
) -> anyhow::Result<Vec<Note>> {
    let sql = format!(
        "SELECT {NOTE_COLS_N} FROM notes n
         LEFT JOIN note_projects np ON np.note_id = n.id
         WHERE n.deleted = false
           AND n.search_vector @@ plainto_tsquery('english', $1)
           AND ($2::TEXT IS NULL OR n.lifecycle = $2)
           AND ($3::UUID IS NULL OR np.project_id = $3)
         GROUP BY {NOTE_COLS_N}
         ORDER BY ts_rank(n.search_vector, plainto_tsquery('english', $1)) DESC
         LIMIT $4"
    );
    let rows = sqlx::query_as::<_, Note>(&sql)
        .bind(query)
        .bind(lifecycle)
        .bind(project_id)
        .bind(limit)
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

/// Check if a note's content has changed by comparing hashes.
pub async fn note_content_changed(
    pool: &PgPool,
    file_path: &str,
    new_hash: &str,
) -> anyhow::Result<bool> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT content_hash FROM notes WHERE file_path = $1 AND deleted = false")
            .bind(file_path)
            .fetch_optional(pool)
            .await?;

    match row {
        Some((existing_hash,)) => Ok(existing_hash != new_hash),
        None => Ok(true), // Note doesn't exist, so "changed"
    }
}
