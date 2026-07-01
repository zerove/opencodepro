use async_trait::async_trait;
use petgraph::graph::NodeIndex;
use sisyphus_types::{CompositeGraph, ExecutionContext, SOPGraph, SOPGraphConfig};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Flow Synthesis
// ---------------------------------------------------------------------------

pub struct SynthesisResult {
    pub graph: SOPGraph,
    pub start_node: NodeIndex,
    pub plan_description: String,
}

#[async_trait]
pub trait FlowSynthesizer: Send + Sync {
    async fn synthesize(
        &self,
        goal: &str,
        context: &ExecutionContext,
        mode: sisyphus_types::WorkMode,
    ) -> Result<SynthesisResult, String>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum ActionType {
    Fetch,
    Process,
    Sink,
    Search,
    Validate,
}

/// Goal-driven SOP synthesis engine.
///
/// Parses a natural-language goal into a DAG of typed actions:
///   - Fetch/Read/Get → fetch node
///   - Process/Transform/Convert → process node
///   - Store/Save/Write → sink node
///   - Search/Find/Query → search node
///   - Validate/Check/Verify → validate node
///
/// Multiple actions are chained as a linear pipeline in detection order.
/// When "parallel" appears in the goal, the first two actions fan out
/// and converge into a single merge node.
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
        _mode: sisyphus_types::WorkMode,
    ) -> Result<SynthesisResult, String> {
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
                graph,
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
                graph,
                start_node: n,
                plan_description: format!("Single-node task: {}", label),
            });
        }

        if is_parallel && actions.len() >= 2 {
            // Fan-out: multiple independent nodes, converge into merge node
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
                graph,
                start_node: n1,
                plan_description: format!(
                    "Fan-out: {} and {} → Merge",
                    action_label(&actions[0]),
                    action_label(&actions[1])
                ),
            })
        } else {
            // Linear pipeline
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
                start_node: first_idx.ok_or_else(|| "actions must have at least 2 entries".to_string())?,
                graph,
                plan_description: build_plan_description(&actions, false),
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Composite (parallel) synthesis — multiple goals → CompositeGraph
// ---------------------------------------------------------------------------

/// Synthesize multiple goals into a single CompositeGraph.
/// Each goal becomes an independent sub-graph with no cross-dependencies,
/// so they can be executed in parallel by CompositeExecutor.
pub async fn synthesize_composite(
    goals: &[&str],
    synthesizer: &dyn FlowSynthesizer,
    ctx: &ExecutionContext,
) -> Result<CompositeGraph, String> {
    let mut composite = CompositeGraph::new();
    for (i, goal) in goals.iter().enumerate() {
        let sop_id = format!("goal_{}", i);
        let result = synthesizer.synthesize(goal, ctx, sisyphus_types::WorkMode::Standard).await?;
        let config = SOPGraphConfig::from_graph(&result.graph);
        composite.add_graph(&sop_id, config);
    }
    Ok(composite)
}
