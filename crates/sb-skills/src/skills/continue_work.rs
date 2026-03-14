//! Continue-work skill: gather context for resuming work on a project.
//!
//! Fetches recent project notes, open tasks, and semantically related threads.
//! Always returns a deferred_prompt for Claude to reason over.

use crate::context::SkillContext;
use crate::skill::{PermissionLevel, Skill, SkillOutput, SkillParams};
use async_trait::async_trait;
use sb_core::markdown;

pub struct ContinueWorkSkill;

#[async_trait]
impl Skill for ContinueWorkSkill {
    fn name(&self) -> &str {
        "continue-work"
    }

    fn description(&self) -> &str {
        "Gather context for resuming work on a project: recent notes, open tasks, related threads"
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(
        &self,
        ctx: &SkillContext,
        params: &SkillParams,
    ) -> anyhow::Result<SkillOutput> {
        let project_name = params
            .project
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("continue-work requires a --project parameter"))?;

        let project = ctx
            .resolve_project(project_name)
            .await?
            .ok_or_else(|| anyhow::anyhow!("project not found: {project_name}"))?;

        // Get recent project notes
        let recent_notes = ctx.get_project_notes(project.id, 20).await?;

        // Get open tasks
        let open_tasks = ctx.get_open_tasks(Some(project.id)).await?;

        // Extract inline tasks from recent notes
        let mut inline_tasks = Vec::new();
        for note in &recent_notes {
            let parsed = markdown::parse_markdown(&note.raw_content);
            for task in parsed.tasks.iter().filter(|t| !t.completed) {
                inline_tasks.push(serde_json::json!({
                    "title": task.title,
                    "source_note": note.title,
                    "source_path": note.file_path,
                }));
            }
        }

        // Find semantically related content across KB
        let search_query = format!("{project_name} current work progress status");
        let related = ctx
            .semantic_search(&search_query, 10)
            .await
            .unwrap_or_default();

        let context = serde_json::json!({
            "project": {
                "name": project.name,
                "root_path": project.root_path,
                "description": project.description,
            },
            "recent_notes": recent_notes.iter().map(|n| serde_json::json!({
                "title": n.title,
                "file_path": n.file_path,
                "lifecycle": n.lifecycle,
                "updated_at": n.updated_at.to_rfc3339(),
                "preview": truncate_content(&n.raw_content, 500),
            })).collect::<Vec<_>>(),
            "open_tasks_db": open_tasks.iter().map(|t| serde_json::json!({
                "title": t.title,
                "status": t.status,
            })).collect::<Vec<_>>(),
            "open_tasks_inline": inline_tasks,
            "related_across_kb": related.iter().map(|r| serde_json::json!({
                "note_title": r.note_title,
                "note_path": r.note_file_path,
                "section": r.heading_context,
                "similarity": format!("{:.3}", r.similarity),
                "preview": truncate_content(&r.chunk_content, 200),
            })).collect::<Vec<_>>(),
        });

        let deferred = format!(
            "You are helping resume work on the '{}' project. Here is the current context:\n\n{}\n\n\
             Based on this context, please:\n\
             1. Summarize where things left off\n\
             2. List the most important open tasks/items\n\
             3. Suggest what to work on next and why\n\
             4. Flag any stale items or blockers",
            project_name,
            serde_json::to_string_pretty(&context)?,
        );

        Ok(SkillOutput {
            summary: format!(
                "Context for '{}': {} recent notes, {} open tasks, {} related items",
                project_name,
                recent_notes.len(),
                open_tasks.len() + inline_tasks.len(),
                related.len(),
            ),
            context: Some(context),
            deferred_prompt: Some(deferred),
            ..Default::default()
        })
    }
}

fn truncate_content(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}
