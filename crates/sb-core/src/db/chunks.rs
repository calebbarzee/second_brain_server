use crate::models::{Chunk, CreateChunk};
use sqlx::PgPool;
use uuid::Uuid;

/// Insert a chunk (or replace if same note_id + chunk_index).
pub async fn upsert_chunk(pool: &PgPool, chunk: &CreateChunk) -> anyhow::Result<Chunk> {
    let row = sqlx::query_as::<_, Chunk>(
        r#"
        INSERT INTO chunks (id, note_id, chunk_index, content, heading_context, token_count)
        VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (note_id, chunk_index) DO UPDATE SET
            content = EXCLUDED.content,
            heading_context = EXCLUDED.heading_context,
            token_count = EXCLUDED.token_count
        RETURNING *
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(chunk.note_id)
    .bind(chunk.chunk_index)
    .bind(&chunk.content)
    .bind(&chunk.heading_context)
    .bind(chunk.token_count)
    .fetch_one(pool)
    .await?;

    Ok(row)
}

/// Delete all chunks for a note (used before re-chunking).
pub async fn delete_chunks_for_note(pool: &PgPool, note_id: Uuid) -> anyhow::Result<u64> {
    let result = sqlx::query("DELETE FROM chunks WHERE note_id = $1")
        .bind(note_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

/// Get all chunks for a note, ordered by chunk_index.
pub async fn get_chunks_for_note(pool: &PgPool, note_id: Uuid) -> anyhow::Result<Vec<Chunk>> {
    let rows =
        sqlx::query_as::<_, Chunk>("SELECT * FROM chunks WHERE note_id = $1 ORDER BY chunk_index")
            .bind(note_id)
            .fetch_all(pool)
            .await?;
    Ok(rows)
}

/// Get chunks that don't have embeddings yet for a given provider/model.
pub async fn get_chunks_without_embeddings(
    pool: &PgPool,
    provider: &str,
    model: &str,
    limit: i64,
) -> anyhow::Result<Vec<Chunk>> {
    let rows = sqlx::query_as::<_, Chunk>(
        r#"
        SELECT c.* FROM chunks c
        LEFT JOIN embeddings e ON e.chunk_id = c.id
            AND e.provider = $1 AND e.model = $2
        WHERE e.id IS NULL
        ORDER BY c.note_id, c.chunk_index
        LIMIT $3
        "#,
    )
    .bind(provider)
    .bind(model)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
