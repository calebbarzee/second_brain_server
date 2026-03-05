use crate::models::Tag;
use sqlx::PgPool;
use uuid::Uuid;

/// Create a tag, returning the existing one if it already exists.
pub async fn upsert_tag(pool: &PgPool, name: &str) -> anyhow::Result<Tag> {
    let row = sqlx::query_as::<_, Tag>(
        r#"
        INSERT INTO tags (id, name) VALUES ($1, $2)
        ON CONFLICT (name) DO UPDATE SET name = EXCLUDED.name
        RETURNING *
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(name)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

/// Associate a tag with a note.
pub async fn tag_note(pool: &PgPool, note_id: Uuid, tag_id: Uuid) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO note_tags (note_id, tag_id)
        VALUES ($1, $2)
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(note_id)
    .bind(tag_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Remove a tag from a note.
pub async fn untag_note(pool: &PgPool, note_id: Uuid, tag_id: Uuid) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM note_tags WHERE note_id = $1 AND tag_id = $2")
        .bind(note_id)
        .bind(tag_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Get all tags for a note.
pub async fn get_tags_for_note(pool: &PgPool, note_id: Uuid) -> anyhow::Result<Vec<Tag>> {
    let rows = sqlx::query_as::<_, Tag>(
        r#"
        SELECT t.* FROM tags t
        JOIN note_tags nt ON nt.tag_id = t.id
        WHERE nt.note_id = $1
        ORDER BY t.name
        "#,
    )
    .bind(note_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// List all tags with note counts.
#[derive(Debug, sqlx::FromRow, serde::Serialize)]
pub struct TagWithCount {
    pub id: Uuid,
    pub name: String,
    pub note_count: i64,
}

pub async fn list_tags_with_counts(pool: &PgPool) -> anyhow::Result<Vec<TagWithCount>> {
    let rows = sqlx::query_as::<_, TagWithCount>(
        r#"
        SELECT t.id, t.name, COUNT(nt.note_id) as note_count
        FROM tags t
        LEFT JOIN note_tags nt ON nt.tag_id = t.id
        GROUP BY t.id, t.name
        ORDER BY note_count DESC, t.name
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Get notes that have no tags.
pub async fn get_untagged_note_ids(pool: &PgPool, limit: i64) -> anyhow::Result<Vec<Uuid>> {
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        r#"
        SELECT n.id FROM notes n
        LEFT JOIN note_tags nt ON nt.note_id = n.id
        WHERE nt.tag_id IS NULL AND n.deleted = false
        ORDER BY n.updated_at DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}
