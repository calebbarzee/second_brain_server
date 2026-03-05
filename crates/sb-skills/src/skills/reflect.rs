//! Reflect skill: compare planned vs completed, detect patterns, find stale items.
//!
//! ReadOnly by default. WriteNew with --write-output (creates a reflection note).

use crate::context::SkillContext;
use crate::skill::{PermissionLevel, Skill, SkillOutput, SkillParams};
use crate::time_period;
use async_trait::async_trait;
use sb_core::db::{notes, skill_runs};
use sb_core::markdown;

pub struct ReflectSkill;

#[async_trait]
impl Skill for ReflectSkill {
    fn name(&self) -> &str {
        "reflect"
    }

    fn description(&self) -> &str {
        "Reflect on a period: compare planned vs completed, detect recurring TODOs, find orphaned/stale notes"
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(
        &self,
        ctx: &SkillContext,
        params: &SkillParams,
    ) -> anyhow::Result<SkillOutput> {
        let period = time_period::parse_period(
            params.period.as_deref().unwrap_or("this-week"),
        )?;

        let project = if let Some(proj_name) = &params.project {
            ctx.resolve_project(proj_name).await?
        } else {
            None
        };
        let project_id = project.as_ref().map(|p| p.id);

        // Fetch notes in range
        let period_notes = ctx
            .get_notes_in_range(period.start, period.end, project_id)
            .await?;

        // Collect all tasks from period notes
        let mut open_tasks = Vec::new();
        let mut completed_tasks = Vec::new();
        let mut task_titles: std::collections::HashMap<String, u32> =
            std::collections::HashMap::new();

        for note in &period_notes {
            let parsed = markdown::parse_markdown(&note.raw_content);
            for task in &parsed.tasks {
                *task_titles
                    .entry(task.title.to_lowercase())
                    .or_insert(0) += 1;
                let task_info = serde_json::json!({
                    "title": task.title,
                    "source": note.title,
                    "source_path": note.file_path,
                });
                if task.completed {
                    completed_tasks.push(task_info);
                } else {
                    open_tasks.push(task_info);
                }
            }
        }

        // Detect recurring tasks (same task title across multiple notes)
        let recurring: Vec<_> = task_titles
            .iter()
            .filter(|(_, count)| **count > 1)
            .map(|(title, count)| {
                serde_json::json!({"title": title, "occurrences": count})
            })
            .collect();

        // Find stale volatile notes (volatile notes not updated recently)
        let stale_volatile = notes::get_notes_by_lifecycle(
            ctx.db.pool(),
            "volatile",
            project_id,
            50,
        )
        .await?;
        let stale_threshold = chrono::Utc::now() - chrono::Duration::days(30);
        let stale_notes: Vec<_> = stale_volatile
            .iter()
            .filter(|n| n.updated_at < stale_threshold)
            .map(|n| {
                serde_json::json!({
                    "title": n.title,
                    "file_path": n.file_path,
                    "last_updated": n.updated_at.to_rfc3339(),
                    "days_stale": (chrono::Utc::now() - n.updated_at).num_days(),
                })
            })
            .collect();

        // Get past reflections for comparison
        let past_runs = skill_runs::list_skill_runs(ctx.db.pool(), Some("reflect"), 5).await?;

        let context = serde_json::json!({
            "period": {
                "label": period.label,
                "start": period.start.to_rfc3339(),
                "end": period.end.to_rfc3339(),
            },
            "stats": {
                "notes_in_period": period_notes.len(),
                "tasks_completed": completed_tasks.len(),
                "tasks_open": open_tasks.len(),
                "recurring_tasks": recurring.len(),
                "stale_volatile_notes": stale_notes.len(),
            },
            "completed_tasks": completed_tasks,
            "open_tasks": open_tasks,
            "recurring_tasks": recurring,
            "stale_volatile_notes": stale_notes,
            "past_reflections": past_runs.iter().map(|r| serde_json::json!({
                "date": r.started_at.to_rfc3339(),
                "summary": r.output_summary,
            })).collect::<Vec<_>>(),
        });

        let deferred = format!(
            "You are performing a reflection on knowledge work for {}. Here is the context:\n\n{}\n\n\
             Please provide:\n\
             1. What was accomplished vs what was planned\n\
             2. Recurring patterns (tasks that keep appearing but don't get done)\n\
             3. Stale items that need attention or should be archived\n\
             4. Suggestions for improving workflow or focus",
            period.label,
            serde_json::to_string_pretty(&context)?,
        );

        let summary = format!(
            "Reflection for {}: {} notes, {} completed / {} open tasks, {} recurring, {} stale",
            period.label,
            period_notes.len(),
            completed_tasks.len(),
            open_tasks.len(),
            recurring.len(),
            stale_notes.len(),
        );

        let mut output = SkillOutput {
            summary,
            context: Some(context),
            deferred_prompt: Some(deferred),
            ..Default::default()
        };

        if params.write_output {
            let date = chrono::Utc::now().format("%Y-%m-%d");
            let note_path = ctx
                .notes_root
                .join(format!("reflections/{date}_reflection.md"));

            let content = format!(
                "---\nlifecycle: volatile\ntype: reflection\nperiod: {}\n---\n\n\
                 # Reflection: {}\n\n\
                 - {} notes reviewed\n\
                 - {} tasks completed, {} open\n\
                 - {} recurring patterns detected\n\
                 - {} stale volatile notes\n\n\
                 _Generated by second-brain reflect skill_\n",
                period.label,
                period.label,
                period_notes.len(),
                completed_tasks.len(),
                open_tasks.len(),
                recurring.len(),
                stale_notes.len(),
            );

            if let Some(parent) = note_path.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            std::fs::write(&note_path, &content)?;
            sb_core::ingest::ingest_file(&ctx.db, &note_path).await.ok();

            output
                .notes_created
                .push(note_path.to_string_lossy().to_string());
        }

        Ok(output)
    }
}
