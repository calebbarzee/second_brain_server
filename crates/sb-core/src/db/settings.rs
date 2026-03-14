use sqlx::PgPool;

/// Get a setting value by key.
pub async fn get_setting(pool: &PgPool, key: &str) -> anyhow::Result<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = $1")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|(v,)| v))
}

/// Set a setting value (upsert).
pub async fn set_setting(pool: &PgPool, key: &str, value: &str) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES ($1, $2) \
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

/// One-time migration: convert absolute file_path values to canonical (repo-relative).
///
/// Strips the `notes_root` prefix from all `notes.file_path` and `links.target_path`.
/// Records completion in the `settings` table so it only runs once.
pub async fn migrate_to_canonical_paths(pool: &PgPool, notes_root: &str) -> anyhow::Result<()> {
    // Already done?
    if get_setting(pool, "paths_canonical").await?.is_some() {
        return Ok(());
    }

    let prefix = if notes_root.ends_with('/') {
        notes_root.to_string()
    } else {
        format!("{notes_root}/")
    };

    let prefix_len = prefix.len() as i32 + 1; // SQL SUBSTR is 1-based
    let like_pattern = format!("{prefix}%");

    // Strip prefix from notes.file_path
    let updated_notes =
        sqlx::query("UPDATE notes SET file_path = SUBSTR(file_path, $1) WHERE file_path LIKE $2")
            .bind(prefix_len)
            .bind(&like_pattern)
            .execute(pool)
            .await?;

    // Strip prefix from links.target_path (only absolute ones)
    let updated_links = sqlx::query(
        "UPDATE links SET target_path = SUBSTR(target_path, $1) WHERE target_path LIKE $2",
    )
    .bind(prefix_len)
    .bind(&like_pattern)
    .execute(pool)
    .await?;

    tracing::info!(
        "migrated to canonical paths: {} notes, {} link targets",
        updated_notes.rows_affected(),
        updated_links.rows_affected()
    );

    set_setting(pool, "paths_canonical", "true").await?;
    Ok(())
}
