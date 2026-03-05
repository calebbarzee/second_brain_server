use crate::watcher::FileChange;
use sb_core::db::notes;
use sb_core::ingest::{self, IngestResult};
use sb_core::Database;
use sb_embed::EmbeddingPipeline;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Statistics for sync operations.
#[derive(Debug, Default)]
pub struct SyncStats {
    pub files_ingested: u64,
    pub files_deleted: u64,
    pub files_skipped: u64,
    pub embeddings_created: u64,
    pub errors: Vec<String>,
}

/// Processes file change events from the watcher.
pub struct SyncProcessor {
    db: Database,
    pipeline: Arc<EmbeddingPipeline>,
}

impl SyncProcessor {
    pub fn new(db: Database, pipeline: Arc<EmbeddingPipeline>) -> Self {
        Self { db, pipeline }
    }

    /// Run the processor loop, consuming file changes from the channel.
    /// This runs until the sender is dropped (watcher stops).
    pub async fn run(&self, mut rx: mpsc::Receiver<FileChange>) {
        tracing::info!("sync processor started");

        while let Some(change) = rx.recv().await {
            match &change {
                FileChange::Modified(path) => {
                    tracing::info!("sync: file modified — {}", path.display());
                    match self.handle_modified(path).await {
                        Ok(()) => {}
                        Err(e) => {
                            tracing::error!("sync error for {}: {e}", path.display());
                        }
                    }
                }
                FileChange::Deleted(path) => {
                    tracing::info!("sync: file deleted — {}", path.display());
                    match self.handle_deleted(path).await {
                        Ok(()) => {}
                        Err(e) => {
                            tracing::error!("sync delete error for {}: {e}", path.display());
                        }
                    }
                }
            }
        }

        tracing::info!("sync processor stopped (channel closed)");
    }

    /// Handle a file creation or modification.
    async fn handle_modified(&self, path: &std::path::Path) -> anyhow::Result<()> {
        match ingest::ingest_file(&self.db, path).await? {
            IngestResult::Ingested(info) => {
                tracing::info!(
                    "ingested '{}' ({} links)",
                    info.title,
                    info.links_stored
                );

                // Embed the newly ingested note
                if let Some(note) =
                    notes::get_note_by_id(self.db.pool(), info.note_id).await?
                {
                    match self.pipeline.process_note(self.db.pool(), &note).await {
                        Ok((chunks, embeddings)) => {
                            tracing::info!(
                                "embedded '{}': {} chunks, {} embeddings",
                                info.title,
                                chunks,
                                embeddings
                            );
                        }
                        Err(e) => {
                            tracing::error!("embedding failed for '{}': {e}", info.title);
                        }
                    }
                }
            }
            IngestResult::Skipped => {
                tracing::debug!("skipped (unchanged): {}", path.display());
            }
        }
        Ok(())
    }

    /// Handle a file deletion (soft-delete the note).
    async fn handle_deleted(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let file_path = path.to_string_lossy().to_string();
        let deleted = notes::soft_delete_note(self.db.pool(), &file_path).await?;
        if deleted {
            tracing::info!("soft-deleted note: {}", file_path);
        } else {
            tracing::debug!("no note found to delete for: {}", file_path);
        }
        Ok(())
    }

    /// Process a single change synchronously (useful for testing or manual sync).
    pub async fn process_change(&self, change: &FileChange) -> anyhow::Result<()> {
        match change {
            FileChange::Modified(path) => self.handle_modified(path).await,
            FileChange::Deleted(path) => self.handle_deleted(path).await,
        }
    }
}
