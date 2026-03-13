//! Git worktree management for multi-user session isolation.
//!
//! Each MCP session gets its own git worktree so multiple users can
//! concurrently edit notes on separate branches without conflicts.

use std::path::PathBuf;
use std::process::Command;

/// Branches that must never be committed to directly.
const PROTECTED_BRANCHES: &[&str] = &["main", "master", "staging", "dev"];

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
        None => format!("{username}/working"),
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
            .args([
                "worktree",
                "add",
                &worktree_path.to_string_lossy(),
                &branch,
            ])
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
