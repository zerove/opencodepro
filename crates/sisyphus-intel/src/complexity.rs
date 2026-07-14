//! Task complexity analysis for dynamic cognitive depth adjustment.

use std::sync::Arc;
use sisyphus_types::{SOPGraph, experience::ExperienceBank};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CognitiveDepth {
    Low,
    Medium,
    High,
    Ultra,
}

impl CognitiveDepth {
    pub fn to_config(&self) -> (u32, f64) {
        match self {
            CognitiveDepth::Low => (3, 0.2),
            CognitiveDepth::Medium => (10, 0.05),
            CognitiveDepth::High => (20, 0.01),
            CognitiveDepth::Ultra => (30, 0.005),
        }
    }

    pub fn should_simulate(&self) -> bool {
        match self {
            CognitiveDepth::Low => false,
            _ => true,
        }
    }
}

pub struct ComplexityAnalyzer;

impl ComplexityAnalyzer {
    /// Analyzes the complexity of a goal and its current SOP representation.
    pub async fn analyze(
        experience_bank: Arc<dyn ExperienceBank>,
        sop_graph: &SOPGraph,
        goal_text: &str,
    ) -> CognitiveDepth {
        // 1. Structural Complexity (0.0 to 1.0)
        let node_count = sop_graph.graph.node_count();
        let structural_score = if node_count <= 3 {
            0.1
        } else if node_count <= 10 {
            0.4
        } else if node_count <= 25 {
            0.7
        } else {
            1.0
        };

        // 2. Semantic Rarity (0.0 to 1.0)
        // If we have many successful experiences for this goal, it's a "Low" complexity task.
        let mut rarity_score = 0.5;
        let query = sisyphus_types::experience::ExperienceQuery {
            query_text: goal_text.to_string(),
            context_hash: String::new(),
            project_id: None,
            action_id: None,
            visual_embedding: None,
            limit: 10,
        };

        if let Ok(exps) = experience_bank.query_similar(query).await {
            let success_count = exps.iter().filter(|e| e.outcome_success).count();
            if success_count >= 5 {
                rarity_score = 0.1; // Well known
            } else if success_count >= 2 {
                rarity_score = 0.3; // Somewhat familiar
            } else if exps.is_empty() {
                rarity_score = 1.0; // Completely new
            }
        }

        // Final combined score
        let final_score = (structural_score * 0.4) + (rarity_score * 0.6);

        if final_score < 0.3 {
            CognitiveDepth::Low
        } else if final_score < 0.6 {
            CognitiveDepth::Medium
        } else if final_score < 0.85 {
            CognitiveDepth::High
        } else {
            CognitiveDepth::Ultra
        }
    }
}
