//! Filesystem-level search using ripgrep and fd/fdfind.
//!
//! Provides a DB-free fallback for searching notes that haven't been ingested yet.
//! Shells out to `rg` for content search and `fd`/`fdfind`/`find` for filename search.

use std::path::PathBuf;
use std::process::Command;

#[derive(Debug)]
pub struct FileSearchResult {
    pub file_path: PathBuf,
    /// Matched line content (for content search) or filename (for filename search)
    pub matched_text: String,
    /// Line number if from content search
    pub line_number: Option<u32>,
}

/// Search note contents using ripgrep. Falls back gracefully if rg is not installed.
pub fn search_content(
    dirs: &[PathBuf],
    query: &str,
    max_results: usize,
) -> anyhow::Result<Vec<FileSearchResult>> {
    let rg = find_binary(&["rg", "ripgrep"]);
    match rg {
        Some(bin) => search_content_rg(&bin, dirs, query, max_results),
        None => search_content_grep(dirs, query, max_results),
    }
}

/// Search for files by name pattern using fd/fdfind. Falls back to find.
pub fn search_filename(
    dirs: &[PathBuf],
    pattern: &str,
    max_results: usize,
) -> anyhow::Result<Vec<FileSearchResult>> {
    let fd = find_binary(&["fd", "fdfind"]);
    match fd {
        Some(bin) => search_filename_fd(&bin, dirs, pattern, max_results),
        None => search_filename_find(dirs, pattern, max_results),
    }
}

fn find_binary(names: &[&str]) -> Option<String> {
    for name in names {
        if let Ok(output) = Command::new("which").arg(name).output()
            && output.status.success()
        {
            return Some(
                String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .to_string(),
            );
        }
    }
    None
}

fn search_content_rg(
    rg_bin: &str,
    dirs: &[PathBuf],
    query: &str,
    max_results: usize,
) -> anyhow::Result<Vec<FileSearchResult>> {
    let mut results = Vec::new();

    for dir in dirs {
        if !dir.exists() {
            continue;
        }

        let output = Command::new(rg_bin)
            .args([
                "--line-number",
                "--no-heading",
                "--color=never",
                "--type=md",
                "--max-count",
                &max_results.to_string(),
                "--ignore-case",
                query,
            ])
            .arg(dir)
            .output()?;

        // rg exits 1 for no matches — that's fine
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines().take(max_results - results.len()) {
                if let Some(result) = parse_rg_line(line) {
                    results.push(result);
                }
            }
        }

        if results.len() >= max_results {
            break;
        }
    }

    results.truncate(max_results);
    Ok(results)
}

fn search_content_grep(
    dirs: &[PathBuf],
    query: &str,
    max_results: usize,
) -> anyhow::Result<Vec<FileSearchResult>> {
    let mut results = Vec::new();

    for dir in dirs {
        if !dir.exists() {
            continue;
        }

        let output = Command::new("grep")
            .args([
                "-r",
                "-n",
                "-i",
                "--include=*.md",
                &format!("-m{max_results}"),
                query,
            ])
            .arg(dir)
            .output()?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines().take(max_results - results.len()) {
                if let Some(result) = parse_rg_line(line) {
                    results.push(result);
                }
            }
        }

        if results.len() >= max_results {
            break;
        }
    }

    results.truncate(max_results);
    Ok(results)
}

/// Parse a line of `rg -n` or `grep -n` output: `filepath:linenum:matched text`
fn parse_rg_line(line: &str) -> Option<FileSearchResult> {
    let (path_str, rest) = line.split_once(':')?;
    let (line_num_str, matched) = rest.split_once(':')?;
    let line_number = line_num_str.parse::<u32>().ok();

    Some(FileSearchResult {
        file_path: PathBuf::from(path_str),
        matched_text: matched.trim().to_string(),
        line_number,
    })
}

fn search_filename_fd(
    fd_bin: &str,
    dirs: &[PathBuf],
    pattern: &str,
    max_results: usize,
) -> anyhow::Result<Vec<FileSearchResult>> {
    let mut results = Vec::new();

    for dir in dirs {
        if !dir.exists() {
            continue;
        }

        let output = Command::new(fd_bin)
            .args([
                "--type=f",
                "--extension=md",
                "--max-results",
                &max_results.to_string(),
                pattern,
            ])
            .arg(dir)
            .output()?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines().take(max_results - results.len()) {
                let path = PathBuf::from(line.trim());
                let filename = path
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_default();
                results.push(FileSearchResult {
                    file_path: path,
                    matched_text: filename,
                    line_number: None,
                });
            }
        }

        if results.len() >= max_results {
            break;
        }
    }

    results.truncate(max_results);
    Ok(results)
}

fn search_filename_find(
    dirs: &[PathBuf],
    pattern: &str,
    max_results: usize,
) -> anyhow::Result<Vec<FileSearchResult>> {
    let mut results = Vec::new();

    for dir in dirs {
        if !dir.exists() {
            continue;
        }

        let output = Command::new("find")
            .arg(dir)
            .args(["-type", "f", "-name", &format!("*{pattern}*.md")])
            .output()?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines().take(max_results - results.len()) {
                let path = PathBuf::from(line.trim());
                let filename = path
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_default();
                results.push(FileSearchResult {
                    file_path: path,
                    matched_text: filename,
                    line_number: None,
                });
            }
        }

        if results.len() >= max_results {
            break;
        }
    }

    results.truncate(max_results);
    Ok(results)
}

/// Discover notes directories by searching up to `max_depth` subdirectories of `$HOME`
/// for directories named "notes".
pub fn discover_notes_dirs(max_depth: u8) -> Vec<PathBuf> {
    let home = match std::env::var_os("HOME") {
        Some(h) => PathBuf::from(h),
        None => return vec![],
    };

    let mut found = Vec::new();

    // Check $HOME/notes directly
    let direct = home.join("notes");
    if direct.is_dir() {
        found.push(direct);
    }

    // Search subdirectories up to max_depth
    if max_depth >= 1
        && let Ok(entries) = std::fs::read_dir(&home)
    {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            // Skip hidden directories
            if path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with('.'))
            {
                continue;
            }

            let notes_subdir = path.join("notes");
            if notes_subdir.is_dir() && !found.contains(&notes_subdir) {
                found.push(notes_subdir);
            }

            // Depth 2: check one more level down
            if max_depth >= 2
                && let Ok(sub_entries) = std::fs::read_dir(&path)
            {
                for sub_entry in sub_entries.filter_map(|e| e.ok()) {
                    let sub_path = sub_entry.path();
                    if !sub_path.is_dir() {
                        continue;
                    }
                    if sub_path
                        .file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with('.'))
                    {
                        continue;
                    }
                    let notes_sub = sub_path.join("notes");
                    if notes_sub.is_dir() && !found.contains(&notes_sub) {
                        found.push(notes_sub);
                    }
                }
            }
        }
    }

    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_rg_line() {
        let line = "/home/user/notes/test.md:42:This is a matched line";
        let result = parse_rg_line(line).unwrap();
        assert_eq!(result.file_path, PathBuf::from("/home/user/notes/test.md"));
        assert_eq!(result.line_number, Some(42));
        assert_eq!(result.matched_text, "This is a matched line");
    }

    #[test]
    fn test_parse_rg_line_no_match() {
        assert!(parse_rg_line("no colon here").is_none());
    }

    #[test]
    fn test_find_binary_rg() {
        // rg should be available in test environment
        let result = find_binary(&["rg"]);
        // Don't assert is_some — CI may not have rg
        if let Some(path) = result {
            assert!(path.contains("rg"));
        }
    }
}
