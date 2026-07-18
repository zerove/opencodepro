//! Skill system definitions for Sisyphus.
//! Defines how cognitive skills (SOPs + Tools) are packaged and distributed.

use crate::graph::SOPGraphConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Metadata for a Sisyphus skill.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SkillMetadata {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub tags: Vec<String>,
    pub created_at: u64,
}

/// A complete package representing a cognitive skill.
/// This bundle can be serialized to JSON/TOML for distribution.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SkillBundle {
    pub metadata: SkillMetadata,
    pub main_sop: SOPGraphConfig,
    /// Optional nested SOPs used by the main SOP.
    pub sub_sops: HashMap<String, SOPGraphConfig>,
    /// List of tool IDs that must be available to execute this skill.
    pub required_tools: Vec<String>,
    /// Human-readable or machine-checkable criteria for skill success.
    pub acceptance_criteria: String,
}

impl SkillBundle {
    /// Create a new SkillBundle from a metadata and a main SOP.
    pub fn new(metadata: SkillMetadata, main_sop: SOPGraphConfig) -> Self {
        Self {
            metadata,
            main_sop,
            sub_sops: HashMap::new(),
            required_tools: Vec::new(),
            acceptance_criteria: String::new(),
        }
    }

    /// Add a sub-SOP to the bundle.
    pub fn add_sub_sop(&mut self, id: String, config: SOPGraphConfig) {
        self.sub_sops.insert(id, config);
    }

    /// Add a required tool to the bundle.
    pub fn add_required_tool(&mut self, tool_id: String) {
        if !self.required_tools.contains(&tool_id) {
            self.required_tools.push(tool_id);
        }
    }
}
