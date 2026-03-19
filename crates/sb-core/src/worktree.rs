//! Git worktree management for multi-user session isolation.
//!
//! Each MCP session gets its own git worktree so multiple users can
//! concurrently edit notes on separate branches without conflicts.

use std::path::PathBuf;
use std::process::Command;

/// Branches that must never be committed to directly.
pub const PROTECTED_BRANCHES: &[&str] = &["main", "master", "staging", "dev"];

/// Configuration for the worktree subsystem.
#[derive(Debug, Clone)]
pub struct WorktreeConfig {
    /// Root of the main git repository (e.g., `/data/notes`).
    pub main_repo: PathBuf,
    /// Directory where session worktrees are created (e.g., `/data/worktrees`).
    pub worktree_dir: PathBuf,
    /// The branch that the main repo and DB index track (e.g., `main`).
    pub tracked_branch: String,
}

/// Info about an active session with a worktree.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub session_id: String,
    pub username: String,
    pub email: String,
    pub branch: String,
    pub worktree_path: PathBuf,
}

/// Create a git worktree for a new session.
///
/// - Validates branch naming: must be `<username>/...` and not protected.
/// - If the branch exists, reuses it. If not, creates from `tracked_branch`.
/// - Configures git user identity in the worktree.
/// - Returns `SessionInfo` on success.
pub fn create_worktree(
    config: &WorktreeConfig,
    session_id: &str,
    username: &str,
    email: &str,
    branch: Option<&str>,
) -> anyhow::Result<SessionInfo> {
    let branch = match branch {
        Some(b) => b.to_string(),
        None => default_branch(username),
    };

    // Validate: must be prefixed with username
    if !branch.starts_with(&format!("{username}/")) {
        anyhow::bail!("Branch '{branch}' must be prefixed with '{username}/'");
    }

    // Validate: not a protected branch
    if PROTECTED_BRANCHES.contains(&branch.as_str()) {
        anyhow::bail!("Cannot use protected branch '{branch}'");
    }

    let worktree_path = config.worktree_dir.join(session_id);

    // Ensure the worktrees directory exists
    std::fs::create_dir_all(&config.worktree_dir)?;

    // Check if branch exists
    let branch_exists = Command::new("git")
        .args(["rev-parse", "--verify", &format!("refs/heads/{branch}")])
        .current_dir(&config.main_repo)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if branch_exists {
        // Check if this branch is already checked out in another worktree
        check_branch_not_checked_out(config, &branch)?;

        let status = Command::new("git")
            .args(["worktree", "add", &worktree_path.to_string_lossy(), &branch])
            .current_dir(&config.main_repo)
            .output()?;
        if !status.status.success() {
            let stderr = String::from_utf8_lossy(&status.stderr);
            anyhow::bail!("git worktree add failed: {stderr}");
        }
    } else {
        // Create new branch from tracked_branch
        let status = Command::new("git")
            .args([
                "worktree",
                "add",
                "-b",
                &branch,
                &worktree_path.to_string_lossy(),
                &config.tracked_branch,
            ])
            .current_dir(&config.main_repo)
            .output()?;
        if !status.status.success() {
            let stderr = String::from_utf8_lossy(&status.stderr);
            anyhow::bail!("git worktree add -b failed: {stderr}");
        }
    }

    // Configure user identity in the worktree
    let _ = Command::new("git")
        .args(["config", "user.name", username])
        .current_dir(&worktree_path)
        .status();
    let _ = Command::new("git")
        .args(["config", "user.email", email])
        .current_dir(&worktree_path)
        .status();

    tracing::info!(
        "created worktree for {username} on {branch} at {}",
        worktree_path.display()
    );

    Ok(SessionInfo {
        session_id: session_id.to_string(),
        username: username.to_string(),
        email: email.to_string(),
        branch,
        worktree_path,
    })
}

/// Remove a worktree when a session ends.
pub fn remove_worktree(config: &WorktreeConfig, session_id: &str) -> anyhow::Result<()> {
    let worktree_path = config.worktree_dir.join(session_id);

    if !worktree_path.exists() {
        return Ok(());
    }

    let status = Command::new("git")
        .args([
            "worktree",
            "remove",
            "--force",
            &worktree_path.to_string_lossy(),
        ])
        .current_dir(&config.main_repo)
        .status()?;

    if !status.success() {
        // Fallback: manual cleanup
        tracing::warn!("git worktree remove failed, falling back to rm");
        std::fs::remove_dir_all(&worktree_path)?;
        let _ = Command::new("git")
            .args(["worktree", "prune"])
            .current_dir(&config.main_repo)
            .status();
    }

    tracing::info!("removed worktree: {}", worktree_path.display());
    Ok(())
}

/// Clean up stale worktree entries (e.g., from crashed sessions).
pub fn prune_worktrees(config: &WorktreeConfig) -> anyhow::Result<()> {
    let _ = Command::new("git")
        .args(["worktree", "prune"])
        .current_dir(&config.main_repo)
        .status()?;
    Ok(())
}

/// Build the default branch name for a session (date-stamped).
pub fn default_branch(username: &str) -> String {
    let today = chrono::Local::now().format("%Y-%m-%d");
    format!("{username}/{today}/working")
}

/// Check that a branch is not already checked out in another worktree.
fn check_branch_not_checked_out(config: &WorktreeConfig, branch: &str) -> anyhow::Result<()> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(&config.main_repo)
        .output()?;

    let list = String::from_utf8_lossy(&output.stdout);
    let target = format!("branch refs/heads/{branch}");

    for line in list.lines() {
        if line == target {
            anyhow::bail!(
                "Branch '{branch}' is already checked out in another worktree. \
                 Use a different branch name (e.g., '{branch}_2')."
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// Create a temporary git repo with an initial commit on `main`.
    fn init_test_repo(dir: &std::path::Path) {
        Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(dir)
            .output()
            .expect("git init");
        Command::new("git")
            .args(["config", "user.name", "testuser"])
            .current_dir(dir)
            .output()
            .expect("git config user.name");
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
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
    }

    fn test_config(tmp: &std::path::Path) -> WorktreeConfig {
        let repo = tmp.join("repo");
        let wt = tmp.join("worktrees");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&wt).unwrap();
        init_test_repo(&repo);
        WorktreeConfig {
            main_repo: repo,
            worktree_dir: wt,
            tracked_branch: "main".into(),
        }
    }

    #[test]
    fn default_branch_contains_today() {
        let branch = default_branch("alice");
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        assert!(
            branch.starts_with(&format!("alice/{today}/")),
            "expected 'alice/{today}/...' but got '{branch}'"
        );
        assert!(branch.ends_with("/working"));
    }

    #[test]
    fn create_worktree_default_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let config = test_config(tmp.path());

        let info = create_worktree(&config, "sess-1", "testuser", "test@example.com", None)
            .expect("create_worktree should succeed");

        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        assert_eq!(info.branch, format!("testuser/{today}/working"));
        assert!(info.worktree_path.exists(), "worktree dir should exist");

        // Verify git branch in the worktree
        let output = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(&info.worktree_path)
            .output()
            .unwrap();
        let wt_branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        assert_eq!(wt_branch, info.branch);

        remove_worktree(&config, "sess-1").expect("cleanup");
    }

    #[test]
    fn create_worktree_custom_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let config = test_config(tmp.path());

        let info = create_worktree(
            &config,
            "sess-2",
            "alice",
            "alice@example.com",
            Some("alice/2026-03-18/notes_on_bees"),
        )
        .expect("create_worktree with custom branch");

        assert_eq!(info.branch, "alice/2026-03-18/notes_on_bees");
        assert!(info.worktree_path.exists());

        remove_worktree(&config, "sess-2").expect("cleanup");
    }

    #[test]
    fn rejects_wrong_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let config = test_config(tmp.path());

        let err = create_worktree(
            &config,
            "sess-3",
            "alice",
            "alice@example.com",
            Some("bob/2026-03-18/sneaky"),
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("must be prefixed with 'alice/'"),
            "expected prefix error, got: {err}"
        );
    }

    #[test]
    fn rejects_protected_branch() {
        // Protected branch check happens after prefix validation, so the
        // branch name must pass the prefix check first. In practice a branch
        // like "main" will never pass because no username is "main", but
        // we verify the validation ordering here.
        let tmp = tempfile::tempdir().unwrap();
        let config = test_config(tmp.path());

        // "main" fails on prefix before reaching the protected check
        let err = create_worktree(&config, "sess-4", "alice", "a@e.com", Some("main"))
            .unwrap_err();
        assert!(
            err.to_string().contains("must be prefixed"),
            "expected prefix error for bare 'main', got: {err}"
        );

        // A branch that passes prefix but matches a protected name can't
        // happen with the current naming scheme (protected names have no '/'),
        // so the prefix check is the effective guard.
    }

    #[test]
    fn remove_worktree_cleans_up() {
        let tmp = tempfile::tempdir().unwrap();
        let config = test_config(tmp.path());

        let info = create_worktree(&config, "sess-5", "testuser", "t@e.com", None).unwrap();
        assert!(info.worktree_path.exists());

        remove_worktree(&config, "sess-5").unwrap();
        assert!(!info.worktree_path.exists(), "worktree dir should be gone");
    }

    #[test]
    fn commit_in_worktree_persists_on_reattach() {
        let tmp = tempfile::tempdir().unwrap();
        let config = test_config(tmp.path());

        // Create worktree and commit a file
        let info = create_worktree(&config, "sess-6", "testuser", "t@e.com", None).unwrap();
        let note = info.worktree_path.join("note.md");
        std::fs::write(&note, "# Hello\n").unwrap();
        Command::new("git")
            .args(["add", "note.md"])
            .current_dir(&info.worktree_path)
            .output()
            .unwrap();
        Command::new("git")
            .args([
                "commit",
                "--author",
                "claude-ai <ai@second-brain.local>",
                "-m",
                "ai: create note",
            ])
            .current_dir(&info.worktree_path)
            .output()
            .unwrap();

        let branch = info.branch.clone();
        remove_worktree(&config, "sess-6").unwrap();

        // Re-attach to the same branch
        let info2 =
            create_worktree(&config, "sess-7", "testuser", "t@e.com", Some(&branch)).unwrap();
        assert_eq!(info2.branch, branch);

        // The committed file should still be there
        assert!(
            info2.worktree_path.join("note.md").exists(),
            "committed note should persist after re-attach"
        );

        remove_worktree(&config, "sess-7").unwrap();
    }
}
