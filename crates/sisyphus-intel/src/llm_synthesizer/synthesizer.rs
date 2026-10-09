//! LLM 驱动的流水线合成器：将目标分解为动作，
//! 并构建线性 SOP 图谱（UltraWork 模式下为严格循环）。

use crate::{
    action_label, default_args_for_action, tool_id_for_action, FlowSynthesizer, GoalDecomposer,
    SynthesisResult,
};
use async_trait::async_trait;
use sisyphus_types::{ExecutionContext, SOPGraph};
use std::sync::Arc;

/// Llm Driven Synthesizer.
pub struct LlmDrivenSynthesizer {
    pub decomposer: Arc<dyn GoalDecomposer>,
    pub tool_executor: Arc<dyn sisyphus_types::ToolExecutor>,
    pub test_executor: Arc<dyn sisyphus_types::TestExecutor>,
}

impl LlmDrivenSynthesizer {
    /// New.
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
                    tool_id: "builtin:bash_exec".into(),
                    fuel_limit: 1_000_000,
                    executor: self.tool_executor.clone(),
                    args: default_args_for_action(&crate::ActionType::Process, goal),
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
                    args: default_args_for_action(action, goal),
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
            // 这 4 个节点过去写的是裸 tool_id（无 `builtin:` 前缀）。
            // `ExecutorRouter` 只按已注册前缀分发，裸 ID 会落到 default
            // 执行器（WASM），必然找不到同名工具——与 913ae9d 修掉的
            // `fast_tool` 是同一类路由缺陷，只是当时只覆盖了 mapping 函数。
            let explore = graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
                name: "Rigorous-Explore".into(),
                tool_id: "builtin:lsp_symbol_search".into(),
                fuel_limit: 2_000_000,
                executor: self.tool_executor.clone(),
                args: serde_json::to_vec(&serde_json::json!({ "query": goal })).unwrap_or_default(),
            }));
            let plan = graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
                name: "Rigorous-Plan".into(),
                tool_id: "builtin:metis/analyse".into(),
                fuel_limit: 2_000_000,
                executor: self.tool_executor.clone(),
                args: serde_json::to_vec(&serde_json::json!({ "request": goal }))
                    .unwrap_or_default(),
            }));
            graph.add_edge(explore, plan);
            let implement = graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
                name: "Rigorous-Implement".into(),
                tool_id: "builtin:bash_exec".into(),
                fuel_limit: 5_000_000,
                executor: self.tool_executor.clone(),
                args: default_args_for_action(&crate::ActionType::Process, goal),
            }));
            graph.add_edge(plan, implement);
            let verify = graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
                name: "Rigorous-Verify".into(),
                tool_id: "builtin:type_check".into(),
                fuel_limit: 1_000_000,
                executor: self.tool_executor.clone(),
                args: serde_json::to_vec(&serde_json::json!({})).unwrap_or_default(),
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
