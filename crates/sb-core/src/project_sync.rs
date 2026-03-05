//! Symlink-based sync for project directories.
//!
//! Reads files matching glob patterns from a project directory (on a specific
//! git branch) and creates symlinks in a mirror subdirectory in the KB. Symlinks
//! keep files in sync without duplication — edits to originals are immediately
//! visible. Tracks provenance (source project, path, branch, commit SHA).

use std::path::{Path, PathBuf};

/// Configuration for a single project to sync.
#[derive(Debug, Clone)]
pub struct ProjectSyncConfig {
    pub name: String,
    pub source_path: PathBuf,
    pub branch: String,
    pub patterns: Vec<String>,
    pub mirror_to: String,
}

/// Result of syncing a single file.
#[derive(Debug)]
pub struct SyncedFile {
    pub source_path: PathBuf,
    pub mirror_path: PathBuf,
    pub changed: bool,
}

/// Result of syncing a project directory.
#[derive(Debug, Default)]
pub struct ProjectSyncStats {
    pub files_linked: u64,
    pub files_skipped: u64,
    pub files_stale: u64,
    pub errors: Vec<String>,
    pub synced_mirror_paths: Vec<PathBuf>,
}

/// Get the current git HEAD commit SHA for a directory.
pub fn get_head_commit(repo_path: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_path)
        .output()
        .ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

/// Get the current branch name for a directory.
pub fn get_current_branch(repo_path: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(repo_path)
        .output()
        .ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

/// Sync a project directory into the KB mirror location.
///
/// - Reads files matching the configured glob patterns from the project dir.
/// - Creates symlinks in `kb_root/mirror_to/` pointing to the source files.
/// - Returns stats and the list of mirror paths that are current.
///
/// This function NEVER modifies the project directory (observer pattern).
pub fn sync_project(
    config: &ProjectSyncConfig,
    kb_root: &Path,
) -> ProjectSyncStats {
    let mut stats = ProjectSyncStats::default();
    let mirror_dir = kb_root.join(&config.mirror_to);

    // Ensure mirror directory exists
    if let Err(e) = std::fs::create_dir_all(&mirror_dir) {
        stats.errors.push(format!("failed to create mirror dir: {e}"));
        return stats;
    }

    // Collect files matching patterns from the source project
    let source_files = collect_matching_files(&config.source_path, &config.patterns);

    for source_file in &source_files {
        // Compute the relative path from the project root
        let rel_path = match source_file.strip_prefix(&config.source_path) {
            Ok(p) => p,
            Err(_) => {
                stats.errors.push(format!("path strip failed: {}", source_file.display()));
                continue;
            }
        };

        let mirror_path = mirror_dir.join(rel_path);

        match link_if_changed(source_file, &mirror_path) {
            Ok(changed) => {
                if changed {
                    stats.files_linked += 1;
                } else {
                    stats.files_skipped += 1;
                }
                stats.synced_mirror_paths.push(mirror_path);
            }
            Err(e) => {
                stats.errors.push(format!("{}: {e}", source_file.display()));
            }
        }
    }

    stats
}

/// Symlink a source file into the mirror location.
/// Creates a symlink at `dest` pointing to `source`. If the symlink already exists
/// and points to the same target, returns Ok(false) (unchanged). If it points
/// elsewhere or doesn't exist, (re)creates it and returns Ok(true).
///
/// Symlinks keep project files in sync without duplication — edits to the original
/// are immediately visible through the link.
fn link_if_changed(source: &Path, dest: &Path) -> anyhow::Result<bool> {
    let source_abs = std::fs::canonicalize(source)?;

    // Check if symlink already points to the right place
    if dest.is_symlink() {
        if let Ok(existing_target) = std::fs::read_link(dest)
            && existing_target == source_abs
        {
            return Ok(false); // already correct
        }
        // Wrong target or read_link failed — remove and recreate
        std::fs::remove_file(dest)?;
    } else if dest.exists() {
        // Regular file exists at dest (from old copy-based sync) — replace with symlink
        std::fs::remove_file(dest)?;
    }

    // Create parent directories
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }

    #[cfg(unix)]
    std::os::unix::fs::symlink(&source_abs, dest)?;

    #[cfg(not(unix))]
    {
        // Fallback: copy on non-Unix platforms
        let content = std::fs::read_to_string(source)?;
        std::fs::write(dest, content)?;
    }

    Ok(true)
}

/// Collect files matching glob patterns from a directory.
fn collect_matching_files(root: &Path, patterns: &[String]) -> Vec<PathBuf> {
    let mut files = Vec::new();

    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
    {
        let rel_path = entry
            .path()
            .strip_prefix(root)
            .unwrap_or(entry.path());
        let rel_str = rel_path.to_string_lossy();

        if patterns.iter().any(|p| matches_glob_pattern(p, &rel_str)) {
            files.push(entry.path().to_path_buf());
        }
    }

    files
}

/// Simple glob matching: supports `*` (single segment) and `**` (any depth).
fn matches_glob_pattern(pattern: &str, path: &str) -> bool {
    // Handle exact filename match
    if !pattern.contains('/') && !pattern.contains('*') {
        return path.ends_with(pattern)
            || Path::new(path)
                .file_name()
                .is_some_and(|f| f.to_string_lossy() == pattern);
    }

    // Handle **/filename pattern (match filename anywhere)
    if pattern.starts_with("**/") {
        let suffix = &pattern[3..];
        if !suffix.contains('*') {
            return path.ends_with(suffix)
                || Path::new(path)
                    .file_name()
                    .is_some_and(|f| f.to_string_lossy() == suffix);
        }
    }

    // Handle prefix/**/*.ext pattern
    if let Some(star_star_pos) = pattern.find("**") {
        let prefix = &pattern[..star_star_pos];
        let suffix = &pattern[star_star_pos + 2..];

        let prefix_matches = if prefix.is_empty() {
            true
        } else {
            path.starts_with(prefix.trim_end_matches('/'))
        };

        let suffix_matches = if suffix.is_empty() || suffix == "/" {
            true
        } else {
            let suffix = suffix.trim_start_matches('/');
            if suffix.contains('*') {
                // *.ext pattern
                let ext = suffix.trim_start_matches("*");
                path.ends_with(ext)
            } else {
                path.ends_with(suffix)
            }
        };

        return prefix_matches && suffix_matches;
    }

    // Fallback: simple contains check
    path.contains(pattern.trim_matches('*'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glob_readme() {
        assert!(matches_glob_pattern("README.md", "README.md"));
        assert!(matches_glob_pattern("README.md", "docs/README.md"));
    }

    #[test]
    fn test_glob_double_star() {
        assert!(matches_glob_pattern("docs/**/*.md", "docs/architecture.md"));
        assert!(matches_glob_pattern("docs/**/*.md", "docs/sub/design.md"));
        assert!(!matches_glob_pattern("docs/**/*.md", "src/main.rs"));
    }

    #[test]
    fn test_glob_star_star_readme() {
        assert!(matches_glob_pattern("**/README.md", "README.md"));
        assert!(matches_glob_pattern("**/README.md", "docs/README.md"));
        assert!(matches_glob_pattern("**/README.md", "a/b/c/README.md"));
    }
}
