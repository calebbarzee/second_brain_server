//! Summarize skill: activity summary for a time period.
//!
//! Parses period → fetches notes in range → groups by project → extracts TODOs
//! → builds structured activity summary. ReadOnly by default; WriteNew with --write-output.

use crate::context::SkillContext;
use crate::skill::{PermissionLevel, Skill, SkillOutput, SkillParams};
use crate::time_period;
use async_trait::async_trait;
use sb_core::markdown;

pub struct SummarizeSkill;

#[async_trait]
impl Skill for SummarizeSkill {
    fn name(&self) -> &str {
        "summarize"
    }

    fn description(&self) -> &str {
        "Summarize activity for a time period: notes created/modified, tasks completed/open, grouped by project"
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(
        &self,
        ctx: &SkillContext,
        params: &SkillParams,
    ) -> anyhow::Result<SkillOutput> {
        let period = time_period::parse_period(params.period.as_deref().unwrap_or("this-week"))?;

        // Resolve project filter
        let project = if let Some(proj_name) = &params.project {
            ctx.resolve_project(proj_name).await?
        } else {
            None
        };
        let project_id = project.as_ref().map(|p| p.id);

        // Fetch notes in range
        let notes = ctx
            .get_notes_in_range(period.start, period.end, project_id)
            .await?;

        // Extract tasks from all notes
        let mut all_open_tasks = Vec::new();
        let mut all_completed_tasks = Vec::new();
        let mut project_groups: std::collections::HashMap<String, Vec<serde_json::Value>> =
            std::collections::HashMap::new();

        for note in &notes {
            let parsed = markdown::parse_markdown(&note.raw_content);

            for task in &parsed.tasks {
                let task_info = serde_json::json!({
                    "title": task.title,
                    "source_note": note.title,
                    "source_path": note.file_path,
                });
                if task.completed {
                    all_completed_tasks.push(task_info);
                } else {
                    all_open_tasks.push(task_info);
                }
            }

            // Group notes by project (use source_project or "unassigned")
            let group = note
                .source_project
                .clone()
                .unwrap_or_else(|| "unassigned".to_string());
            project_groups
                .entry(group)
                .or_default()
                .push(serde_json::json!({
                    "title": note.title,
                    "file_path": note.file_path,
                    "lifecycle": note.lifecycle,
                    "updated_at": note.updated_at.to_rfc3339(),
                }));
        }

        // Also fetch DB tasks
        let db_open_tasks = ctx.get_open_tasks(project_id).await.unwrap_or_default();

        let context = serde_json::json!({
            "period": {
                "label": period.label,
                "start": period.start.to_rfc3339(),
                "end": period.end.to_rfc3339(),
            },
            "stats": {
                "notes_count": notes.len(),
                "open_tasks": all_open_tasks.len() + db_open_tasks.len(),
                "completed_tasks": all_completed_tasks.len(),
                "projects": project_groups.keys().collect::<Vec<_>>(),
            },
            "notes_by_project": project_groups,
            "open_tasks_from_notes": all_open_tasks,
            "completed_tasks_from_notes": all_completed_tasks,
            "open_tasks_from_db": db_open_tasks.iter().map(|t| serde_json::json!({
                "title": t.title,
                "status": t.status,
                "created_at": t.created_at.to_rfc3339(),
            })).collect::<Vec<_>>(),
        });

        let summary = format!(
            "Activity summary for {}: {} notes, {} open tasks, {} completed tasks across {} projects",
            period.label,
            notes.len(),
            all_open_tasks.len() + db_open_tasks.len(),
            all_completed_tasks.len(),
            project_groups.len(),
        );

        // Build a deferred prompt for Claude to reason over
        let deferred = format!(
            "You are analyzing a knowledge base activity summary. Here is the structured context:\n\n\
             {}\n\n\
             Please provide:\n\
             1. A concise narrative summary of what was worked on during {}\n\
             2. Key accomplishments (completed tasks)\n\
             3. Outstanding items that need attention (open tasks)\n\
             4. Any patterns or observations across projects",
            serde_json::to_string_pretty(&context)?,
            period.label,
        );

        let mut output = SkillOutput {
            summary,
            context: Some(context),
            deferred_prompt: Some(deferred),
            ..Default::default()
        };

        // If write_output is set, create a summary note
        if params.write_output {
            let date = chrono::Utc::now().format("%Y-%m-%d");
            let note_path = ctx.notes_root.join(format!("summaries/{date}_summary.md"));

            let content = format!(
                "---\nlifecycle: volatile\ntype: summary\nperiod: {}\n---\n\n# Summary: {}\n\n_{} notes, {} open tasks, {} completed tasks_\n\n\
                 _Generated by second-brain summarize skill_\n",
                period.label,
                period.label,
                notes.len(),
                all_open_tasks.len(),
                all_completed_tasks.len(),
            );

            if let Some(parent) = note_path.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            std::fs::write(&note_path, &content)?;

            // Ingest the new note
            let mapper = sb_core::PathMapper::new(ctx.notes_root.clone());
            sb_core::ingest::ingest_file(&ctx.db, &note_path, &mapper)
                .await
                .ok();

            output
                .notes_created
                .push(note_path.to_string_lossy().to_string());
        }

        Ok(output)
    }
}
