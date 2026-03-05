use crate::models::Link;
use sqlx::PgPool;
use uuid::Uuid;

/// Replace all links for a source note (delete old, insert new).
pub async fn replace_links_for_note(
    pool: &PgPool,
    source_note_id: Uuid,
    links: &[(String, String, Option<String>)], // (link_text, target_path, context)
) -> anyhow::Result<u64> {
    // Delete existing links from this note
    sqlx::query("DELETE FROM links WHERE source_note_id = $1")
        .bind(source_note_id)
        .execute(pool)
        .await?;

    let mut inserted = 0u64;
    for (link_text, target_path, context) in links {
        // Try to resolve target_path to a note ID.
        // First try exact match, then try matching by file name suffix
        // (handles wikilinks like "rust-ownership.md" matching "./test-notes/rust-ownership.md")
        let target_note_id: Option<Uuid> = sqlx::query_scalar(
            r#"
            SELECT id FROM notes
            WHERE deleted = false
              AND (file_path = $1 OR file_path LIKE '%/' || $1)
            LIMIT 1
            "#,
        )
        .bind(target_path)
        .fetch_optional(pool)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO links (id, source_note_id, target_note_id, link_text, target_path, context)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(source_note_id)
        .bind(target_note_id)
        .bind(link_text)
        .bind(target_path)
        .bind(context.as_deref())
        .execute(pool)
        .await?;
        inserted += 1;
    }

    Ok(inserted)
}

/// Get all outbound links from a note.
pub async fn get_links_from_note(pool: &PgPool, note_id: Uuid) -> anyhow::Result<Vec<Link>> {
    let rows = sqlx::query_as::<_, Link>(
        "SELECT * FROM links WHERE source_note_id = $1 ORDER BY link_text",
    )
    .bind(note_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Get all inbound links to a note (backlinks).
pub async fn get_links_to_note(pool: &PgPool, note_id: Uuid) -> anyhow::Result<Vec<Link>> {
    let rows = sqlx::query_as::<_, Link>(
        "SELECT * FROM links WHERE target_note_id = $1 ORDER BY link_text",
    )
    .bind(note_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Link graph result with resolved note metadata.
#[derive(Debug)]
pub struct LinkGraphEntry {
    pub direction: &'static str, // "outbound" or "inbound"
    pub link_text: String,
    pub target_path: String,
    pub linked_note_title: Option<String>,
    pub linked_note_path: Option<String>,
}

/// Get the full link graph for a note (both inbound and outbound).
pub async fn get_link_graph(pool: &PgPool, note_id: Uuid) -> anyhow::Result<Vec<LinkGraphEntry>> {
    let mut graph = Vec::new();

    // Outbound links
    let outbound = sqlx::query_as::<_, (String, String, Option<String>, Option<String>)>(
        r#"
        SELECT l.link_text, l.target_path, n.title, n.file_path
        FROM links l
        LEFT JOIN notes n ON n.id = l.target_note_id AND n.deleted = false
        WHERE l.source_note_id = $1
        ORDER BY l.link_text
        "#,
    )
    .bind(note_id)
    .fetch_all(pool)
    .await?;

    for (link_text, target_path, title, file_path) in outbound {
        graph.push(LinkGraphEntry {
            direction: "outbound",
            link_text,
            target_path,
            linked_note_title: title,
            linked_note_path: file_path,
        });
    }

    // Inbound links (backlinks)
    let inbound = sqlx::query_as::<_, (String, String, String)>(
        r#"
        SELECT l.link_text, n.title, n.file_path
        FROM links l
        JOIN notes n ON n.id = l.source_note_id AND n.deleted = false
        WHERE l.target_note_id = $1
        ORDER BY n.title
        "#,
    )
    .bind(note_id)
    .fetch_all(pool)
    .await?;

    for (link_text, title, file_path) in inbound {
        graph.push(LinkGraphEntry {
            direction: "inbound",
            link_text,
            target_path: file_path.clone(),
            linked_note_title: Some(title),
            linked_note_path: Some(file_path),
        });
    }

    Ok(graph)
}

/// Re-resolve all unresolved links that point to a given target_path.
/// Called after a new note is ingested — previously dangling links may now resolve.
/// Matches both exact paths and filename suffixes.
pub async fn resolve_links_to_path(
    pool: &PgPool,
    file_path: &str,
    note_id: Uuid,
) -> anyhow::Result<u64> {
    // Extract just the filename for suffix matching
    let filename = std::path::Path::new(file_path)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();

    let result = sqlx::query(
        r#"
        UPDATE links SET target_note_id = $1
        WHERE target_note_id IS NULL
          AND (target_path = $2 OR target_path = $3)
        "#,
    )
    .bind(note_id)
    .bind(file_path)
    .bind(&filename)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
