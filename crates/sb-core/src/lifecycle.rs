//! Note lifecycle classification.
//!
//! Lifecycle states:
//! - `active` (default) — living knowledge, actively useful
//! - `volatile` — brain dumps, running TODOs, daily logs
//! - `enduring` — architecture docs, reference material, finalized decisions
//! - `archived` — retained for reference but excluded from default queries

use std::path::Path;

/// Note lifecycle states.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lifecycle {
    Active,
    Volatile,
    Enduring,
    Archived,
}

impl Lifecycle {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Volatile => "volatile",
            Self::Enduring => "enduring",
            Self::Archived => "archived",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "active" => Some(Self::Active),
            "volatile" => Some(Self::Volatile),
            "enduring" => Some(Self::Enduring),
            "archived" => Some(Self::Archived),
            _ => None,
        }
    }
}

impl std::fmt::Display for Lifecycle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Classify a note's lifecycle from filename patterns and frontmatter.
/// Frontmatter `lifecycle: <value>` always takes precedence.
pub fn classify_note(file_path: &str, frontmatter: Option<&serde_json::Value>) -> Lifecycle {
    // 1. Frontmatter override (highest priority)
    if let Some(fm) = frontmatter
        && let Some(lifecycle_str) = fm.get("lifecycle").and_then(|v| v.as_str())
        && let Some(lc) = Lifecycle::parse(lifecycle_str)
    {
        return lc;
    }

    // 2. Path-based: anything in archive/ directory
    if file_path.contains("/archive/") || file_path.starts_with("archive/") {
        return Lifecycle::Archived;
    }

    // 3. Filename heuristics
    let filename = Path::new(file_path)
        .file_name()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    // Volatile patterns
    if filename.starts_with("todo")
        || filename.contains("_todo")
        || filename.contains("_daily")
        || filename.contains("_dump")
        || filename.starts_with("daily_")
        || filename.starts_with("dump_")
        || filename.starts_with("scratch_")
        || filename.starts_with("wip_")
    {
        return Lifecycle::Volatile;
    }

    // Enduring patterns
    if filename.starts_with("architecture")
        || filename.starts_with("design_")
        || filename.starts_with("reference_")
        || filename.starts_with("spec_")
        || filename == "readme.md"
    {
        return Lifecycle::Enduring;
    }

    // Default
    Lifecycle::Active
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_volatile_todo() {
        assert_eq!(classify_note("TODO_list.md", None), Lifecycle::Volatile);
        assert_eq!(
            classify_note("project_todo_items.md", None),
            Lifecycle::Volatile
        );
    }

    #[test]
    fn test_classify_volatile_daily() {
        assert_eq!(
            classify_note("2026-03-04_daily_log.md", None),
            Lifecycle::Volatile
        );
    }

    #[test]
    fn test_classify_enduring() {
        assert_eq!(
            classify_note("architecture_overview.md", None),
            Lifecycle::Enduring
        );
        assert_eq!(classify_note("README.md", None), Lifecycle::Enduring);
    }

    #[test]
    fn test_classify_archived() {
        assert_eq!(
            classify_note("/notes/archive/old_note.md", None),
            Lifecycle::Archived
        );
    }

    #[test]
    fn test_classify_default_active() {
        assert_eq!(classify_note("meeting-notes.md", None), Lifecycle::Active);
    }

    #[test]
    fn test_frontmatter_override() {
        let fm = serde_json::json!({"lifecycle": "enduring"});
        assert_eq!(
            classify_note("TODO_list.md", Some(&fm)),
            Lifecycle::Enduring
        );
    }
}
