//! Infer project associations from note filenames and paths.
//!
//! Detection strategies (in priority order):
//! 1. Configured path→project mappings (highest confidence)
//! 2. Path-based: notes under `projects/<project_name>/` → project "<project_name>"
//! 3. Files directly in `projects/` dir → extract project name from filename segments
//! 4. Fuzzy substring match against known project names (for files anywhere)
//! 5. Frontmatter `project:` field (TODO: wire up during ingestion)

use std::path::Path;

/// Result of project detection for a note.
#[derive(Debug, Clone)]
pub struct DetectedProject {
    pub name: String,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Confidence {
    /// Matches a configured path→project mapping
    Configured,
    /// Found in a `projects/<name>/` subdirectory
    PathBased,
    /// File is directly in `projects/` dir, name extracted from filename
    ProjectsDirFile,
    /// Filename contains a known project name as a substring
    FuzzyMatch,
}

/// A configured mapping from path pattern to project name.
#[derive(Debug, Clone)]
pub struct ProjectMapping {
    pub project_name: String,
    /// A path prefix or pattern to match against note file paths
    pub path_prefix: String,
}

/// Common filename segments that are descriptors, not project names.
const DESCRIPTOR_SEGMENTS: &[&str] = &[
    "architecture",
    "design",
    "status",
    "notes",
    "todo",
    "todos",
    "wip",
    "draft",
    "scratch",
    "daily",
    "weekly",
    "monthly",
    "meeting",
    "review",
    "summary",
    "plan",
    "spec",
    "reference",
    "readme",
    "changelog",
    "log",
    "dump",
    "brainstorm",
];

/// Detect which project a note belongs to.
///
/// `known_projects` enables fuzzy matching: if a known project name appears as a
/// substring in the filename (case-insensitive), it matches with `FuzzyMatch` confidence.
pub fn detect_project(
    file_path: &str,
    mappings: &[ProjectMapping],
    known_projects: &[String],
) -> Option<DetectedProject> {
    // 1. Check configured mappings first (highest confidence)
    for mapping in mappings {
        if file_path.contains(&mapping.path_prefix) {
            return Some(DetectedProject {
                name: mapping.project_name.clone(),
                confidence: Confidence::Configured,
            });
        }
    }

    // 2. Check for projects/<name>/file.md path pattern (subdir)
    if let Some(project_name) = extract_project_from_subdir(file_path) {
        return Some(DetectedProject {
            name: project_name,
            confidence: Confidence::PathBased,
        });
    }

    // 3. Files directly in a `projects/` directory (no subdirectory between projects/ and file)
    if let Some(project_name) = extract_project_from_projects_dir_file(file_path) {
        return Some(DetectedProject {
            name: project_name,
            confidence: Confidence::ProjectsDirFile,
        });
    }

    // 4. Fuzzy match against known project names (case-insensitive substring)
    if let Some(project_name) = fuzzy_match_known_projects(file_path, known_projects) {
        return Some(DetectedProject {
            name: project_name,
            confidence: Confidence::FuzzyMatch,
        });
    }

    None
}

/// Extract project name from `projects/<name>/...` path pattern.
/// Only matches when there's a subdirectory under `projects/` (not direct files).
fn extract_project_from_subdir(file_path: &str) -> Option<String> {
    let path = Path::new(file_path);
    let components: Vec<_> = path.components().collect();

    for (i, component) in components.iter().enumerate() {
        if let std::path::Component::Normal(os_str) = component
            && os_str.to_string_lossy() == "projects"
            && i + 2 < components.len()
            && let Some(std::path::Component::Normal(name)) = components.get(i + 1)
        {
            return Some(name.to_string_lossy().to_string());
        }
    }
    None
}

/// Extract project name from a file directly in `projects/` directory.
/// E.g. `projects/stale_bread_architecture.md` → "stale_bread"
/// E.g. `projects/TODO_myproject_2026-03-05.md` → "myproject"
///
/// Strategy: split filename by `_`, filter out date patterns and known descriptors,
/// then join the remaining segments as the project name.
fn extract_project_from_projects_dir_file(file_path: &str) -> Option<String> {
    let path = Path::new(file_path);
    let components: Vec<_> = path.components().collect();

    // Find `projects` component and check the file is directly inside it
    let mut in_projects_dir = false;
    for (i, component) in components.iter().enumerate() {
        if let std::path::Component::Normal(os_str) = component
            && os_str.to_string_lossy() == "projects"
            && i + 1 == components.len() - 1
        {
            in_projects_dir = true;
            break;
        }
    }

    if !in_projects_dir {
        return None;
    }

    let filename = path.file_stem()?.to_string_lossy().to_string();
    extract_project_name_from_segments(&filename)
}

/// Given a filename stem like "stale_bread_architecture" or "TODO_myproject_2026-03-05",
/// extract the project name by filtering out descriptors and dates.
fn extract_project_name_from_segments(filename: &str) -> Option<String> {
    let segments: Vec<&str> = filename.split('_').collect();

    let project_segments: Vec<&str> = segments
        .iter()
        .filter(|s| !is_descriptor_segment(s) && !is_date_segment(s))
        .copied()
        .collect();

    if project_segments.is_empty() {
        return None;
    }

    Some(project_segments.join("_"))
}

/// Check if a segment is a common descriptor (case-insensitive).
fn is_descriptor_segment(segment: &str) -> bool {
    let lower = segment.to_lowercase();
    DESCRIPTOR_SEGMENTS.contains(&lower.as_str())
}

/// Check if a segment looks like a date (YYYY-MM-DD, YYYYMMDD, or just digits ≥ 6 chars).
fn is_date_segment(segment: &str) -> bool {
    // YYYY-MM-DD
    if segment.len() == 10
        && segment.chars().nth(4) == Some('-')
        && segment.chars().nth(7) == Some('-')
    {
        return segment
            .chars()
            .filter(|c| *c != '-')
            .all(|c| c.is_ascii_digit());
    }
    // YYYYMMDD
    if segment.len() == 8 && segment.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    // MM-DD-YYYY or similar
    if segment.len() >= 6 && segment.chars().filter(|c| c.is_ascii_digit()).count() >= 6 {
        return true;
    }
    false
}

/// Fuzzy match: check if any known project name appears as a case-insensitive
/// substring in the filename.
fn fuzzy_match_known_projects(file_path: &str, known_projects: &[String]) -> Option<String> {
    if known_projects.is_empty() {
        return None;
    }

    let filename = Path::new(file_path)
        .file_stem()?
        .to_string_lossy()
        .to_lowercase();

    // Sort by length descending so longer project names match first
    // (avoids "ai" matching inside "stale_brain")
    let mut sorted_projects: Vec<&String> = known_projects.iter().collect();
    sorted_projects.sort_by_key(|b| std::cmp::Reverse(b.len()));

    for project in sorted_projects {
        let project_lower = project.to_lowercase();
        if project_lower.len() >= 2 && filename.contains(&project_lower) {
            return Some(project.clone());
        }
    }

    None
}

/// Extract a date from a filename, if present.
/// Supports: `2026-03-04_foo.md`, `20260304_foo.md`, `foo_2026-03-04.md`
pub fn extract_date_from_filename(file_path: &str) -> Option<chrono::NaiveDate> {
    let filename = Path::new(file_path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())?;

    // Try YYYY-MM-DD pattern anywhere in filename
    for (i, _) in filename.match_indices(|c: char| c.is_ascii_digit()) {
        if i + 10 <= filename.len() {
            let candidate = &filename[i..i + 10];
            if let Ok(date) = chrono::NaiveDate::parse_from_str(candidate, "%Y-%m-%d") {
                return Some(date);
            }
        }
        // Also try YYYYMMDD
        if i + 8 <= filename.len() {
            let candidate = &filename[i..i + 8];
            if let Ok(date) = chrono::NaiveDate::parse_from_str(candidate, "%Y%m%d") {
                return Some(date);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_path_based_subdir() {
        // projects/<name>/file.md → project is the subdir name
        let result = detect_project(
            "/home/user/notes/projects/bread/stale_design.md",
            &[],
            &[],
        );
        assert!(result.is_some());
        let p = result.unwrap();
        assert_eq!(p.name, "bread");
        assert_eq!(p.confidence, Confidence::PathBased);
    }

    #[test]
    fn test_detect_projects_dir_file() {
        // File directly in projects/ with descriptors stripped
        let result = detect_project(
            "/home/user/notes/projects/stale_bread_architecture.md",
            &[],
            &[],
        );
        assert!(result.is_some());
        let p = result.unwrap();
        assert_eq!(p.name, "stale_bread");
        assert_eq!(p.confidence, Confidence::ProjectsDirFile);
    }

    #[test]
    fn test_detect_projects_dir_file_with_todo_prefix() {
        // TODO_projectname_date pattern
        let result = detect_project(
            "/home/user/notes/projects/TODO_myproject_2026-03-05.md",
            &[],
            &[],
        );
        assert!(result.is_some());
        let p = result.unwrap();
        assert_eq!(p.name, "myproject");
        assert_eq!(p.confidence, Confidence::ProjectsDirFile);
    }

    #[test]
    fn test_detect_configured_mapping() {
        let mappings = vec![ProjectMapping {
            project_name: "stale_bread".to_string(),
            path_prefix: "stale_bread".to_string(),
        }];
        let result = detect_project(
            "/home/user/notes/stale_bread_status.md",
            &mappings,
            &[],
        );
        assert!(result.is_some());
        let p = result.unwrap();
        assert_eq!(p.name, "stale_bread");
        assert_eq!(p.confidence, Confidence::Configured);
    }

    #[test]
    fn test_fuzzy_match_known_project() {
        let known = vec!["stale_bread".to_string()];
        let result = detect_project(
            "/home/user/notes/stale_bread_architecture.md",
            &[],
            &known,
        );
        assert!(result.is_some());
        let p = result.unwrap();
        assert_eq!(p.name, "stale_bread");
        assert_eq!(p.confidence, Confidence::FuzzyMatch);
    }

    #[test]
    fn test_no_detection() {
        let result = detect_project("/home/user/notes/random-note.md", &[], &[]);
        assert!(result.is_none());
    }

    #[test]
    fn test_short_name_not_fuzzy_matched() {
        // Single-char project names shouldn't fuzzy match
        let known = vec!["a".to_string()];
        let result = detect_project("/home/user/notes/a_note.md", &[], &known);
        assert!(result.is_none());
    }

    #[test]
    fn test_longer_project_matches_first() {
        // "stale_bread" should match before "stale"
        let known = vec!["stale".to_string(), "stale_bread".to_string()];
        let result = detect_project(
            "/home/user/notes/stale_bread_status.md",
            &[],
            &known,
        );
        assert!(result.is_some());
        let p = result.unwrap();
        assert_eq!(p.name, "stale_bread");
    }

    #[test]
    fn test_extract_date_iso() {
        let date = extract_date_from_filename("2026-03-04_meeting.md");
        assert_eq!(
            date,
            Some(chrono::NaiveDate::from_ymd_opt(2026, 3, 4).unwrap())
        );
    }

    #[test]
    fn test_extract_date_compact() {
        let date = extract_date_from_filename("20260304_meeting.md");
        assert_eq!(
            date,
            Some(chrono::NaiveDate::from_ymd_opt(2026, 3, 4).unwrap())
        );
    }

    #[test]
    fn test_extract_no_date() {
        let date = extract_date_from_filename("random-note.md");
        assert!(date.is_none());
    }
}
