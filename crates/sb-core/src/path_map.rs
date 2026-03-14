//! Path mapping between canonical (repo-relative) paths and absolute filesystem paths.
//!
//! The DB stores canonical paths like `daily/today.md`. Each session resolves
//! these to absolute paths in its own worktree or the main repo.

use std::path::{Path, PathBuf};

/// Maps between canonical (repo-relative) paths and absolute filesystem paths.
#[derive(Debug, Clone)]
pub struct PathMapper {
    /// Root of the git repository (main checkout or worktree).
    root: PathBuf,
}

impl PathMapper {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Convert an absolute filesystem path to a canonical (repo-relative) path.
    /// Returns `None` if the path is not under the root.
    pub fn to_canonical(&self, abs_path: &Path) -> Option<String> {
        abs_path
            .strip_prefix(&self.root)
            .ok()
            .map(|rel| rel.to_string_lossy().to_string())
    }

    /// Convert a canonical (repo-relative) path to an absolute filesystem path.
    pub fn to_absolute(&self, canonical: &str) -> PathBuf {
        self.root.join(canonical)
    }

    /// Normalize a path that may be absolute or already canonical.
    /// If it starts with the root prefix, strip it. Otherwise return as-is.
    pub fn normalize(&self, path: &str) -> String {
        let p = Path::new(path);
        if p.is_absolute() {
            self.to_canonical(p).unwrap_or_else(|| path.to_string())
        } else {
            path.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_canonical() {
        let mapper = PathMapper::new(PathBuf::from("/data/notes"));
        assert_eq!(
            mapper.to_canonical(Path::new("/data/notes/daily/today.md")),
            Some("daily/today.md".to_string())
        );
    }

    #[test]
    fn test_to_canonical_root_file() {
        let mapper = PathMapper::new(PathBuf::from("/data/notes"));
        assert_eq!(
            mapper.to_canonical(Path::new("/data/notes/todo.md")),
            Some("todo.md".to_string())
        );
    }

    #[test]
    fn test_to_canonical_outside_root() {
        let mapper = PathMapper::new(PathBuf::from("/data/notes"));
        assert_eq!(mapper.to_canonical(Path::new("/other/path/note.md")), None);
    }

    #[test]
    fn test_to_absolute() {
        let mapper = PathMapper::new(PathBuf::from("/data/notes"));
        assert_eq!(
            mapper.to_absolute("daily/today.md"),
            PathBuf::from("/data/notes/daily/today.md")
        );
    }

    #[test]
    fn test_normalize_absolute() {
        let mapper = PathMapper::new(PathBuf::from("/data/notes"));
        assert_eq!(mapper.normalize("/data/notes/todo.md"), "todo.md");
    }

    #[test]
    fn test_normalize_already_canonical() {
        let mapper = PathMapper::new(PathBuf::from("/data/notes"));
        assert_eq!(mapper.normalize("todo.md"), "todo.md");
    }

    #[test]
    fn test_normalize_outside_root() {
        let mapper = PathMapper::new(PathBuf::from("/data/notes"));
        assert_eq!(mapper.normalize("/other/path.md"), "/other/path.md");
    }
}
