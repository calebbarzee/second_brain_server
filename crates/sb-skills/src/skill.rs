//! Core skill trait, registry, and output types.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

/// What a skill is allowed to do to notes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PermissionLevel {
    /// Query DB, gather context, return analysis
    ReadOnly,
    /// Create new notes but never modify existing
    WriteNew,
    /// Modify or reorganize existing notes
    Destructive,
}

/// Parameters passed to a skill execution.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct SkillParams {
    /// Time period (e.g., "today", "this-week", "2026-03-04")
    pub period: Option<String>,
    /// Project name to scope the skill to
    pub project: Option<String>,
    /// Preview mode — return changeset without applying
    pub dry_run: bool,
    /// Allow destructive skills to write
    pub allow_writes: bool,
    /// Write output as a new note (for WriteNew skills)
    pub write_output: bool,
}

/// Output produced by a skill.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct SkillOutput {
    /// Human-readable summary of what the skill found/did
    pub summary: String,
    /// Structured context as JSON (for Claude to reason over)
    pub context: Option<serde_json::Value>,
    /// A deferred prompt for the MCP client (Claude) to process
    pub deferred_prompt: Option<String>,
    /// Notes created by WriteNew skills
    pub notes_created: Vec<String>,
    /// Notes modified by Destructive skills
    pub notes_modified: Vec<String>,
    /// Git diff of changes (for Destructive skills)
    pub git_diff: Option<String>,
    /// Preview changeset (for dry-run mode)
    pub changeset: Option<serde_json::Value>,
}

/// The trait all skills implement.
#[async_trait]
pub trait Skill: Send + Sync {
    /// Machine name (e.g., "summarize", "contextualize")
    fn name(&self) -> &str;
    /// Human description
    fn description(&self) -> &str;
    /// What this skill is allowed to do
    fn permission_level(&self) -> PermissionLevel;
    /// Execute the skill with given context and params.
    async fn execute(
        &self,
        ctx: &crate::context::SkillContext,
        params: &SkillParams,
    ) -> anyhow::Result<SkillOutput>;
}

/// Registry of available skills.
pub struct SkillRegistry {
    skills: HashMap<String, Arc<dyn Skill>>,
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self {
            skills: HashMap::new(),
        }
    }

    /// Create a registry with all built-in skills.
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();
        registry.register(Arc::new(crate::skills::SummarizeSkill));
        registry.register(Arc::new(crate::skills::ContinueWorkSkill));
        registry.register(Arc::new(crate::skills::ReflectSkill));
        registry.register(Arc::new(crate::skills::ConnectIdeasSkill));
        registry.register(Arc::new(crate::skills::ContextualizeSkill));
        registry
    }

    pub fn register(&mut self, skill: Arc<dyn Skill>) {
        self.skills.insert(skill.name().to_string(), skill);
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn Skill>> {
        self.skills.get(name)
    }

    pub fn list(&self) -> Vec<SkillInfo> {
        self.list_info()
    }

    /// List skills as serializable info.
    pub fn list_info(&self) -> Vec<SkillInfo> {
        let mut infos: Vec<SkillInfo> = self
            .skills
            .values()
            .map(|s| SkillInfo {
                name: s.name().to_string(),
                description: s.description().to_string(),
                permission_level: s.permission_level(),
            })
            .collect();
        infos.sort_by(|a, b| a.name.cmp(&b.name));
        infos
    }
}

#[derive(Debug, serde::Serialize)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub permission_level: PermissionLevel,
}
