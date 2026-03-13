use crate::chunker::{Chunker, ChunkerConfig};
use crate::provider::EmbeddingProvider;
use sb_core::db::{chunks, embeddings};
use sb_core::models::Note;
use sqlx::PgPool;
use std::sync::Arc;

/// Orchestrates chunking and embedding for notes.
pub struct EmbeddingPipeline {
    provider: Arc<dyn EmbeddingProvider>,
    chunker: Chunker,
    batch_size: usize,
}

/// Stats returned after processing.
#[derive(Debug, Default)]
pub struct PipelineStats {
    pub notes_processed: u64,
    pub chunks_created: u64,
    pub embeddings_created: u64,
    pub errors: Vec<String>,
}

impl EmbeddingPipeline {
    pub fn new(provider: Arc<dyn EmbeddingProvider>, batch_size: usize, chunker_config: ChunkerConfig) -> Self {
        Self {
            provider,
            chunker: Chunker::new(chunker_config),
            batch_size,
        }
    }

    /// Process a single note: chunk it and embed all chunks.
    pub async fn process_note(&self, pool: &PgPool, note: &Note) -> anyhow::Result<(u64, u64)> {
        // Delete existing chunks and embeddings for this note (re-process from scratch)
        chunks::delete_chunks_for_note(pool, note.id).await?;

        // Chunk the note
        let new_chunks = self.chunker.chunk(note.id, &note.raw_content);
        if new_chunks.is_empty() {
            tracing::debug!("no chunks produced for note: {}", note.file_path);
            return Ok((0, 0));
        }

        // Insert chunks into DB
        let mut chunk_ids = Vec::new();
        let mut chunk_texts = Vec::new();
        for chunk_data in &new_chunks {
            let chunk = chunks::upsert_chunk(pool, chunk_data).await?;
            chunk_ids.push(chunk.id);
            chunk_texts.push(chunk.content);
        }

        let chunks_created = chunk_ids.len() as u64;

        // Embed in batches
        let mut embeddings_created = 0u64;
        for batch_start in (0..chunk_texts.len()).step_by(self.batch_size) {
            let batch_end = (batch_start + self.batch_size).min(chunk_texts.len());
            let text_refs: Vec<&str> = chunk_texts[batch_start..batch_end]
                .iter()
                .map(|s| s.as_str())
                .collect();

            let vectors = self.provider.embed_batch(&text_refs).await?;

            for (i, vector) in vectors.into_iter().enumerate() {
                let chunk_id = chunk_ids[batch_start + i];
                embeddings::insert_embedding(
                    pool,
                    chunk_id,
                    self.provider.provider_name(),
                    self.provider.model_name(),
                    &vector,
                )
                .await?;
                embeddings_created += 1;
            }
        }

        tracing::info!(
            "embedded note '{}': {} chunks, {} embeddings",
            note.file_path,
            chunks_created,
            embeddings_created
        );

        Ok((chunks_created, embeddings_created))
    }

    /// Process all notes that have no embeddings for the current provider/model.
    pub async fn process_unembedded(
        &self,
        pool: &PgPool,
    ) -> anyhow::Result<PipelineStats> {
        let mut stats = PipelineStats::default();

        // Find notes that have chunks without embeddings for our provider/model
        let notes_to_process = get_notes_needing_embedding(
            pool,
            self.provider.provider_name(),
            self.provider.model_name(),
        )
        .await?;

        for note in &notes_to_process {
            match self.process_note(pool, note).await {
                Ok((c, e)) => {
                    stats.notes_processed += 1;
                    stats.chunks_created += c;
                    stats.embeddings_created += e;
                }
                Err(err) => {
                    stats.errors.push(format!("{}: {err}", note.file_path));
                    tracing::error!("failed to process note '{}': {err}", note.file_path);
                }
            }
        }

        Ok(stats)
    }

    /// Re-process ALL notes (re-chunk and re-embed everything).
    pub async fn process_all(&self, pool: &PgPool) -> anyhow::Result<PipelineStats> {
        let mut stats = PipelineStats::default();
        let notes = sb_core::db::notes::list_notes(pool, 100_000, 0).await?;
        for note in &notes {
            match self.process_note(pool, note).await {
                Ok((c, e)) => {
                    stats.notes_processed += 1;
                    stats.chunks_created += c;
                    stats.embeddings_created += e;
                }
                Err(err) => {
                    stats.errors.push(format!("{}: {err}", note.file_path));
                    tracing::error!("failed to process note '{}': {err}", note.file_path);
                }
            }
        }
        Ok(stats)
    }

    /// Embed a single query string (for semantic search).
    pub async fn embed_query(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        self.provider.embed(text).await
    }

    /// Unload the embedding model from memory (e.g. free Ollama VRAM/RAM).
    pub async fn unload_model(&self) -> anyhow::Result<()> {
        self.provider.unload_model().await
    }
}

/// Find notes that have been ingested but don't have complete embeddings.
async fn get_notes_needing_embedding(
    pool: &PgPool,
    provider: &str,
    model: &str,
) -> anyhow::Result<Vec<Note>> {
    let sql = format!(
        "SELECT DISTINCT {} FROM notes n
         LEFT JOIN chunks c ON c.note_id = n.id
         LEFT JOIN embeddings e ON e.chunk_id = c.id
             AND e.provider = $1 AND e.model = $2
         WHERE n.deleted = false
           AND (c.id IS NULL OR e.id IS NULL)
         ORDER BY n.updated_at DESC",
        sb_core::db::notes::NOTE_COLS_N,
    );
    let rows = sqlx::query_as::<_, Note>(&sql)
        .bind(provider)
        .bind(model)
        .fetch_all(pool)
        .await?;

    Ok(rows)
}
