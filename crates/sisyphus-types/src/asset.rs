//! Cognitive Asset types for persistent knowledge and experience.

use crate::tools::ToolSpec;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Asset Kind.
pub enum AssetKind {
    Tool,
    SOP,
    Insight,
    Constraint,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Cognitive Asset.
pub struct CognitiveAsset {
    pub id: String,
    pub kind: AssetKind,
    pub name: String,
    pub description: String,
    pub content: AssetContent,
    pub metadata: HashMap<String, String>,
    pub version: u32,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Asset Content.
pub enum AssetContent {
    Tool(ToolSpec),
    SOP(SOPGraphConfig),
    Insight(InsightData),
    Constraint(ConstraintData),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Insight Data.
pub struct InsightData {
    pub observation: String,
    pub reasoning: String,
    pub evidence_ids: Vec<String>,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Constraint Data.
pub struct ConstraintData {
    pub description: String,
    pub strictness: f32, // 0.0 to 1.0
    pub violation_penalty: f32,
}

/// A relationship between two cognitive assets in the Insight Graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetRelation {
    pub from_id: String,
    pub to_id: String,
    pub relation_type: RelationType,
    pub weight: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Relation Type.
pub enum RelationType {
    Solves,
    Requires,
    EvolvedFrom,
    ConflictsWith,
    Refines,
}

/// Re-exporting for AssetContent
use crate::graph::SOPGraphConfig;
