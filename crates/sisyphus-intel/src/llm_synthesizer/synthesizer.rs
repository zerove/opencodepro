//! The LLM-driven pipeline synthesizer: decomposes a goal into actions and
//! builds a linear SOP graph (or a rigorous cycle in UltraWork mode).

use crate::{action_label, tool_id_for_action, FlowSynthesizer, GoalDecomposer, SynthesisResult};
use async_trait::async_trait;
use sisyphus_types::{ExecutionContext, SOPGraph};
use std::sync::Arc;

pub struct LlmDrivenSynthesizer {
    pub decomposer: Arc<dyn GoalDecomposer>,
    pub tool_executor: Arc<dyn sisyphus_types::ToolExecutor>,
    pub test_executor: Arc<dyn sisyphus_types::TestExecutor>,
}

impl LlmDrivenSynthesizer {
    pub fn new(
        decomposer: Arc<dyn GoalDecomposer>,
        tool_executor: Arc<dyn sisyphus_types::ToolExecutor>,
        test_executor: Arc<dyn sisyphus_types::TestExecutor>,
    ) -> Self {
        Self {
            decomposer,
            tool_executor,
            test_executor,
        }
    }
}

#[async_trait]
impl FlowSynthesizer for LlmDrivenSynthesizer {
    async fn synthesize(
        &self,
        goal: &str,
        _context: &ExecutionContext,
        _project_id: &str,
        mode: sisyphus_types::WorkMode,
        _insights: Option<&str>,
    ) -> Result<SynthesisResult, sisyphus_types::IntelError> {
        if mode == sisyphus_types::WorkMode::Standard {
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
                    graph: Arc::new(graph),
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
                if first_idx.is_none() {
                    first_idx = Some(n);
                }
                if let Some(p) = prev {
                    graph.add_edge(p, n);
                }
                prev = Some(n);
            }

            Ok(SynthesisResult {
                start_node: first_idx.ok_or_else(|| {
                    sisyphus_types::IntelError::Internal("actions not empty".to_string())
                })?,
                graph: Arc::new(graph),
                plan_description: format!("Standard pipeline: {}", actions.len()),
            })
        } else {
            let mut graph = SOPGraph::new();
            let explore = graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
                name: "Rigorous-Explore".into(),
                tool_id: "lsp_symbol_search".into(),
                fuel_limit: 2_000_000,
                executor: self.tool_executor.clone(),
            }));
            let plan = graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
                name: "Rigorous-Plan".into(),
                tool_id: "metis/analyse".into(),
                fuel_limit: 2_000_000,
                executor: self.tool_executor.clone(),
            }));
            graph.add_edge(explore, plan);
            let implement = graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
                name: "Rigorous-Implement".into(),
                tool_id: "bash_exec".into(),
                fuel_limit: 5_000_000,
                executor: self.tool_executor.clone(),
            }));
            graph.add_edge(plan, implement);
            let verify = graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
                name: "Rigorous-Verify".into(),
                tool_id: "type_check".into(),
                fuel_limit: 1_000_000,
                executor: self.tool_executor.clone(),
            }));
            graph.add_edge(implement, verify);

            Ok(SynthesisResult {
                start_node: explore,
                graph: Arc::new(graph),
                plan_description:
                    "UltraWork Rigorous Cycle: Explore -> Plan -> Implement -> Verify".into(),
            })
        }
    }
}
