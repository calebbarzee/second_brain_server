//! Git operations for the notes directory.
//!
//! Provides snapshot commits, diff, and revert for skill safety,
//! plus branch-safe single-file commits for MCP tool writes.
//!
//! # Branch model
//!
//! Users and AI agents work together on date-stamped branches:
//!
//! ```text
//! main                                    ← protected, never committed to directly
//!   └─ alice/2026-03-18/working           ← default session branch (date-stamped)
//!        ├─ human commit                  (git config user.name = alice)
//!        ├─ AI commit                     (git config user.name = claude-ai)
//!        └─ human commit
//!   └─ alice/2026-03-18/notes_on_bees     ← custom topic branch
//! ```
//!
//! - Default branch: `<username>/<YYYY-MM-DD>/working` (auto-generated per session).
//! - Custom branches: `<username>/<date>/<topic>` (user-specified).
//! - AI edits are committed to that same branch with the AI author identity.
//! - Protected branches (`main`, `master`, `staging`, `dev`) are never written to.
//! - The branch must be prefixed with the repo's `user.name` from git config.
//! - AI-touched files get `edited_by: ai` in their YAML frontmatter.
//!
//! After a work session the user reviews and merges to main via PR or manual merge.

use sb_core::worktree::PROTECTED_BRANCHES;
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

/// Get the current git branch name.
pub fn current_branch(notes_root: &Path) -> anyhow::Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(notes_root)
        .output()?;
    if !output.status.success() {
        anyhow::bail!("git rev-parse --abbrev-ref HEAD failed");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Get the git config `user.name` for this repo.
pub fn git_username(notes_root: &Path) -> anyhow::Result<String> {
    let output = Command::new("git")
        .args(["config", "user.name"])
        .current_dir(notes_root)
        .output()?;
    if !output.status.success() {
        anyhow::bail!("git config user.name not set");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Validate that the current branch is safe to commit to.
///
/// Returns `Ok(branch_name)` if the branch is valid, or an error explaining
/// why the commit was refused.
///
/// Rules:
/// 1. Must not be a protected branch (main, master, staging, dev).
/// 2. Must be prefixed with the repo owner's git username
///    (from `git config user.name` or the `repo_owner` parameter).
///
/// The `repo_owner` is the human user who owns the notes repo — the branch
/// must start with their username regardless of who is committing.
pub fn validate_branch(notes_root: &Path, repo_owner: &str) -> anyhow::Result<String> {
    let branch = current_branch(notes_root)?;

    // Rule 1: not a protected branch
    if PROTECTED_BRANCHES.contains(&branch.as_str()) {
        anyhow::bail!(
            "Refusing to commit to protected branch '{branch}'. \
             Please checkout a working branch first, e.g.: \
             git checkout -b {repo_owner}/note-edits"
        );
    }

    // Rule 2: branch must be prefixed with repo owner's username
    if !branch.starts_with(&format!("{repo_owner}/")) {
        anyhow::bail!(
            "Branch '{branch}' is not owned by '{repo_owner}'. \
             Expected a branch like '{repo_owner}/<topic>'. \
             Please checkout the correct branch first."
        );
    }

    Ok(branch)
}

/// Commit a single file to the current branch with the AI author identity.
///
/// This is the main entry point for MCP tool writes (note_create, note_update).
///
/// # Safety checks
/// - Validates the current branch (not protected, owned by repo owner).
/// - Only stages the specified file — other uncommitted changes are untouched.
/// - Uses the `ai_author_name` / `ai_author_email` as the commit author
///   so AI changes are distinguishable from human changes in `git log`.
///
/// # Returns
/// `Ok(Some((branch, sha)))` on success, `Ok(None)` if nothing to commit,
/// or `Err` if the branch is invalid or git operations fail.
pub fn commit_file(
    notes_root: &Path,
    file_path: &Path,
    message: &str,
    repo_owner: &str,
    ai_author_name: &str,
    ai_author_email: &str,
) -> anyhow::Result<Option<(String, String)>> {
    let branch = validate_branch(notes_root, repo_owner)?;

    // Stage only this specific file
    let status = Command::new("git")
        .args(["add", "--"])
        .arg(file_path)
        .current_dir(notes_root)
        .status()?;
    if !status.success() {
        anyhow::bail!("git add failed for {}", file_path.display());
    }

    // Check if this file is actually staged
    let output = Command::new("git")
        .args(["diff", "--cached", "--name-only"])
        .current_dir(notes_root)
        .output()?;
    if output.stdout.is_empty() {
        return Ok(None);
    }

    // Commit with the AI author identity
    let author_str = format!("{ai_author_name} <{ai_author_email}>");
    let status = Command::new("git")
        .args(["commit", "--author", &author_str, "-m", message])
        .current_dir(notes_root)
        .status()?;
    if !status.success() {
        anyhow::bail!("git commit failed");
    }

    // Return the commit SHA
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(notes_root)
        .output()?;

    if output.status.success() {
        let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(Some((branch, sha)))
    } else {
        anyhow::bail!("git rev-parse HEAD failed");
    }
}

/// Check if a path is inside a git repository.
pub fn is_git_repo(path: &Path) -> bool {
    Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(path)
        .output()
        .is_ok_and(|o| o.status.success())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// Create a temp git repo on a user-owned branch, ready for testing.
    fn init_repo_on_branch(dir: &Path, user: &str, branch: &str) {
        Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(dir)
            .output()
            .expect("git init");
        Command::new("git")
            .args(["config", "user.name", user])
            .current_dir(dir)
            .output()
            .expect("git config user.name");
        Command::new("git")
            .args(["config", "user.email", &format!("{user}@test.com")])
            .current_dir(dir)
            .output()
            .expect("git config user.email");
        std::fs::write(dir.join("README.md"), "# test\n").unwrap();
        Command::new("git")
            .args(["add", "README.md"])
            .current_dir(dir)
            .output()
            .expect("git add");
        Command::new("git")
            .args(["commit", "-m", "initial commit"])
            .current_dir(dir)
            .output()
            .expect("git commit");
        Command::new("git")
            .args(["checkout", "-b", branch])
            .current_dir(dir)
            .output()
            .expect("git checkout -b");
    }

    #[test]
    fn is_git_repo_true() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo_on_branch(tmp.path(), "alice", "alice/work");
        assert!(is_git_repo(tmp.path()));
    }

    #[test]
    fn is_git_repo_false() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!is_git_repo(tmp.path()));
    }

    #[test]
    fn current_branch_returns_name() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo_on_branch(tmp.path(), "alice", "alice/2026-03-18/working");
        assert_eq!(
            current_branch(tmp.path()).unwrap(),
            "alice/2026-03-18/working"
        );
    }

    #[test]
    fn git_username_returns_config() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo_on_branch(tmp.path(), "alice", "alice/work");
        assert_eq!(git_username(tmp.path()).unwrap(), "alice");
    }

    #[test]
    fn is_clean_on_fresh_repo() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo_on_branch(tmp.path(), "alice", "alice/work");
        assert!(is_clean(tmp.path()).unwrap());
    }

    #[test]
    fn is_clean_false_with_changes() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo_on_branch(tmp.path(), "alice", "alice/work");
        std::fs::write(tmp.path().join("dirty.md"), "change").unwrap();
        assert!(!is_clean(tmp.path()).unwrap());
    }

    #[test]
    fn validate_branch_rejects_protected() {
        let tmp = tempfile::tempdir().unwrap();
        // Stay on main (don't switch to user branch)
        Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "alice"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "a@t.com"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        std::fs::write(tmp.path().join("README.md"), "# t\n").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(tmp.path())
            .output()
            .unwrap();

        let err = validate_branch(tmp.path(), "alice").unwrap_err();
        assert!(
            err.to_string().contains("protected"),
            "expected protected error, got: {err}"
        );
    }

    #[test]
    fn validate_branch_rejects_wrong_owner() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo_on_branch(tmp.path(), "alice", "bob/sneaky");
        let err = validate_branch(tmp.path(), "alice").unwrap_err();
        assert!(
            err.to_string().contains("not owned by"),
            "expected owner error, got: {err}"
        );
    }

    #[test]
    fn validate_branch_accepts_valid() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo_on_branch(tmp.path(), "alice", "alice/2026-03-18/working");
        let branch = validate_branch(tmp.path(), "alice").unwrap();
        assert_eq!(branch, "alice/2026-03-18/working");
    }

    #[test]
    fn snapshot_commit_and_diff() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo_on_branch(tmp.path(), "alice", "alice/work");

        // Clean repo — snapshot should return None
        assert!(snapshot_commit(tmp.path(), "nothing").unwrap().is_none());

        // Make a change and snapshot
        std::fs::write(tmp.path().join("note.md"), "# Hello\n").unwrap();
        let sha = snapshot_commit(tmp.path(), "add note")
            .unwrap()
            .expect("should have a commit");
        assert!(!sha.is_empty());
        assert!(is_clean(tmp.path()).unwrap());
    }

    #[test]
    fn commit_file_single_file_only() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo_on_branch(tmp.path(), "alice", "alice/work");

        // Create two files but only commit one
        std::fs::write(tmp.path().join("a.md"), "file a").unwrap();
        std::fs::write(tmp.path().join("b.md"), "file b").unwrap();

        let result = commit_file(
            tmp.path(),
            Path::new("a.md"),
            "ai: create a.md",
            "alice",
            "claude-ai",
            "ai@test.local",
        )
        .unwrap()
        .expect("should commit");

        assert_eq!(result.0, "alice/work");
        assert!(!result.1.is_empty());

        // b.md should still be untracked
        let output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        let status = String::from_utf8_lossy(&output.stdout);
        assert!(status.contains("b.md"), "b.md should still be untracked");
        assert!(
            !status.contains("a.md"),
            "a.md should be committed and clean"
        );

        // Verify AI author
        let output = Command::new("git")
            .args(["log", "-1", "--format=%an"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        let author = String::from_utf8_lossy(&output.stdout).trim().to_string();
        assert_eq!(author, "claude-ai");
    }

    #[test]
    fn commit_file_nothing_to_commit() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo_on_branch(tmp.path(), "alice", "alice/work");

        // Commit README.md which is already committed — nothing to stage
        let result = commit_file(
            tmp.path(),
            Path::new("README.md"),
            "ai: no-op",
            "alice",
            "claude-ai",
            "ai@test.local",
        )
        .unwrap();

        assert!(result.is_none(), "should be None when nothing to commit");
    }
}
