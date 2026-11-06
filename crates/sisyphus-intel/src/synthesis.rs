use async_trait::async_trait;
use petgraph::graph::NodeIndex;
use sisyphus_tools_llm::metis::LlmClient;
use sisyphus_types::{CompositeGraph, ExecutionContext, SOPGraph, SOPGraphConfig};
use std::collections::HashMap;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// 流程合成
// ---------------------------------------------------------------------------

/// Synthesis Result.
pub struct SynthesisResult {
    pub graph: Arc<SOPGraph>,
    pub start_node: NodeIndex,
    pub plan_description: String,
}

#[async_trait]
/// Flow Synthesizer.
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
/// Action Type.
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

/// Map a semantic action to the ID of a **real, registered** tool.
///
/// The `builtin:` prefix is mandatory: [`ExecutorRouter`] dispatches on the
/// prefix and strips it before forwarding, so `builtin:web_fetch` reaches
/// `BuiltinToolExecutor` as `web_fetch` and hits its `match` arm. A bare or
/// unregistered ID falls through to the router's `default` executor (the WASM
/// executor in the bridge), which cannot run anything by that name.
///
/// `Process`, `Sink` and `Validate` have no purpose-built builtin tool. They
/// are mapped to `bash_exec`, which is the only registered tool able to
/// actually carry out an arbitrary transformation, emit its result, or check
/// it. Mapping them to a plausible-looking but unregistered name would make
/// every synthesized plan fail at execution time.
///
/// [`ExecutorRouter`]: sisyphus_types::node::ExecutorRouter
pub(crate) fn tool_id_for_action(action: &ActionType) -> &str {
    match action {
        ActionType::Fetch => "builtin:web_fetch",
        ActionType::Search => "builtin:web_search",
        ActionType::Process | ActionType::Sink | ActionType::Validate => "builtin:bash_exec",
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

/// 为合成节点构造**可执行**的工具入参。
///
/// `tool_id_for_action` 只解决了「路由到哪个执行器」，但 builtin 工具
/// 全部强制校验入参：`web_fetch` 要 `url`、`bash_exec` 要 `command`。
/// 而 `WasmSOPNode` 过去把 args 写死为空，因此每个合成节点在执行期都
/// 会因缺参而失败（实测 `missing or non-string arg 'url'`），最终被
/// supervisor 判为 deviation 0.81 并触发 `AllIterationsPivoted`。
///
/// 这里给出各动作的最小可执行参数。真实 URL / 命令应由 LLM 规划产出，
/// 这里的默认值只保证「工具能被调用」而不因缺参失败；
/// `DynamicSopSynthesizer`（LLM 路径）保留 LLM 给出的 args。
pub(crate) fn default_args_for_action(action: &ActionType, goal: &str) -> Vec<u8> {
    let payload = match action {
        ActionType::Fetch => serde_json::json!({
            "url": "https://example.com",
        }),
        ActionType::Search => serde_json::json!({
            "query": goal,
        }),
        ActionType::Process | ActionType::Sink | ActionType::Validate => serde_json::json!({
            "command": "echo",
        }),
    };
    serde_json::to_vec(&payload).unwrap_or_default()
}

fn build_plan_description(actions: &[ActionType], parallel: bool) -> String {
    let labels: Vec<&str> = actions.iter().map(action_label).collect();
    if parallel && actions.len() >= 2 {
        format!("Fan-out: {} → Merge → {}", labels[0], labels[1])
    } else {
        format!("Linear Pipeline: {}", labels.join(" -> "))
    }
}

/// Goal Driven Synthesizer.
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
                tool_id: "builtin:bash_exec".into(),
                fuel_limit: 1_000_000,
                executor: self.tool_executor.clone(),
                args: default_args_for_action(&ActionType::Process, goal),
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
                args: default_args_for_action(action, goal),
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
                args: default_args_for_action(&actions[0], goal),
            }));
            let n2 = graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
                name: action_label(&actions[1]).into(),
                tool_id: tool_id_for_action(&actions[1]).into(),
                fuel_limit: 1_000_000,
                executor: self.tool_executor.clone(),
                args: default_args_for_action(&actions[1], goal),
            }));
            let merge = graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
                name: "merge".into(),
                tool_id: "builtin:bash_exec".into(),
                fuel_limit: 1_000_000,
                executor: self.tool_executor.clone(),
                args: default_args_for_action(&ActionType::Sink, goal),
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
                start_node: first_idx
                    .ok_or_else(|| "actions must have at least 2 entries".to_string())?,
                graph: Arc::new(graph),
                plan_description: build_plan_description(&actions, false),
            })
        }
    }
}

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
/// Dynamic Sop Plan.
pub struct DynamicSopPlan {
    pub nodes: Vec<DynamicNode>,
    pub edges: Vec<DynamicEdge>,
    pub start_node: String,
}

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
/// Dynamic Node.
pub struct DynamicNode {
    pub id: String,
    pub name: String,
    pub tool_id: String,
    pub args: serde_json::Value,
}

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
/// Dynamic Edge.
pub struct DynamicEdge {
    pub from: String,
    pub to: String,
}

/// Dynamic Sop Synthesizer.
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
                // LLM 规划的 args 必须透传：builtin 工具（web_fetch/bash_exec）
                // 全部强制校验入参，丢弃后每个节点都会因缺参失败。
                args: serde_json::to_vec(&node.args).unwrap_or_default(),
            }));
            id_map.insert(node.id.clone(), idx);
        }

        for edge in &plan.edges {
            let src = id_map
                .get(&edge.from)
                .ok_or_else(|| format!("Source node {} not found", edge.from))?;
            let tgt =
                id_map.get(&edge.to).ok_or_else(|| format!("Target node {} not found", edge.to))?;
            graph.add_edge(*src, *tgt);
        }

        let start_node =
            id_map.get(&plan.start_node).ok_or_else(|| "Start node not found".to_string())?;

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

/// Meta Learning Synthesizer.
///
/// 默认只依据本地 `MetaLearner`。装上 [`Self::with_federated_selection`] 后，
/// 当本地证据不足时可以改用集群聚合出的策略（v0.4.0 Phase E，ADR-017
/// Amendment 3）。
pub struct MetaLearningSynthesizer {
    pub inner: Arc<dyn FlowSynthesizer>,
    pub meta_learner: Arc<parking_lot::Mutex<sisyphus_tools_llm_tools::meta_learner::MetaLearner>>,
    /// 集群策略权重视图。`None` = 不消费集群证据。
    ///
    /// 与 `StrategyWeightsHandler` 共享同一个 `Arc`：handler 写（本地发布 /
    /// 入站合并），这里只读。锁序固定为「先 `meta_learner`、后视图」，且都在
    /// `await` 之前释放——`parking_lot` 的守卫不是 Send，跨 await 也过不了
    /// clippy 的 `await_holding_lock`。
    federated_weights: Option<
        Arc<parking_lot::RwLock<sisyphus_types::strategy_weights::FederatedStrategyWeights>>,
    >,
    /// 决策优先级与证据门槛。
    selection: sisyphus_types::strategy_weights::StrategySelectionConfig,
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
            federated_weights: None,
            selection: sisyphus_types::strategy_weights::StrategySelectionConfig::default(),
        }
    }

    /// 接入集群策略权重视图（v0.4.0 Phase E，ADR-017 Amendment 3）。
    ///
    /// `config.enabled = false` 时与不调用本方法完全等价：关闭的开关必须真的
    /// 关闭，而不是「读了但不采纳」。
    pub fn with_federated_selection(
        mut self,
        weights: Arc<
            parking_lot::RwLock<sisyphus_types::strategy_weights::FederatedStrategyWeights>,
        >,
        config: sisyphus_types::strategy_weights::StrategySelectionConfig,
    ) -> Self {
        self.federated_weights = Some(weights);
        self.selection = config;
        self
    }

    /// 计算本次执行要提示的策略（纯决策，见 `select_strategy`）。
    fn decide_strategy(
        &self,
        goal: &str,
    ) -> Option<sisyphus_types::strategy_weights::StrategyDecision> {
        use sisyphus_types::strategy_weights::{select_strategy, StrategyStat};

        // 本地侧：先取「最优策略名」，再取该策略的统计。两者取自同一把锁的
        // 同一次持锁区间，避免中间被 `record_outcome` 插入而看到半新半旧。
        let local = {
            let ml = self.meta_learner.lock();
            ml.best_strategy_params(&serde_json::json!({ "goal": goal })).map(|(name, params)| {
                let stats = ml.strategy_stats(&name);
                (
                    name,
                    params,
                    StrategyStat {
                        uses: stats.total_uses,
                        successes: stats.successes,
                        avg_duration_secs: stats.avg_duration,
                    },
                )
            })
        };

        // 集群侧只在启用时读锁。
        let cluster = if self.selection.enabled {
            self.federated_weights
                .as_ref()
                .and_then(|view| view.read().best_strategy(self.selection.effective_min_uses()))
        } else {
            None
        };

        let params = local.as_ref().map(|(_, params, _)| params.clone());
        let decision = select_strategy(
            local.as_ref().map(|(name, _, stat)| (name.as_str(), *stat)),
            cluster,
            &self.selection,
        );
        // 参数只对本地来源有意义：集群快照不同步参数。
        match decision {
            Some(d) if d.source == sisyphus_types::strategy_weights::StrategySource::Local => {
                Some(d.with_params(params.unwrap_or_else(|| serde_json::json!({}))))
            },
            other => other,
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
        let decision = self.decide_strategy(goal);

        // 本地路径保持原有字符串不变（外部可能据此做匹配/断言）；集群路径用
        // 独立文案，并在提示里带上样本量——否则模型无从判断推荐的可信度。
        let augmented_goal = match decision.as_ref() {
            Some(d) if d.source == sisyphus_types::strategy_weights::StrategySource::Cluster => {
                format!(
                    "Goal: {}\n(Hint: Based on cluster-wide evidence ({} successes out of {} runs across all nodes, {:.0}% success), the strategy '{}' is recommended for similar goals)",
                    goal,
                    d.successes,
                    d.uses,
                    d.success_rate() * 100.0,
                    d.strategy
                )
            },
            Some(d) => format!(
                "Goal: {}\n(Hint: Based on historical success, the strategy '{:?}' is recommended for similar goals)",
                goal,
                (d.strategy.as_str(), d.params.clone().unwrap_or_else(|| serde_json::json!({})))
            ),
            None => goal.to_string(),
        };

        tracing::info!(
            goal = %goal,
            strategy = decision.as_ref().map(|d| d.strategy.as_str()),
            source = decision.as_ref().map(|d| d.source.as_str()),
            uses = decision.as_ref().map(|d| d.uses),
            success_rate = decision.as_ref().map(|d| d.success_rate()),
            "Synthesizing with meta-learning hint"
        );

        self.inner.synthesize(&augmented_goal, context, project_id, mode, insights).await
    }
}

/// Strategic Sop Synthesizer.
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
            if let Ok(res) =
                self.inner.synthesize(&augmented_goal, context, project_id, mode, insights).await
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
    fn test_tool_id_is_stable_per_action() {
        // Pins the action -> ID mapping so it cannot silently regress to a
        // placeholder such as the "fast_tool" that 913ae9d removed.
        //
        // This does NOT prove the IDs are registered: the expectations below are
        // hand-written literals, so renaming a tool in the bridge registry
        // leaves this test green. Registry consistency is verified in
        // `sisyphus-bridge` by `tools::tests::planner_tool_ids_exist_in_registry`,
        // which checks the synthesizer's real output against the real registry.
        let cases = [
            (ActionType::Fetch, "builtin:web_fetch"),
            (ActionType::Search, "builtin:web_search"),
            (ActionType::Process, "builtin:bash_exec"),
            (ActionType::Sink, "builtin:bash_exec"),
            (ActionType::Validate, "builtin:bash_exec"),
        ];
        for (action, expected) in cases {
            assert_eq!(tool_id_for_action(&action), expected);
        }
    }

    #[test]
    fn test_tool_ids_carry_routable_prefix() {
        // `ExecutorRouter` only dispatches on a known prefix and strips it
        // before forwarding; a bare ID silently falls through to `default`.
        for action in [
            ActionType::Fetch,
            ActionType::Process,
            ActionType::Sink,
            ActionType::Search,
            ActionType::Validate,
        ] {
            let id = tool_id_for_action(&action);
            assert!(
                id.starts_with("builtin:"),
                "{id:?} lacks the builtin: prefix and would fall through to the default executor"
            );
        }
    }

    #[test]
    fn test_tool_ids_are_distinct_where_tools_differ() {
        // Guards against regressing to a single placeholder for every action.
        // Process/Sink/Validate intentionally share bash_exec.
        assert_ne!(
            tool_id_for_action(&ActionType::Fetch),
            tool_id_for_action(&ActionType::Process)
        );
        assert_ne!(
            tool_id_for_action(&ActionType::Search),
            tool_id_for_action(&ActionType::Sink)
        );
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

    // -----------------------------------------------------------------------
    // MetaLearningSynthesizer 消费集群策略权重（v0.4.0 Phase E，ADR-017 Amendment 3）
    // -----------------------------------------------------------------------

    struct CapturingSynthesizer {
        augmented_goals: Arc<parking_lot::Mutex<Vec<String>>>,
    }

    #[async_trait::async_trait]
    impl FlowSynthesizer for CapturingSynthesizer {
        async fn synthesize(
            &self,
            goal: &str,
            _context: &ExecutionContext,
            _project_id: &str,
            _mode: sisyphus_types::WorkMode,
            _insights: Option<&str>,
        ) -> Result<SynthesisResult, sisyphus_types::IntelError> {
            let mut goals = self.augmented_goals.lock();
            goals.push(goal.to_string());
            Err(sisyphus_types::IntelError::SynthesisFailed {
                goal: goal.to_string(),
                reason: "captured".to_string(),
            })
        }
    }

    fn capturing_syn() -> Arc<CapturingSynthesizer> {
        Arc::new(CapturingSynthesizer {
            augmented_goals: Arc::new(parking_lot::Mutex::new(Vec::new())),
        })
    }

    fn temp_meta_learner(
    ) -> Arc<parking_lot::Mutex<sisyphus_tools_llm_tools::meta_learner::MetaLearner>> {
        let dir = tempfile::tempdir().expect("temp dir").keep().to_string_lossy().into_owned();
        Arc::new(parking_lot::Mutex::new(
            sisyphus_tools_llm_tools::meta_learner::MetaLearner::new(&dir),
        ))
    }

    fn record(
        meta_learner: &Arc<parking_lot::Mutex<sisyphus_tools_llm_tools::meta_learner::MetaLearner>>,
        strategy: &str,
        uses: u64,
        successes: u64,
    ) {
        let mut ml = meta_learner.lock();
        for i in 0..uses {
            ml.record_outcome(sisyphus_types::MetaLearningSample {
                context: serde_json::json!({ "goal": "fetch ai news" }),
                strategy: strategy.into(),
                params: serde_json::json!({}),
                success: i < successes,
                duration_secs: 1.0,
                timestamp: 0,
            })
            .expect("record outcome");
        }
    }

    fn capture_syn(
        meta_learner: Arc<parking_lot::Mutex<sisyphus_tools_llm_tools::meta_learner::MetaLearner>>,
        weights: Arc<
            parking_lot::RwLock<sisyphus_types::strategy_weights::FederatedStrategyWeights>,
        >,
        selection: sisyphus_types::strategy_weights::StrategySelectionConfig,
    ) -> (
        Arc<MetaLearningSynthesizer>,
        Arc<parking_lot::Mutex<Vec<String>>>,
    ) {
        let capturing = capturing_syn();
        let augmented_goals = capturing.augmented_goals.clone();
        let inner: Arc<dyn FlowSynthesizer> = capturing;
        let syn = MetaLearningSynthesizer::new(inner, meta_learner.clone());
        (
            Arc::new(syn.with_federated_selection(weights, selection)),
            augmented_goals,
        )
    }

    async fn last_augmented_goal(
        syn: &Arc<MetaLearningSynthesizer>,
        augmented_goals: &Arc<parking_lot::Mutex<Vec<String>>>,
    ) -> String {
        let _ = syn
            .synthesize(
                "fetch ai news",
                &ExecutionContext::new(),
                "test",
                sisyphus_types::WorkMode::Standard,
                None,
            )
            .await;
        augmented_goals.lock().pop().expect("augmented goal should have been captured")
    }

    fn cluster_view(
        cluster_strategies: &[(&str, u64, u64)],
    ) -> Arc<parking_lot::RwLock<sisyphus_types::strategy_weights::FederatedStrategyWeights>> {
        let view = Arc::new(parking_lot::RwLock::new(
            sisyphus_types::strategy_weights::FederatedStrategyWeights::default(),
        ));
        if let Some((name, uses, successes)) = cluster_strategies.iter().max_by_key(|(_, u, _)| *u)
        {
            let insight = sisyphus_types::LearningInsight {
                total_samples: *uses,
                overall_success_rate: if *uses > 0 {
                    *successes as f64 / *uses as f64
                } else {
                    0.0
                },
                strategies: vec![sisyphus_types::StrategyInsight {
                    strategy: (*name).to_string(),
                    total_uses: *uses,
                    successes: *successes,
                    failures: *uses - *successes,
                    avg_duration_secs: 2.0,
                    success_rate: if *uses > 0 {
                        *successes as f64 / *uses as f64
                    } else {
                        0.0
                    },
                }],
                trend: "stable".into(),
            };
            let snapshot = sisyphus_types::strategy_weights::NodeStrategySnapshot::from_insight(
                "node-x", 1, &insight,
            );
            view.write().merge(snapshot);
        }
        view
    }

    #[tokio::test]
    async fn test_meta_synth_disabled_uses_local_only() {
        let ml = temp_meta_learner();
        record(&ml, "local-strong", 10, 10);

        let (syn, augmented_goals) = capture_syn(
            ml.clone(),
            cluster_view(&[]),
            sisyphus_types::strategy_weights::StrategySelectionConfig::default(),
        );

        let goal = last_augmented_goal(&syn, &augmented_goals).await;
        assert!(
            goal.contains("Based on historical success"),
            "关闭时必须用本地提示"
        );
        assert!(
            !goal.contains("cluster-wide evidence"),
            "关闭时不允许出现集群提示"
        );
    }

    #[tokio::test]
    async fn test_meta_synth_enabled_uses_cluster_when_local_thin() {
        let ml = temp_meta_learner();
        // 本地只有 2 个样本 < min_uses=5，证据不足。
        record(&ml, "local-thin", 2, 2);

        let (syn, augmented_goals) = capture_syn(
            ml.clone(),
            cluster_view(&[("cluster-proven", 40, 36)]),
            sisyphus_types::strategy_weights::StrategySelectionConfig {
                enabled: true,
                min_uses: 5,
            },
        );

        let goal = last_augmented_goal(&syn, &augmented_goals).await;
        assert!(
            goal.contains("cluster-wide evidence"),
            "应出现集群 hint：{}",
            goal
        );
        assert!(
            goal.contains("cluster-proven"),
            "应命中集群推荐策略：{}",
            goal
        );
        assert!(
            !goal.contains("Based on historical success"),
            "集群来源不应复用本地提示文案：{}",
            goal
        );
    }

    #[tokio::test]
    async fn test_meta_synth_enabled_local_sufficient_skips_cluster() {
        let ml = temp_meta_learner();
        // 本地 10 个样本 ≥ min_uses=5，证据充分，不应改用集群。
        record(&ml, "local-strong", 10, 10);

        let (syn, augmented_goals) = capture_syn(
            ml.clone(),
            cluster_view(&[("cluster-proven", 40, 36)]),
            sisyphus_types::strategy_weights::StrategySelectionConfig {
                enabled: true,
                min_uses: 5,
            },
        );

        let goal = last_augmented_goal(&syn, &augmented_goals).await;
        assert!(
            goal.contains("Based on historical success"),
            "本地充分时应走本地提示"
        );
        assert!(
            !goal.contains("cluster-wide evidence"),
            "本地充分时不应走集群"
        );
    }
}
