use crate::models::SyncState;
use sqlx::PgPool;
use uuid::Uuid;

/// Upsert sync state for a note after syncing.
pub async fn upsert_sync_state(
    pool: &PgPool,
    note_id: Uuid,
    file_hash: &str,
    direction: &str,
) -> anyhow::Result<SyncState> {
    let row = sqlx::query_as::<_, SyncState>(
        r#"
        INSERT INTO sync_state (note_id, file_hash, last_synced, sync_direction)
        VALUES ($1, $2, NOW(), $3)
        ON CONFLICT (note_id) DO UPDATE SET
            file_hash = EXCLUDED.file_hash,
            last_synced = NOW(),
            sync_direction = EXCLUDED.sync_direction
        RETURNING *
        "#,
    )
    .bind(note_id)
    .bind(file_hash)
    .bind(direction)
    .fetch_one(pool)
    .await?;

    Ok(row)
}

/// Get the sync state for a specific note.
pub async fn get_sync_state(pool: &PgPool, note_id: Uuid) -> anyhow::Result<Option<SyncState>> {
    let row = sqlx::query_as::<_, SyncState>("SELECT * FROM sync_state WHERE note_id = $1")
        .bind(note_id)
        .fetch_optional(pool)
        .await?;

    Ok(row)
}

/// Get sync state by file hash (to check if a file needs re-syncing).
pub async fn get_sync_state_by_hash(
    pool: &PgPool,
    note_id: Uuid,
    file_hash: &str,
) -> anyhow::Result<bool> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT file_hash FROM sync_state WHERE note_id = $1")
            .bind(note_id)
            .fetch_optional(pool)
            .await?;

    match row {
        Some((existing_hash,)) => Ok(existing_hash == file_hash),
        None => Ok(false), // No sync state = needs sync
    }
}

/// Delete sync state for a note (e.g., when note is deleted).
pub async fn delete_sync_state(pool: &PgPool, note_id: Uuid) -> anyhow::Result<bool> {
    let result = sqlx::query("DELETE FROM sync_state WHERE note_id = $1")
        .bind(note_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}
