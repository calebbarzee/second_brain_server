//! SkillRunner: looks up skills, checks permissions, manages git safety.

use crate::context::SkillContext;
use crate::git_ops;
use crate::skill::{PermissionLevel, SkillOutput, SkillParams, SkillRegistry};
use sb_core::db::skill_runs;
use std::sync::Arc;

/// Orchestrates skill execution with permission checks and git safety.
pub struct SkillRunner {
    registry: SkillRegistry,
    ctx: Arc<SkillContext>,
}

impl SkillRunner {
    pub fn new(registry: SkillRegistry, ctx: Arc<SkillContext>) -> Self {
        Self { registry, ctx }
    }

    /// Run a skill by name.
    pub async fn run(&self, skill_name: &str, params: &SkillParams) -> anyhow::Result<SkillOutput> {
        let skill = self
            .registry
            .get(skill_name)
            .ok_or_else(|| anyhow::anyhow!("unknown skill: {skill_name}"))?
            .clone();

        let perm = skill.permission_level();

        // Check permission level
        if perm == PermissionLevel::Destructive && !params.allow_writes {
            // Run in preview mode (dry_run)
            let mut preview_params = params.clone();
            preview_params.dry_run = true;

            let run = skill_runs::create_skill_run(
                self.ctx.db.pool(),
                skill_name,
                Some(&serde_json::to_value(params)?),
            )
            .await?;

            let output = skill.execute(&self.ctx, &preview_params).await?;

            skill_runs::complete_skill_run(
                self.ctx.db.pool(),
                run.id,
                "preview",
                Some(&output.summary),
            )
            .await?;

            return Ok(output);
        }

        // Record the skill run
        let run = skill_runs::create_skill_run(
            self.ctx.db.pool(),
            skill_name,
            Some(&serde_json::to_value(params)?),
        )
        .await?;

        // For destructive skills: create a pre-snapshot
        let pre_commit = if perm == PermissionLevel::Destructive {
            git_ops::snapshot_commit(
                &self.ctx.notes_root,
                &format!("[second-brain] pre-{skill_name} snapshot"),
            )
            .ok()
            .flatten()
        } else {
            None
        };

        // Execute the skill
        let mut output = match skill.execute(&self.ctx, params).await {
            Ok(out) => out,
            Err(e) => {
                skill_runs::complete_skill_run(
                    self.ctx.db.pool(),
                    run.id,
                    "failed",
                    Some(&e.to_string()),
                )
                .await
                .ok();
                return Err(e);
            }
        };

        // For destructive skills: create a post-snapshot and capture diff
        if perm == PermissionLevel::Destructive && params.allow_writes {
            let post_msg = format!(
                "[second-brain] {skill_name}: {}",
                summarize_changes(&output)
            );
            if let Ok(Some(_post_sha)) =
                git_ops::snapshot_commit(&self.ctx.notes_root, &post_msg)
            {
                if let Some(pre_sha) = &pre_commit {
                    if let Ok(diff) = git_ops::diff_since(&self.ctx.notes_root, pre_sha) {
                        output.git_diff = Some(diff);
                    }
                }
            }
        }

        // Record completion
        let status = if output.notes_modified.is_empty() && output.notes_created.is_empty() {
            "completed_readonly"
        } else {
            "completed"
        };

        skill_runs::complete_skill_run(
            self.ctx.db.pool(),
            run.id,
            status,
            Some(&output.summary),
        )
        .await?;

        Ok(output)
    }

    /// List available skills.
    pub fn list_skills(&self) -> Vec<crate::skill::SkillInfo> {
        self.registry.list_info()
    }
}

fn summarize_changes(output: &SkillOutput) -> String {
    let mut parts = Vec::new();
    if !output.notes_created.is_empty() {
        parts.push(format!("created {} notes", output.notes_created.len()));
    }
    if !output.notes_modified.is_empty() {
        parts.push(format!("modified {} notes", output.notes_modified.len()));
    }
    if parts.is_empty() {
        "no changes".to_string()
    } else {
        parts.join(", ")
    }
}
