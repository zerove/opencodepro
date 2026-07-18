use async_trait::async_trait;
use sisyphus_tools_llm::LlmClient;
use sisyphus_types::{SOPGraphConfig, SopEvaluation, SopEvaluator, TypesError};
use std::collections::HashMap;
use std::sync::Arc;

pub struct HeuristicSopEvaluator {
    llm: Arc<dyn LlmClient>,
}

impl HeuristicSopEvaluator {
    pub fn new(llm: Arc<dyn LlmClient>) -> Self {
        Self { llm }
    }

    fn calculate_complexity_score(&self, config: &SOPGraphConfig) -> f32 {
        let node_count = config.nodes.len();
        if node_count == 0 {
            return 0.0;
        }

        // Heuristic: Optimal SOP length is between 3 and 7 nodes.
        // Penalize very short (too simple/incomplete) or very long (too fragile) SOPs.
        if (3..=7).contains(&node_count) {
            1.0
        } else if node_count < 3 {
            0.7
        } else {
            (7.0 / node_count as f32).max(0.3)
        }
    }

    fn calculate_robustness_score(&self, config: &SOPGraphConfig) -> f32 {
        let node_count = config.nodes.len();
        if node_count == 0 {
            return 0.0;
        }

        // Heuristic: SOPs with branch/retry logic or high density of edges are more robust.
        let mut robustness_bonus = 0.0;
        for node in &config.nodes {
            match &node.kind {
                sisyphus_types::NodeKind::Retry { .. } => robustness_bonus += 0.2,
                sisyphus_types::NodeKind::Branch { .. } => robustness_bonus += 0.1,
                _ => {}
            }
        }

        (1.0_f32 + robustness_bonus).min(1.5_f32) / 1.5_f32
    }
}

#[async_trait]
impl SopEvaluator for HeuristicSopEvaluator {
    async fn evaluate(
        &self,
        goal: &str,
        config: &SOPGraphConfig,
    ) -> Result<SopEvaluation, TypesError> {
        // 1. Static Heuristics
        let complexity = self.calculate_complexity_score(config);
        let robustness = self.calculate_robustness_score(config);

        // 2. LLM Semantic Review
        let prompt = format!(
            "Review the following SOP designed for the goal: '{}'\n\nSOP Nodes:\n{:?}\nSOP Edges:\n{:?}\n\n\
            Please evaluate if this SOP logically achieves the goal. \
            Provide a score from 0.0 to 1.0 and a brief explanation. \
            Return format: SCORE: <score> | FEEDBACK: <feedback>",
            goal, config.nodes, config.edges
        );

        let llm_response = self
            .llm
            .generate(&prompt)
            .await
            .map_err(|e| TypesError::InternalError(e.to_string()))?;

        let mut llm_score = 0.5;
        if let Some(score_pos) = llm_response.find("SCORE: ") {
            let score_part = &llm_response[score_pos + 7..];
            if let Some(end_pos) = score_part.find(" |") {
                if let Ok(parsed) = score_part[..end_pos].trim().parse::<f32>() {
                    llm_score = parsed.clamp(0.0, 1.0);
                }
            }
        }

        let feedback = llm_response.clone();

        // Final Weighted Score
        // LLM logic review is the primary driver (60%), heuristics provide a baseline (40%)
        let final_score = (llm_score * 0.6) + ((complexity + robustness) / 2.0 * 0.4);

        let mut metrics = HashMap::new();
        metrics.insert("complexity".to_string(), complexity);
        metrics.insert("robustness".to_string(), robustness);
        metrics.insert("llm_logic".to_string(), llm_score);

        Ok(SopEvaluation {
            score: final_score,
            metrics,
            feedback,
            is_acceptable: final_score >= 0.7,
        })
    }
}
