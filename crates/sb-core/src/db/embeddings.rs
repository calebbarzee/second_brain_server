use pgvector::Vector;
use sqlx::PgPool;
use uuid::Uuid;

/// Store an embedding vector for a chunk.
pub async fn insert_embedding(
    pool: &PgPool,
    chunk_id: Uuid,
    provider: &str,
    model: &str,
    vector: &[f32],
) -> anyhow::Result<Uuid> {
    let vec = Vector::from(vector.to_vec());
    let row: (Uuid,) = sqlx::query_as(
        r#"
        INSERT INTO embeddings (id, chunk_id, provider, model, vector)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(chunk_id)
    .bind(provider)
    .bind(model)
    .bind(vec)
    .fetch_one(pool)
    .await?;

    Ok(row.0)
}

/// Delete embeddings for a chunk (used when re-embedding).
pub async fn delete_embeddings_for_chunk(pool: &PgPool, chunk_id: Uuid) -> anyhow::Result<u64> {
    let result = sqlx::query("DELETE FROM embeddings WHERE chunk_id = $1")
        .bind(chunk_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

/// Semantic search result with metadata.
#[derive(Debug)]
pub struct SemanticSearchResult {
    pub chunk_id: Uuid,
    pub note_id: Uuid,
    pub chunk_content: String,
    pub heading_context: Option<String>,
    pub note_title: String,
    pub note_file_path: String,
    pub similarity: f64,
}

/// Semantic search: find the top-k most similar chunks to a query vector.
pub async fn semantic_search(
    pool: &PgPool,
    query_vector: &[f32],
    limit: i64,
) -> anyhow::Result<Vec<SemanticSearchResult>> {
    let vec = Vector::from(query_vector.to_vec());

    let rows = sqlx::query_as::<_, (Uuid, Uuid, String, Option<String>, String, String, f64)>(
        r#"
        SELECT
            c.id AS chunk_id,
            c.note_id,
            c.content AS chunk_content,
            c.heading_context,
            n.title AS note_title,
            n.file_path AS note_file_path,
            1 - (e.vector <=> $1::vector) AS similarity
        FROM embeddings e
        JOIN chunks c ON c.id = e.chunk_id
        JOIN notes n ON n.id = c.note_id AND n.deleted = false
        ORDER BY e.vector <=> $1::vector
        LIMIT $2
        "#,
    )
    .bind(vec)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| SemanticSearchResult {
            chunk_id: r.0,
            note_id: r.1,
            chunk_content: r.2,
            heading_context: r.3,
            note_title: r.4,
            note_file_path: r.5,
            similarity: r.6,
        })
        .collect())
}

/// Find chunks similar to a specific note's chunks (for "find related" feature).
pub async fn find_related_notes(
    pool: &PgPool,
    note_id: Uuid,
    limit: i64,
) -> anyhow::Result<Vec<SemanticSearchResult>> {
    // Average the embeddings of all chunks in the given note, then search.
    // Filter out NULL avg_vec (when note has no embeddings) to avoid NULL similarity.
    let rows = sqlx::query_as::<_, (Uuid, Uuid, String, Option<String>, String, String, f64)>(
        r#"
        WITH note_embedding AS (
            SELECT avg(e.vector) AS avg_vec
            FROM embeddings e
            JOIN chunks c ON c.id = e.chunk_id
            WHERE c.note_id = $1
        )
        SELECT
            c.id AS chunk_id,
            c.note_id,
            c.content AS chunk_content,
            c.heading_context,
            n.title AS note_title,
            n.file_path AS note_file_path,
            1 - (e.vector <=> ne.avg_vec) AS similarity
        FROM note_embedding ne, embeddings e
        JOIN chunks c ON c.id = e.chunk_id
        JOIN notes n ON n.id = c.note_id AND n.deleted = false
        WHERE c.note_id != $1
          AND ne.avg_vec IS NOT NULL
        ORDER BY e.vector <=> ne.avg_vec
        LIMIT $2
        "#,
    )
    .bind(note_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| SemanticSearchResult {
            chunk_id: r.0,
            note_id: r.1,
            chunk_content: r.2,
            heading_context: r.3,
            note_title: r.4,
            note_file_path: r.5,
            similarity: r.6,
        })
        .collect())
}

/// Count total embeddings in the database.
pub async fn count_embeddings(pool: &PgPool) -> anyhow::Result<i64> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM embeddings")
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}
