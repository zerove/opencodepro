//! Shared types for the Skill system.
//!
//! Defines Skill, SkillSource, TriggerDef, and the internal SkillMeta.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A loadable skill definition, corresponding to a SKILL.md file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    /// Unique name (from filename or frontmatter).
    pub name: String,
    /// Short description.
    pub description: String,
    /// Version string.
    #[serde(default = "default_version")]
    pub version: String,
    /// Author.
    #[serde(default)]
    pub author: Option<String>,
    /// Tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Trigger conditions.
    #[serde(default)]
    pub triggers: Vec<TriggerDef>,
    /// Full instruction body (after frontmatter).
    #[serde(skip)]
    pub instructions: String,
    /// Source classification.
    #[serde(skip)]
    pub source: SkillSource,
    /// Source file path.
    #[serde(skip)]
    pub source_path: Option<PathBuf>,
}

fn default_version() -> String {
    "0.1.0".into()
}

/// Source hierarchy for a skill.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillSource {
    /// Built-in skill (shipped with release).
    #[default]
    BuiltIn,
    /// User-global skill (~/.config/sisyphus/skills/).
    User,
    /// Project-local skill (.sisyphus/skills/).
    Project,
}

impl SkillSource {
    pub fn priority(&self) -> u8 {
        match self {
            SkillSource::Project => 30,
            SkillSource::User => 20,
            SkillSource::BuiltIn => 10,
        }
    }
}

/// A trigger definition, corresponding to triggers in SKILL.md frontmatter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerDef {
    /// Trigger keyword/regex pattern.
    pub pattern: Option<String>,
    /// Associated tool names (empty means all).
    #[serde(default)]
    pub tools: Vec<String>,
    /// Trigger description.
    pub description: Option<String>,
    /// Hook phase: "before" or "after".
    #[serde(default)]
    pub hook_phase: Option<String>,
    /// Hook effect: "deny", "inject", "log".
    #[serde(default)]
    pub hook_effect: Option<String>,
    /// Hook message/payload.
    #[serde(default)]
    pub hook_message: Option<String>,
}

/// SKILL.md frontmatter structure (internal, not publicly exported).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SkillMeta {
    pub name: Option<String>,
    pub description: Option<String>,
    #[serde(default = "default_version")]
    pub version: String,
    pub author: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub triggers: Vec<TriggerDef>,
}

impl Default for SkillMeta {
    fn default() -> Self {
        SkillMeta {
            name: None,
            description: None,
            version: "0.1.0".into(),
            author: None,
            tags: Vec::new(),
            triggers: Vec::new(),
        }
    }
}
