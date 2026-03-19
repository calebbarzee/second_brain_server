//! Connect-ideas skill: find cross-project semantic connections.
//!
//! For recent notes across different projects, computes cross-project
//! semantic similarity and surfaces pairs with high similarity but no
//! existing link.

use crate::context::SkillContext;
use crate::skill::{PermissionLevel, Skill, SkillOutput, SkillParams};
use crate::time_period;
use async_trait::async_trait;
use sb_core::db::{embeddings, links};

pub struct ConnectIdeasSkill;

#[async_trait]
impl Skill for ConnectIdeasSkill {
    fn name(&self) -> &str {
        "connect-ideas"
    }

    fn description(&self) -> &str {
        "Find cross-project semantic connections: notes that are conceptually related but not linked"
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(
        &self,
        ctx: &SkillContext,
        params: &SkillParams,
    ) -> anyhow::Result<SkillOutput> {
        let period = time_period::parse_period(params.period.as_deref().unwrap_or("this-month"))?;

        // Get recent notes
        let notes = ctx
            .get_notes_in_range(period.start, period.end, None)
            .await?;

        let mut connections = Vec::new();

        // For each note, find related notes and check if they're already linked
        for note in &notes {
            let related = embeddings::find_related_notes(ctx.db.pool(), note.id, 5).await?;

            // Get existing outbound links for this note
            let existing_links = links::get_links_from_note(ctx.db.pool(), note.id).await?;
            let linked_note_ids: std::collections::HashSet<_> = existing_links
                .iter()
                .filter_map(|l| l.target_note_id)
                .collect();

            // Deduplicate by note
            let mut seen = std::collections::HashSet::new();
            for result in &related {
                if !seen.insert(result.note_id) {
                    continue;
                }

                // Skip if already linked
                if linked_note_ids.contains(&result.note_id) {
                    continue;
                }

                // Skip low similarity
                if result.similarity < 0.5 {
                    continue;
                }

                // Check if they're in different "contexts" (different directories)
                let same_dir = std::path::Path::new(&note.file_path).parent()
                    == std::path::Path::new(&result.note_file_path).parent();
                let cross_context = !same_dir;

                connections.push(serde_json::json!({
                    "note_a": {
                        "title": note.title,
                        "path": note.file_path,
                        "project": note.source_project,
                    },
                    "note_b": {
                        "title": result.note_title,
                        "path": result.note_file_path,
                    },
                    "similarity": format!("{:.3}", result.similarity),
                    "cross_context": cross_context,
                    "preview": truncate(&result.chunk_content, 200),
                }));
            }
        }

        // Sort by similarity (highest first)
        connections.sort_by(|a, b| {
            let sim_a: f64 = a["similarity"]
                .as_str()
                .unwrap_or("0")
                .parse()
                .unwrap_or(0.0);
            let sim_b: f64 = b["similarity"]
                .as_str()
                .unwrap_or("0")
                .parse()
                .unwrap_or(0.0);
            sim_b
                .partial_cmp(&sim_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Limit to top connections
        connections.truncate(20);

        let context = serde_json::json!({
            "period": period.label,
            "notes_analyzed": notes.len(),
            "connections_found": connections.len(),
            "connections": connections,
        });

        let deferred = format!(
            "Here are potential conceptual connections found across your knowledge base:\n\n{}\n\n\
             Please:\n\
             1. Identify the most interesting/useful connections\n\
             2. Suggest which notes should be linked together\n\
             3. Highlight any surprising cross-project patterns\n\
             4. Note any connections that suggest consolidation opportunities",
            serde_json::to_string_pretty(&context)?,
        );

        Ok(SkillOutput {
            summary: format!(
                "Found {} potential connections across {} notes",
                connections.len(),
                notes.len(),
            ),
            context: Some(context),
            deferred_prompt: Some(deferred),
            ..Default::default()
        })
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}
