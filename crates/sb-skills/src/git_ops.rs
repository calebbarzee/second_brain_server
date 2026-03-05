//! Git operations for the notes directory.
//!
//! Provides snapshot commits, diff, and revert for skill safety.

use std::path::Path;
use std::process::Command;

/// Create a snapshot commit of all changes in the notes directory.
/// Returns the commit SHA on success.
pub fn snapshot_commit(notes_root: &Path, message: &str) -> anyhow::Result<Option<String>> {
    // Check if there are changes to commit
    if is_clean(notes_root)? {
        return Ok(None);
    }

    // Stage all changes
    let status = Command::new("git")
        .args(["add", "-A"])
        .current_dir(notes_root)
        .status()?;
    if !status.success() {
        anyhow::bail!("git add failed");
    }

    // Commit
    let status = Command::new("git")
        .args(["commit", "-m", message])
        .current_dir(notes_root)
        .status()?;
    if !status.success() {
        anyhow::bail!("git commit failed");
    }

    // Get the commit SHA
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(notes_root)
        .output()?;

    if output.status.success() {
        Ok(Some(
            String::from_utf8_lossy(&output.stdout).trim().to_string(),
        ))
    } else {
        anyhow::bail!("git rev-parse HEAD failed");
    }
}

/// Get the diff since a given commit.
pub fn diff_since(notes_root: &Path, commit_sha: &str) -> anyhow::Result<String> {
    let output = Command::new("git")
        .args(["diff", commit_sha, "HEAD"])
        .current_dir(notes_root)
        .output()?;

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Get the diff of uncommitted changes.
pub fn diff_uncommitted(notes_root: &Path) -> anyhow::Result<String> {
    let output = Command::new("git")
        .args(["diff", "HEAD"])
        .current_dir(notes_root)
        .output()?;

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Revert a specific commit.
pub fn revert_commit(notes_root: &Path, commit_sha: &str) -> anyhow::Result<()> {
    let status = Command::new("git")
        .args(["revert", "--no-edit", commit_sha])
        .current_dir(notes_root)
        .status()?;

    if !status.success() {
        anyhow::bail!("git revert failed for {commit_sha}");
    }
    Ok(())
}

/// Check if the notes directory has uncommitted changes.
pub fn is_clean(notes_root: &Path) -> anyhow::Result<bool> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(notes_root)
        .output()?;

    Ok(output.status.success() && output.stdout.is_empty())
}

/// Check if a path is inside a git repository.
pub fn is_git_repo(path: &Path) -> bool {
    Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(path)
        .output()
        .is_ok_and(|o| o.status.success())
}
