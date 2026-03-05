//! Contextualize skill: auto-detect projects, suggest tags, suggest links.
//!
//! Destructive permission level — can modify notes (add cross-refs, tags).
//! Always runs in preview mode first; changes only applied with allow_writes.

use crate::context::SkillContext;
use crate::skill::{PermissionLevel, Skill, SkillOutput, SkillParams};
use crate::time_period;
use async_trait::async_trait;
use sb_core::db::{embeddings, notes, projects, tags};
use sb_core::lifecycle;
use sb_core::project_detect;

pub struct ContextualizeSkill;

#[async_trait]
impl Skill for ContextualizeSkill {
    fn name(&self) -> &str {
        "contextualize"
    }

    fn description(&self) -> &str {
        "Auto-detect projects, suggest tags and links, classify lifecycle. Preview mode by default; use --allow-writes to apply changes."
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Destructive
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

        // Get notes to contextualize
        let target_notes = ctx
            .get_notes_in_range(period.start, period.end, project.as_ref().map(|p| p.id))
            .await?;

        let mut proposed_changes = Vec::new();
        let mut modified_paths = Vec::new();

        for note in &target_notes {
            // Skip enduring notes (never auto-modify)
            if note.lifecycle == "enduring" && !params.allow_writes {
                continue;
            }

            let mut changes = serde_json::Map::new();
            changes.insert(
                "note".to_string(),
                serde_json::json!({
                    "title": note.title,
                    "path": note.file_path,
                    "current_lifecycle": note.lifecycle,
                }),
            );

            // 1. Project detection (pass known project names for fuzzy matching)
            let known_project_names: Vec<String> = projects::list_projects_with_counts(ctx.db.pool())
                .await
                .unwrap_or_default()
                .iter()
                .map(|p| p.project_name.clone())
                .collect();
            let detected = project_detect::detect_project(&note.file_path, &[], &known_project_names);
            if let Some(det) = &detected {
                if note.source_project.is_none() {
                    changes.insert(
                        "project_suggestion".to_string(),
                        serde_json::json!({
                            "name": det.name,
                            "confidence": format!("{:?}", det.confidence),
                        }),
                    );

                    // Apply if not dry_run
                    if !params.dry_run {
                        let proj = projects::upsert_project(
                            ctx.db.pool(),
                            &det.name,
                            &note.file_path,
                            None,
                        )
                        .await?;
                        projects::associate_note_project(ctx.db.pool(), note.id, proj.id)
                            .await?;
                    }
                }
            }

            // 2. Lifecycle classification
            let suggested_lifecycle =
                lifecycle::classify_note(&note.file_path, note.frontmatter.as_ref());
            if suggested_lifecycle.as_str() != note.lifecycle {
                changes.insert(
                    "lifecycle_suggestion".to_string(),
                    serde_json::json!({
                        "from": note.lifecycle,
                        "to": suggested_lifecycle.as_str(),
                    }),
                );

                if !params.dry_run {
                    notes::update_lifecycle(
                        ctx.db.pool(),
                        note.id,
                        suggested_lifecycle.as_str(),
                    )
                    .await?;
                }
            }

            // 3. Find unlinked related notes (potential link suggestions)
            let related =
                embeddings::find_related_notes(ctx.db.pool(), note.id, 5).await?;
            let link_suggestions: Vec<_> = related
                .iter()
                .filter(|r| r.similarity > 0.6)
                .map(|r| {
                    serde_json::json!({
                        "title": r.note_title,
                        "path": r.note_file_path,
                        "similarity": format!("{:.3}", r.similarity),
                    })
                })
                .collect();

            if !link_suggestions.is_empty() {
                changes.insert(
                    "link_suggestions".to_string(),
                    serde_json::Value::Array(link_suggestions),
                );
            }

            // 4. Tag suggestions (find tags from similar notes)
            let tag_suggestions = suggest_tags_from_similar(ctx, note.id).await;
            if !tag_suggestions.is_empty() {
                changes.insert(
                    "tag_suggestions".to_string(),
                    serde_json::json!(tag_suggestions),
                );

                if !params.dry_run {
                    for tag_name in &tag_suggestions {
                        let tag = tags::upsert_tag(ctx.db.pool(), tag_name).await?;
                        tags::tag_note(ctx.db.pool(), note.id, tag.id).await?;
                    }
                }
            }

            if changes.len() > 1 {
                // More than just the "note" field
                proposed_changes.push(serde_json::Value::Object(changes));
                if !params.dry_run {
                    modified_paths.push(note.file_path.clone());
                }
            }
        }

        let changeset = serde_json::json!({
            "notes_analyzed": target_notes.len(),
            "changes_proposed": proposed_changes.len(),
            "changes": proposed_changes,
        });

        let summary = if params.dry_run {
            format!(
                "Preview: analyzed {} notes, proposed {} changes (use --allow-writes to apply)",
                target_notes.len(),
                proposed_changes.len(),
            )
        } else {
            format!(
                "Contextualized {} notes: {} changes applied",
                target_notes.len(),
                proposed_changes.len(),
            )
        };

        Ok(SkillOutput {
            summary,
            context: Some(changeset.clone()),
            changeset: if params.dry_run {
                Some(changeset)
            } else {
                None
            },
            notes_modified: modified_paths,
            ..Default::default()
        })
    }
}

/// Find tags used on semantically similar notes.
async fn suggest_tags_from_similar(
    ctx: &SkillContext,
    note_id: uuid::Uuid,
) -> Vec<String> {
    let related = embeddings::find_related_notes(ctx.db.pool(), note_id, 5)
        .await
        .unwrap_or_default();

    let mut tag_counts: std::collections::HashMap<String, u32> =
        std::collections::HashMap::new();

    for result in &related {
        if let Ok(note_tags) = tags::get_tags_for_note(ctx.db.pool(), result.note_id).await {
            for tag in note_tags {
                *tag_counts.entry(tag.name).or_insert(0) += 1;
            }
        }
    }

    // Get current note's tags to exclude
    let current_tags = tags::get_tags_for_note(ctx.db.pool(), note_id)
        .await
        .unwrap_or_default();
    let current_tag_names: std::collections::HashSet<_> =
        current_tags.iter().map(|t| t.name.clone()).collect();

    // Suggest tags that appear on 2+ similar notes but not on current note
    let mut suggestions: Vec<_> = tag_counts
        .into_iter()
        .filter(|(name, count)| *count >= 2 && !current_tag_names.contains(name))
        .map(|(name, _)| name)
        .collect();
    suggestions.sort();
    suggestions
}
