use async_trait::async_trait;
use petgraph::graph::NodeIndex;
use sisyphus_tools_llm::metis::LlmClient;
use sisyphus_types::{CompositeGraph, ExecutionContext, SOPGraph, SOPGraphConfig};
use std::collections::HashMap;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// 流程合成
// ---------------------------------------------------------------------------

pub struct SynthesisResult {
    pub graph: Arc<SOPGraph>,
    pub start_node: NodeIndex,
    pub plan_description: String,
}

#[async_trait]
pub trait FlowSynthesizer: Send + Sync {
    async fn synthesize(
        &self,
        goal: &str,
        context: &ExecutionContext,
        project_id: &str,
        mode: sisyphus_types::WorkMode,
        insights: Option<&str>,
    ) -> Result<SynthesisResult, sisyphus_types::IntelError>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum ActionType {
    Fetch,
    Process,
    Sink,
    Search,
    Validate,
}

/// 目标驱动的 SOP 合成引擎。
///
/// 将自然语言目标解析为类型化动作的 DAG：
///   - Fetch/Read/Get → 获取节点
///   - Process/Transform/Convert → 处理节点
///   - Store/Save/Write → 汇点节点
///   - Search/Find/Query → 搜索节点
///   - Validate/Check/Verify → 验证节点
///
/// 多个动作按检测顺序链接为线性流水线。
/// 当目标出现 "parallel" 时，前两个动作扇出并汇聚到
/// 单个合并节点。
pub(crate) fn classify_goal(goal: &str) -> Vec<ActionType> {
    let goal_lower = goal.to_lowercase();
    let mut actions = Vec::new();

    let verb_map: [(&[&str], ActionType); 5] = [
        (
            &[
                "fetch", "load", "get", "read", "retrieve", "download", "pull",
            ] as &[&str],
            ActionType::Fetch,
        ),
        (
            &[
                "process",
                "transform",
                "convert",
                "parse",
                "compute",
                "analyze",
                "calculate",
                "build",
            ],
            ActionType::Process,
        ),
        (
            &[
                "store", "save", "write", "sink", "persist", "archive", "upload",
            ],
            ActionType::Sink,
        ),
        (
            &["search", "find", "query", "lookup", "discover", "scan"],
            ActionType::Search,
        ),
        (
            &[
                "validate", "check", "verify", "test", "inspect", "audit", "lint",
            ],
            ActionType::Validate,
        ),
    ];

    for (verbs, action) in &verb_map {
        if verbs.iter().any(|v| goal_lower.contains(v)) && !actions.contains(action) {
            actions.push(action.clone());
        }
    }

    if actions.is_empty() {
        actions.push(ActionType::Fetch);
    }
    actions
}

pub(crate) fn tool_id_for_action(action: &ActionType) -> &str {
    match action {
        ActionType::Fetch => "fast_tool",
        ActionType::Process => "fast_tool",
        ActionType::Sink => "fast_tool",
        ActionType::Search => "fast_tool",
        ActionType::Validate => "fast_tool",
    }
}

pub(crate) fn action_label(action: &ActionType) -> &str {
    match action {
        ActionType::Fetch => "fetch",
        ActionType::Process => "process",
        ActionType::Sink => "sink",
        ActionType::Search => "search",
        ActionType::Validate => "validate",
    }
}

fn build_plan_description(actions: &[ActionType], parallel: bool) -> String {
    let labels: Vec<&str> = actions.iter().map(action_label).collect();
    if parallel && actions.len() >= 2 {
        format!("Fan-out: {} → Merge → {}", labels[0], labels[1])
    } else {
        format!("Linear Pipeline: {}", labels.join(" -> "))
    }
}

pub struct GoalDrivenSynthesizer {
    pub tool_executor: Arc<dyn sisyphus_types::ToolExecutor>,
}

impl GoalDrivenSynthesizer {
    /// New.
    pub fn new(tool_executor: Arc<dyn sisyphus_types::ToolExecutor>) -> Self {
        Self { tool_executor }
    }
}

#[async_trait]
impl FlowSynthesizer for GoalDrivenSynthesizer {
    async fn synthesize(
        &self,
        goal: &str,
        _context: &ExecutionContext,
        _project_id: &str,
        _mode: sisyphus_types::WorkMode,
        _insights: Option<&str>,
    ) -> Result<SynthesisResult, sisyphus_types::IntelError> {
        let actions = classify_goal(goal);
        let is_parallel = goal.to_lowercase().contains("parallel");
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

        if actions.len() == 1 {
            let action = &actions[0];
            let label = action_label(action);
            let n = graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
                name: label.into(),
                tool_id: tool_id_for_action(action).into(),
                fuel_limit: 1_000_000,
                executor: self.tool_executor.clone(),
            }));
            return Ok(SynthesisResult {
                graph: Arc::new(graph),
                start_node: n,
                plan_description: format!("Single-node task: {}", label),
            });
        }

        if is_parallel && actions.len() >= 2 {
            // 扇出：多个独立节点汇聚到合并节点
            let n1 = graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
                name: action_label(&actions[0]).into(),
                tool_id: tool_id_for_action(&actions[0]).into(),
                fuel_limit: 1_000_000,
                executor: self.tool_executor.clone(),
            }));
            let n2 = graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
                name: action_label(&actions[1]).into(),
                tool_id: tool_id_for_action(&actions[1]).into(),
                fuel_limit: 1_000_000,
                executor: self.tool_executor.clone(),
            }));
            let merge = graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
                name: "merge".into(),
                tool_id: "fast_tool".into(),
                fuel_limit: 1_000_000,
                executor: self.tool_executor.clone(),
            }));
            graph.add_edge(n1, merge);
            graph.add_edge(n2, merge);

            Ok(SynthesisResult {
                graph: Arc::new(graph),
                start_node: n1,
                plan_description: format!(
                    "Fan-out: {} and {} → Merge",
                    action_label(&actions[0]),
                    action_label(&actions[1])
                ),
            })
        } else {
            // 线性流水线
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
                start_node: first_idx
                    .ok_or_else(|| "actions must have at least 2 entries".to_string())?,
                graph: Arc::new(graph),
                plan_description: build_plan_description(&actions, false),
            })
        }
    }
}

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub struct DynamicSopPlan {
    pub nodes: Vec<DynamicNode>,
    pub edges: Vec<DynamicEdge>,
    pub start_node: String,
}

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub struct DynamicNode {
    pub id: String,
    pub name: String,
    pub tool_id: String,
    pub args: serde_json::Value,
}

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub struct DynamicEdge {
    pub from: String,
    pub to: String,
}

pub struct DynamicSopSynthesizer {
    pub tool_executor: Arc<dyn sisyphus_types::ToolExecutor>,
    pub llm_client: Arc<dyn LlmClient>,
}

impl DynamicSopSynthesizer {
    /// New.
    pub fn new(
        tool_executor: Arc<dyn sisyphus_types::ToolExecutor>,
        llm_client: Arc<dyn LlmClient>,
    ) -> Self {
        Self {
            tool_executor,
            llm_client,
        }
    }
}

#[async_trait]
impl FlowSynthesizer for DynamicSopSynthesizer {
    async fn synthesize(
        &self,
        goal: &str,
        _context: &ExecutionContext,
        _project_id: &str,
        _mode: sisyphus_types::WorkMode,
        insights: Option<&str>,
    ) -> Result<SynthesisResult, sisyphus_types::IntelError> {
        let insight_text = match insights {
            Some(i) => format!("\n\nHistorical insights for similar goals:\n{}", i),
            None => "".to_string(),
        };

        let prompt = format!(
            "Synthesize a Standard Operating Procedure (SOP) for the following goal: '{}'.\
            {}Return a JSON object with 'nodes' (id, name, tool_id, args), 'edges' (from, to), and 'start_node'.\
            Ensure it is a valid DAG.", 
            goal, insight_text
        );

        let response = self
            .llm_client
            .generate(&prompt)
            .await
            .map_err(|e| format!("LLM synthesis failed: {e}"))?;

        let plan: DynamicSopPlan = serde_json::from_str(&response)
            .map_err(|e| format!("Failed to parse SOP JSON: {e}. Response was: {response}"))?;

        let mut graph = SOPGraph::new();
        let mut id_map = HashMap::new();

        for node in &plan.nodes {
            let idx = graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
                name: node.name.clone(),
                tool_id: node.tool_id.clone(),
                fuel_limit: 1_000_000,
                executor: self.tool_executor.clone(),
            }));
            id_map.insert(node.id.clone(), idx);
        }

        for edge in &plan.edges {
            let src = id_map
                .get(&edge.from)
                .ok_or_else(|| format!("Source node {} not found", edge.from))?;
            let tgt = id_map
                .get(&edge.to)
                .ok_or_else(|| format!("Target node {} not found", edge.to))?;
            graph.add_edge(*src, *tgt);
        }

        let start_node = id_map
            .get(&plan.start_node)
            .ok_or_else(|| "Start node not found".to_string())?;

        Ok(SynthesisResult {
            graph: Arc::new(graph),
            start_node: *start_node,
            plan_description: format!("Dynamic LLM-generated SOP with {} nodes", plan.nodes.len()),
        })
    }
}

/// 将多个目标合成单个 CompositeGraph。
/// 每个目标成为无跨依赖的独立子图，
/// 因此可由 CompositeExecutor 并行执行。
pub async fn synthesize_composite(
    goals: &[&str],
    synthesizer: &dyn FlowSynthesizer,
    ctx: &ExecutionContext,
    project_id: &str,
) -> Result<CompositeGraph, sisyphus_types::IntelError> {
    let mut composite = CompositeGraph::new();
    for (i, goal) in goals.iter().enumerate() {
        let sop_id = format!("goal_{}", i);
        let result = synthesizer
            .synthesize(
                goal,
                ctx,
                project_id,
                sisyphus_types::WorkMode::Standard,
                None,
            )
            .await?;
        let config = SOPGraphConfig::from_graph(&result.graph)
            .map_err(|e| sisyphus_types::IntelError::Internal(e.to_string()))?;
        composite.add_graph(&sop_id, config);
    }
    Ok(composite)
}

pub struct MetaLearningSynthesizer {
    pub inner: Arc<dyn FlowSynthesizer>,
    pub meta_learner: Arc<parking_lot::Mutex<sisyphus_tools_llm_tools::meta_learner::MetaLearner>>,
}

impl MetaLearningSynthesizer {
    /// New.
    pub fn new(
        inner: Arc<dyn FlowSynthesizer>,
        meta_learner: Arc<parking_lot::Mutex<sisyphus_tools_llm_tools::meta_learner::MetaLearner>>,
    ) -> Self {
        Self {
            inner,
            meta_learner,
        }
    }
}

#[async_trait]
impl FlowSynthesizer for MetaLearningSynthesizer {
    async fn synthesize(
        &self,
        goal: &str,
        context: &ExecutionContext,
        project_id: &str,
        mode: sisyphus_types::WorkMode,
        insights: Option<&str>,
    ) -> Result<SynthesisResult, sisyphus_types::IntelError> {
        let best_strategy = {
            let ml = self.meta_learner.lock();
            ml.best_strategy_params(&serde_json::json!({ "goal": goal }))
        };

        let augmented_goal = if let Some(ref strategy) = best_strategy {
            format!("Goal: {}\n(Hint: Based on historical success, the strategy '{:?}' is recommended for similar goals)", goal, strategy)
        } else {
            goal.to_string()
        };

        tracing::info!(goal = %goal, hint = ?best_strategy, "Synthesizing with meta-learning hint");

        self.inner
            .synthesize(&augmented_goal, context, project_id, mode, insights)
            .await
    }
}

pub struct StrategicSopSynthesizer {
    pub inner: Arc<dyn FlowSynthesizer>,
    pub simulator: Arc<dyn crate::Simulator>,
}

impl StrategicSopSynthesizer {
    /// New.
    pub fn new(inner: Arc<dyn FlowSynthesizer>, simulator: Arc<dyn crate::Simulator>) -> Self {
        Self { inner, simulator }
    }
}

#[async_trait]
impl FlowSynthesizer for StrategicSopSynthesizer {
    async fn synthesize(
        &self,
        goal: &str,
        context: &ExecutionContext,
        project_id: &str,
        mode: sisyphus_types::WorkMode,
        insights: Option<&str>,
    ) -> Result<SynthesisResult, sisyphus_types::IntelError> {
        let directives = [
            "", 
            "\n(Strategic Directive: Prioritize efficiency and minimize node count to reduce cost)",
            "\n(Strategic Directive: Prioritize robustness; include comprehensive validation and check nodes)",
        ];

        let mut candidates = Vec::new();
        for directive in directives {
            let augmented_goal = format!("{}{}", goal, directive);
            if let Ok(res) = self
                .inner
                .synthesize(&augmented_goal, context, project_id, mode, insights)
                .await
            {
                candidates.push(res);
            }
        }

        if candidates.is_empty() {
            return Err("Failed to synthesize any candidate SOPs".into());
        }

        let mut best_result = None;
        let mut max_score = f64::NEG_INFINITY;

        for res in candidates {
            let sim = self
                .simulator
                .simulate(
                    res.graph.clone(),
                    context,
                    project_id,
                    "strategic_eval",
                    None,
                )
                .await?;

            // 计分：概率为主，成本为次要惩罚
            // 概率（0.0-1.0）* 1000 - 成本（u64）
            let score = (sim.success_probability * 1000.0) - (sim.estimated_cost as f64);

            tracing::info!(
                goal = %goal,
                prob = sim.success_probability,
                cost = sim.estimated_cost,
                score = score,
                "Candidate evaluation completed"
            );

            if score > max_score {
                max_score = score;
                best_result = Some(res);
            }
        }

        let mut final_res = best_result.ok_or_else(|| "No valid candidate selected".to_string())?;
        final_res.plan_description = format!(
            "Strategic selection (Score: {:.2}): {}",
            max_score, final_res.plan_description
        );

        Ok(final_res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // classify_goal（目标分类）
    // -----------------------------------------------------------------------

    #[test]
    fn test_classify_fetch_verb() {
        let actions = classify_goal("fetch data from api");
        assert_eq!(actions, vec![ActionType::Fetch]);
    }

    #[test]
    fn test_classify_process_verb() {
        let actions = classify_goal("transform the input into JSON");
        assert_eq!(actions, vec![ActionType::Process]);
    }

    #[test]
    fn test_classify_sink_verb() {
        let actions = classify_goal("save results to database");
        assert_eq!(actions, vec![ActionType::Sink]);
    }

    #[test]
    fn test_classify_search_verb() {
        let actions = classify_goal("search for matching documents");
        assert_eq!(actions, vec![ActionType::Search]);
    }

    #[test]
    fn test_classify_validate_verb() {
        let actions = classify_goal("validate the configuration file");
        assert_eq!(actions, vec![ActionType::Validate]);
    }

    #[test]
    fn test_classify_multi_action_pipeline() {
        // "fetch" + "process" + "save" = 获取 + 处理 + 汇点
        let actions = classify_goal("fetch data, process it, then save");
        assert_eq!(actions.len(), 3);
        assert!(actions.contains(&ActionType::Fetch));
        assert!(actions.contains(&ActionType::Process));
        assert!(actions.contains(&ActionType::Sink));
    }

    #[test]
    fn test_classify_no_known_verb_defaults_to_fetch() {
        let actions = classify_goal("do something random");
        assert_eq!(actions, vec![ActionType::Fetch]);
    }

    #[test]
    fn test_classify_case_insensitive() {
        let actions = classify_goal("FETCH AND PARSE");
        assert_eq!(actions, vec![ActionType::Fetch, ActionType::Process]);
    }

    #[test]
    fn test_classify_no_duplicate_actions() {
        // 即使 "fetch" 出现两次，结果中也只有一个 Fetch
        let actions = classify_goal("fetch then fetch again");
        assert_eq!(actions, vec![ActionType::Fetch]);
    }

    #[test]
    fn test_classify_verb_prefix_no_false_positive() {
        // "store" 在 "history" 中 → 不应触发汇点
        let actions = classify_goal("read the history log");
        // "read" 映射到获取，"history" 不含 "store"
        assert_eq!(actions, vec![ActionType::Fetch]);
    }

    // -----------------------------------------------------------------------
    // action_label（动作标签）
    // -----------------------------------------------------------------------

    #[test]
    fn test_action_label_mapping() {
        assert_eq!(action_label(&ActionType::Fetch), "fetch");
        assert_eq!(action_label(&ActionType::Process), "process");
        assert_eq!(action_label(&ActionType::Sink), "sink");
        assert_eq!(action_label(&ActionType::Search), "search");
        assert_eq!(action_label(&ActionType::Validate), "validate");
    }

    // -----------------------------------------------------------------------
    // tool_id_for_action（动作工具 ID）
    // -----------------------------------------------------------------------

    #[test]
    fn test_tool_id_all_actions_return_fast_tool() {
        // 目前所有动作映射到 "fast_tool"
        assert_eq!(tool_id_for_action(&ActionType::Fetch), "fast_tool");
        assert_eq!(tool_id_for_action(&ActionType::Process), "fast_tool");
        assert_eq!(tool_id_for_action(&ActionType::Sink), "fast_tool");
        assert_eq!(tool_id_for_action(&ActionType::Search), "fast_tool");
        assert_eq!(tool_id_for_action(&ActionType::Validate), "fast_tool");
    }

    // -----------------------------------------------------------------------
    // build_plan_description（构建计划描述）
    // -----------------------------------------------------------------------

    #[test]
    fn test_plan_description_linear() {
        let actions = vec![ActionType::Fetch, ActionType::Process, ActionType::Sink];
        let desc = build_plan_description(&actions, false);
        assert_eq!(desc, "Linear Pipeline: fetch -> process -> sink");
    }

    #[test]
    fn test_plan_description_single_action() {
        let actions = vec![ActionType::Search];
        let desc = build_plan_description(&actions, false);
        assert_eq!(desc, "Linear Pipeline: search");
    }

    #[test]
    fn test_plan_description_parallel_fanout() {
        let actions = vec![ActionType::Fetch, ActionType::Process];
        let desc = build_plan_description(&actions, true);
        assert_eq!(desc, "Fan-out: fetch → Merge → process");
    }

    #[test]
    fn test_plan_description_parallel_ignored_when_single() {
        // parallel=true 但只有 1 个动作 → 回退到线性
        let actions = vec![ActionType::Fetch];
        let desc = build_plan_description(&actions, true);
        assert_eq!(desc, "Linear Pipeline: fetch");
    }

    // -----------------------------------------------------------------------
    // GoalDrivenSynthesizer::synthesize — 图谱结构
    // -----------------------------------------------------------------------

    fn dummy_executor() -> Arc<dyn sisyphus_types::ToolExecutor> {
        Arc::new(DummyExecutor)
    }

    struct DummyExecutor;
    #[async_trait::async_trait]
    impl sisyphus_types::ToolExecutor for DummyExecutor {
        async fn execute(
            &self,
            _tool_id: &str,
            _args: Vec<u8>,
            _fuel_limit: u64,
        ) -> Result<Vec<u8>, sisyphus_types::TypesError> {
            Ok(b"mock".to_vec())
        }
    }

    fn test_synthesizer() -> GoalDrivenSynthesizer {
        GoalDrivenSynthesizer::new(dummy_executor())
    }

    fn empty_ctx() -> ExecutionContext {
        ExecutionContext::new()
    }

    #[tokio::test]
    async fn test_synthesize_single_action() {
        let syn = test_synthesizer();
        let result = syn
            .synthesize(
                "fetch the data",
                &empty_ctx(),
                "test",
                sisyphus_types::WorkMode::Standard,
                None,
            )
            .await
            .unwrap();
        assert_eq!(result.graph.graph.node_count(), 1);
        assert!(result.plan_description.contains("Single-node"));
    }

    #[tokio::test]
    async fn test_synthesize_linear_pipeline() {
        let syn = test_synthesizer();
        let result = syn
            .synthesize(
                "fetch and process the data",
                &empty_ctx(),
                "test",
                sisyphus_types::WorkMode::Standard,
                None,
            )
            .await
            .unwrap();
        // Fetch → Process = 2 个节点，1 条边
        assert_eq!(result.graph.graph.node_count(), 2);
        assert_eq!(result.graph.graph.edge_count(), 1);
        assert!(result.plan_description.contains("Pipeline"));
    }

    #[tokio::test]
    async fn test_synthesize_parallel_fanout() {
        let syn = test_synthesizer();
        let result = syn
            .synthesize(
                "fetch and process in parallel",
                &empty_ctx(),
                "test",
                sisyphus_types::WorkMode::Standard,
                None,
            )
            .await
            .unwrap();
        // Fetch + Process → Merge = 3 个节点，2 条边
        assert_eq!(result.graph.graph.node_count(), 3);
        assert_eq!(result.graph.graph.edge_count(), 2);
        assert!(result.plan_description.contains("Fan-out"));
    }

    #[tokio::test]
    async fn test_synthesize_empty_goal_defaults() {
        let syn = test_synthesizer();
        let result = syn
            .synthesize(
                "xyzzy",
                &empty_ctx(),
                "test",
                sisyphus_types::WorkMode::Standard,
                None,
            )
            .await
            .unwrap();
        // 回退到 "default" 单节点
        assert_eq!(result.graph.graph.node_count(), 1);
    }
}
