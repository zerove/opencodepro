use crate::{
    action_label, classify_goal, tool_id_for_action, ActionType, FlowSynthesizer, SynthesisResult,
};
use async_trait::async_trait;
use sisyphus_types::{ExecutionContext, SOPGraph};
use std::sync::Arc;

#[async_trait]
pub trait GoalDecomposer: Send + Sync {
    async fn decompose(&self, goal: &str) -> Result<Vec<ActionType>, String>;
}

pub struct LlmGoalDecomposer<F>
where
    F: Fn(String) -> Result<Vec<ActionType>, String> + Send + Sync + 'static,
{
    decompose_fn: F,
}

impl<F> LlmGoalDecomposer<F>
where
    F: Fn(String) -> Result<Vec<ActionType>, String> + Send + Sync + 'static,
{
    pub fn new(decompose_fn: F) -> Self {
        Self { decompose_fn }
    }
}

#[async_trait]
impl<F> GoalDecomposer for LlmGoalDecomposer<F>
where
    F: Fn(String) -> Result<Vec<ActionType>, String> + Send + Sync + 'static,
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
    pub fn new(
        decomposer: Arc<dyn GoalDecomposer>,
        tool_executor: Arc<dyn sisyphus_types::ToolExecutor>,
    ) -> Self {
        Self {
            decomposer,
            tool_executor,
        }
    }
}

#[async_trait]
impl FlowSynthesizer for LlmDrivenSynthesizer {
    async fn synthesize(
        &self,
        goal: &str,
        _context: &ExecutionContext,
        mode: sisyphus_types::WorkMode,
    ) -> Result<SynthesisResult, String> {
        if mode == sisyphus_types::WorkMode::Standard {
            // --- Standard Linear Pipeline ---
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
                if first_idx.is_none() { first_idx = Some(n); }
                if let Some(p) = prev { graph.add_edge(p, n); }
                prev = Some(n);
            }

            Ok(SynthesisResult {
                start_node: first_idx.expect("actions not empty"),
                graph,
                plan_description: format!("Standard pipeline: {}", actions.len()),
            })
        } else {
            // --- UltraWork Rigorous Cycle (Explore -> Plan -> Implement -> Verify) ---
            let mut graph = SOPGraph::new();
            
            // 1. Explore Node
            let explore = graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
                name: "Rigorous-Explore".into(),
                tool_id: "lsp_symbol_search".into(),
                fuel_limit: 2_000_000,
                executor: self.tool_executor.clone(),
            }));

            // 2. Plan Node
            let plan = graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
                name: "Rigorous-Plan".into(),
                tool_id: "metis/analyse".into(),
                fuel_limit: 2_000_000,
                executor: self.tool_executor.clone(),
            }));
            graph.add_edge(explore, plan);

            // 3. Implement Node
            let implement = graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
                name: "Rigorous-Implement".into(),
                tool_id: "bash_exec".into(),
                fuel_limit: 5_000_000,
                executor: self.tool_executor.clone(),
            }));
            graph.add_edge(plan, implement);

            // 4. Verify Node
            let verify = graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
                name: "Rigorous-Verify".into(),
                tool_id: "type_check".into(),
                fuel_limit: 1_000_000,
                executor: self.tool_executor.clone(),
            }));
            graph.add_edge(implement, verify);

            Ok(SynthesisResult {
                start_node: explore,
                graph,
                plan_description: "UltraWork Rigorous Cycle: Explore -> Plan -> Implement -> Verify".into(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fallback_decomposer() {
        let llm_decomposer = Arc::new(LlmGoalDecomposer::new(|_| Err("LLM Error".into())));
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

pub struct LogicalSOPSynthesizer {
    pub llm_client: Arc<dyn sisyphus_tools_llm::LlmClient>,
    pub tool_executor: Arc<dyn sisyphus_types::ToolExecutor>,
}

impl LogicalSOPSynthesizer {
    pub fn new(
        llm_client: Arc<dyn sisyphus_tools_llm::LlmClient>,
        tool_executor: Arc<dyn sisyphus_types::ToolExecutor>,
    ) -> Self {
        Self { llm_client, tool_executor }
    }
}

#[async_trait]
impl FlowSynthesizer for LogicalSOPSynthesizer {
    async fn synthesize(
        &self,
        goal: &str,
        _context: &ExecutionContext,
        _mode: sisyphus_types::WorkMode,
    ) -> Result<SynthesisResult, String> {
        let system_prompt = "\
You are a Strategic SOP Architect. Your goal is to design a high-fidelity SOP graph to achieve the user's goal.
You can use the following node types:
1. Wasm: { tool_id, fuel_limit } - Executes a specific tool.
2. Retry: { child, max_retries } - Retries the child node on failure.
3. Branch: A decision node. It takes the output of the previous node and jumps to a specific next node.

Output a valid JSON matching the SOPGraphConfig structure:
{
  \"nodes\": [
    { \"name\": \"Node1\", \"kind\": { \"Wasm\": { \"tool_id\": \"...\", \"fuel_limit\": 1000000 } } },
    { \"name\": \"Node2\", \"kind\": \"Branch\" },
    ...
  ],
  \"edges\": [ [0, 1], [1, 2], ... ]
}
Also provide a 'plan_description' explaining the logic.
Respond ONLY with JSON.";

        let prompt = format!("Goal: {}\n\nDesign the SOP graph.", goal);
        let response = self.llm_client.chat(system_prompt, &prompt, true).await
            .map_err(|e| e.to_string())?;

        let parsed: serde_json::Value = serde_json::from_str(&response).map_err(|e| e.to_string())?;
        let config: sisyphus_types::SOPGraphConfig = serde_json::from_value(parsed.clone()).map_err(|e| e.to_string())?;
        
        let graph = SOPGraph::from_config(&config, self.tool_executor.clone()).map_err(|e| e.to_string())?;
        let plan_description = parsed["plan_description"].as_str().unwrap_or("Logical SOP").to_string();

        Ok(SynthesisResult {
            start_node: graph.graph.node_indices().next().unwrap(),
            graph,
            plan_description,
        })
    }
}
