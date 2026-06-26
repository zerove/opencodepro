use async_trait::async_trait;
use std::sync::Arc;
use sisyphus_types::{ExecutionContext};
use crate::{
    FlowSynthesizer, SynthesisResult, SOPGraph, 
    ActionType, classify_goal, tool_id_for_action, action_label
};

#[async_trait]
pub trait GoalDecomposer: Send + Sync {
    async fn decompose(&self, goal: &str) -> Result<Vec<ActionType>, String>;
}

pub struct LlmGoalDecomposer<F> 
where 
    F: Fn(String) -> Result<Vec<ActionType>, String> + Send + Sync + 'static 
{
    decompose_fn: F,
}

impl<F> LlmGoalDecomposer<F> 
where 
    F: Fn(String) -> Result<Vec<ActionType>, String> + Send + Sync + 'static 
{
    pub fn new(decompose_fn: F) -> Self {
        Self { decompose_fn }
    }
}

#[async_trait]
impl<F> GoalDecomposer for LlmGoalDecomposer<F> 
where 
    F: Fn(String) -> Result<Vec<ActionType>, String> + Send + Sync + 'static 
{
    async fn decompose(&self, goal: &str) -> Result<Vec<ActionType>, String> {
        (self.decompose_fn)(goal.to_string())
    }
}

pub struct FallbackGoalDecomposer {
    inner: Arc<dyn GoalDecomposer>,
}

impl FallbackGoalDecomposer {
    pub fn new(inner: Arc<dyn GoalDecomposer>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl GoalDecomposer for FallbackGoalDecomposer {
    async fn decompose(&self, goal: &str) -> Result<Vec<ActionType>, String> {
        match self.inner.decompose(goal).await {
            Ok(actions) => Ok(actions),
            Err(e) => {
                tracing::warn!(error = %e, "LLM decomposition failed, falling back to keyword matching");
                Ok(classify_goal(goal))
            }
        }
    }
}

pub struct LlmDrivenSynthesizer {
    pub decomposer: Arc<dyn GoalDecomposer>,
    pub tool_executor: Arc<dyn sisyphus_types::ToolExecutor>,
}

impl LlmDrivenSynthesizer {
    pub fn new(decomposer: Arc<dyn GoalDecomposer>, tool_executor: Arc<dyn sisyphus_types::ToolExecutor>) -> Self {
        Self { decomposer, tool_executor }
    }
}

#[async_trait]
// ... (existing code) ...
impl FlowSynthesizer for LlmDrivenSynthesizer {
    async fn synthesize(
        &self,
        goal: &str,
        _context: &ExecutionContext,
    ) -> Result<SynthesisResult, String> {
        let actions = self.decomposer.decompose(goal).await?;
        let mut graph = SOPGraph::new();

        if actions.is_empty() {
            let n = graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
                name: "default".into(),
                tool_id: "fast_tool".into(),
                fuel_limit: 1_000_000,
                executor: self.tool_executor.clone(),
            }));
            return Ok(SynthesisResult {
                graph,
                start_node: n,
                plan_description: "Default single-node task".into(),
            });
        }

        // Simple linear pipeline synthesis from decomposed actions
        let mut prev = None;
        let mut first_idx = None;

        for action in &actions {
            let label = action_label(action);
            let n = graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
                name: label.into(),
                tool_id: tool_id_for_action(action).into(),
                fuel_limit: 1_000_000,
                executor: self.tool_executor.clone(),
            }));
            if first_idx.is_none() {
                first_idx = Some(n);
            }
            if let Some(p) = prev {
                graph.add_edge(p, n);
            }
            prev = Some(n);
        }

        Ok(SynthesisResult {
            start_node: first_idx.expect("actions not empty"),
            graph,
            plan_description: format!("LLM-synthesized pipeline: {}", actions.len())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sisyphus_types::ToolExecutor;
    use async_trait::async_trait;

    #[allow(dead_code)]
    struct MockExecutor;
    #[async_trait]
    impl ToolExecutor for MockExecutor {
        async fn execute(&self, _id: &str, _args: Vec<u8>, _fuel: u64) -> Result<Vec<u8>, String> {
            Ok(b"ok".to_vec())
        }
    }

    #[tokio::test]
    async fn test_fallback_decomposer() {
        let llm_decomposer = Arc::new(LlmGoalDecomposer::new(|_| {
            Err("LLM Error".into())
        }));
        let fallback = Arc::new(FallbackGoalDecomposer::new(llm_decomposer));
        
        let goal = "fetch and process";
        let actions = fallback.decompose(goal).await.unwrap();
        
        assert!(actions.contains(&ActionType::Fetch));
        assert!(actions.contains(&ActionType::Process));
    }

    #[tokio::test]
    async fn test_llm_decomposer_success() {
        let llm_decomposer = Arc::new(LlmGoalDecomposer::new(|_| {
            Ok(vec![ActionType::Fetch, ActionType::Sink])
        }));
        
        let goal = "some goal";
        let actions = llm_decomposer.decompose(goal).await.unwrap();
        assert_eq!(actions, vec![ActionType::Fetch, ActionType::Sink]);
    }
}
