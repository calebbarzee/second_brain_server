use crate::db::{links, notes, sync_state};
use crate::models::CreateNote;
use crate::{markdown, Database};
use std::path::Path;

/// Result of ingesting a single file.
#[derive(Debug)]
pub enum IngestResult {
    /// File was ingested (new or changed).
    Ingested(IngestInfo),
    /// File was skipped (content unchanged).
    Skipped,
}

#[derive(Debug)]
pub struct IngestInfo {
    pub note_id: uuid::Uuid,
    pub file_path: String,
    pub title: String,
    pub links_stored: u64,
}

/// Ingest a single markdown file: parse, upsert to DB, extract links, update sync state.
/// Returns `Ingested` if the file was new or changed, `Skipped` if unchanged.
pub async fn ingest_file(db: &Database, path: &Path) -> anyhow::Result<IngestResult> {
    let raw = std::fs::read_to_string(path)?;
    let file_path = path.to_string_lossy().to_string();
    let hash = markdown::content_hash(&raw);

    // Skip if content hasn't changed
    if !notes::note_content_changed(db.pool(), &file_path, &hash).await? {
        return Ok(IngestResult::Skipped);
    }

    let parsed = markdown::parse_markdown(&raw);

    let note = CreateNote {
        file_path: file_path.clone(),
        title: parsed.title.clone(),
        content_hash: hash.clone(),
        raw_content: raw,
        frontmatter: parsed.frontmatter,
    };

    let db_note = notes::upsert_note(db.pool(), &note).await?;

    // Extract and store links
    let link_data: Vec<(String, String, Option<String>)> = parsed
        .links
        .iter()
        .map(|l| {
            let resolved = resolve_link_path(&file_path, &l.target, l.is_wikilink);
            (l.link_text.clone(), resolved, None)
        })
        .collect();

    let links_stored = links::replace_links_for_note(db.pool(), db_note.id, &link_data).await?;

    // Resolve any dangling links that point to this note
    links::resolve_links_to_path(db.pool(), &file_path, db_note.id).await?;

    // Update sync state
    sync_state::upsert_sync_state(db.pool(), db_note.id, &hash, "file_to_db").await?;

    Ok(IngestResult::Ingested(IngestInfo {
        note_id: db_note.id,
        file_path,
        title: parsed.title,
        links_stored,
    }))
}

/// Ingest all markdown files in a directory recursively.
pub async fn ingest_directory(db: &Database, dir: &Path) -> anyhow::Result<IngestStats> {
    let mut stats = IngestStats::default();

    let files: Vec<_> = walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
        .map(|e| e.path().to_path_buf())
        .collect();

    for file_path in &files {
        match ingest_file(db, file_path).await {
            Ok(IngestResult::Ingested(info)) => {
                stats.ingested += 1;
                stats.links_stored += info.links_stored;
                stats.ingested_note_ids.push(info.note_id);
            }
            Ok(IngestResult::Skipped) => {
                stats.skipped += 1;
            }
            Err(e) => {
                stats.errors.push(format!("{}: {e}", file_path.display()));
            }
        }
    }

    Ok(stats)
}

#[derive(Debug, Default)]
pub struct IngestStats {
    pub ingested: u64,
    pub skipped: u64,
    pub links_stored: u64,
    pub errors: Vec<String>,
    pub ingested_note_ids: Vec<uuid::Uuid>,
}

/// Resolve a link target to an absolute-ish file path.
/// For wikilinks: search by note name (append .md if needed).
/// For relative paths: resolve relative to the source note's directory.
fn resolve_link_path(source_file_path: &str, target: &str, is_wikilink: bool) -> String {
    // Skip external URLs
    if target.starts_with("http://") || target.starts_with("https://") || target.starts_with('#') {
        return target.to_string();
    }

    if is_wikilink {
        // Wikilinks: store as-is with .md extension for matching
        let target = target.trim();
        if target.ends_with(".md") {
            target.to_string()
        } else {
            format!("{target}.md")
        }
    } else {
        // Relative path: resolve relative to source note's directory
        let source_dir = Path::new(source_file_path)
            .parent()
            .unwrap_or(Path::new(""));
        let resolved = source_dir.join(target);
        // Normalize the path (remove ./ and ../)
        normalize_path(&resolved)
    }
}

/// Simple path normalization that removes `.` and `..` components.
fn normalize_path(path: &Path) -> String {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {} // skip `.`
            std::path::Component::ParentDir => {
                components.pop(); // go up one level
            }
            other => components.push(other),
        }
    }
    let result: std::path::PathBuf = components.iter().collect();
    result.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_wikilink() {
        let result = resolve_link_path("/notes/foo.md", "some note", true);
        assert_eq!(result, "some note.md");
    }

    #[test]
    fn test_resolve_wikilink_with_md() {
        let result = resolve_link_path("/notes/foo.md", "some note.md", true);
        assert_eq!(result, "some note.md");
    }

    #[test]
    fn test_resolve_relative_path() {
        let result = resolve_link_path("/home/user/notes/foo.md", "./bar.md", false);
        assert_eq!(result, "/home/user/notes/bar.md");
    }

    #[test]
    fn test_resolve_relative_parent() {
        let result = resolve_link_path("/home/user/notes/sub/foo.md", "../bar.md", false);
        assert_eq!(result, "/home/user/notes/bar.md");
    }

    #[test]
    fn test_resolve_external_url() {
        let result = resolve_link_path("/notes/foo.md", "https://example.com", false);
        assert_eq!(result, "https://example.com");
    }

    #[test]
    fn test_resolve_anchor() {
        let result = resolve_link_path("/notes/foo.md", "#section", false);
        assert_eq!(result, "#section");
    }
}
