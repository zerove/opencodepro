use sisyphus_types::{SOPGraph, ExecutionContext};
use serde::{Serialize, Deserialize};
use std::sync::Arc;
use async_trait::async_trait;
use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use rayon::prelude::*;
use petgraph::graph::NodeIndex;

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
    async fn synthesize(&self, goal: &str, context: &ExecutionContext) -> Result<SynthesisResult, String>;
}

#[derive(Debug, Clone, PartialEq)]
enum ActionType {
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
fn classify_goal(goal: &str) -> Vec<ActionType> {
    let goal_lower = goal.to_lowercase();
    let mut actions = Vec::new();

    let verb_map: [(&[&str], ActionType); 5] = [
        (&["fetch", "load", "get", "read", "retrieve", "download", "pull"] as &[&str], ActionType::Fetch),
        (&["process", "transform", "convert", "parse", "compute", "analyze", "calculate", "build"], ActionType::Process),
        (&["store", "save", "write", "sink", "persist", "archive", "upload"], ActionType::Sink),
        (&["search", "find", "query", "lookup", "discover", "scan"], ActionType::Search),
        (&["validate", "check", "verify", "test", "inspect", "audit", "lint"], ActionType::Validate),
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

fn tool_id_for_action(action: &ActionType) -> &str {
    match action {
        ActionType::Fetch    => "fast_tool",
        ActionType::Process  => "fast_tool",
        ActionType::Sink     => "fast_tool",
        ActionType::Search   => "fast_tool",
        ActionType::Validate => "fast_tool",
    }
}

fn action_label(action: &ActionType) -> &str {
    match action {
        ActionType::Fetch    => "fetch",
        ActionType::Process  => "process",
        ActionType::Sink     => "sink",
        ActionType::Search   => "search",
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
    async fn synthesize(&self, goal: &str, _context: &ExecutionContext) -> Result<SynthesisResult, String> {
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
                plan_description: format!("Fan-out: {} and {} → Merge", action_label(&actions[0]), action_label(&actions[1])),
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
                graph,
                start_node: first_idx.unwrap(),
                plan_description: build_plan_description(&actions, false),
            })
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationResult {
    pub path_id: String,
    pub success_probability: f64,
    pub estimated_cost: u64,
    pub predicted_outcome: String,
}

#[async_trait]
pub trait Simulator: Send + Sync {
    async fn simulate(&self, graph: &SOPGraph, context: &ExecutionContext, sop_id: &str) -> Result<SimulationResult, String>;
}

// ---------------------------------------------------------------------------
// Reflection types
// ---------------------------------------------------------------------------

/// A single node execution record extracted from a trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedNode {
    pub node: String,
    pub status: String,
    pub output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflectionReport {
    pub original_sop_id: String,
    pub deviation_score: f64,
    pub suggested_improvements: Vec<String>,
    pub evolved_sop_structure: Option<String>,
}

#[async_trait]
pub trait Reflector: Send + Sync {
    async fn reflect(&self, trace: &str, predicted: &SimulationResult) -> Result<ReflectionReport, String>;
}

pub trait SOPEvolver: Send + Sync {
    fn evolve(&self, graph: &SOPGraph, report: &ReflectionReport) -> Result<SOPGraph, String>;
}

pub struct StandardEvolver;

impl Default for StandardEvolver {
    fn default() -> Self { Self::new() }
}

impl StandardEvolver {
    pub fn new() -> Self {
        Self
    }
}

impl SOPEvolver for StandardEvolver {
    fn evolve(&self, graph: &SOPGraph, report: &ReflectionReport) -> Result<SOPGraph, String> {
        let mut new_graph = SOPGraph::new();
        let mut node_map = std::collections::HashMap::new();

        let needs_retry = report.suggested_improvements.iter().any(|s| s.contains("retry logic"));
        let needs_fuel = report.suggested_improvements.iter().any(|s| s.contains("ResourceExhausted") || s.contains("fuel limit"));

        for node_idx in graph.graph.node_indices() {
            let node = graph.graph.node_weight(node_idx).unwrap();
            let name = node.name().to_string();
            
            let mut evolved_node: Arc<dyn sisyphus_types::SOPNode> = if let Some(wasm_node) = node.as_any().downcast_ref::<sisyphus_types::WasmSOPNode>() {
                let mut fuel = wasm_node.fuel_limit;
                if needs_fuel {
                    fuel *= 2;
                }

                Arc::new(sisyphus_types::WasmSOPNode {
                    name: wasm_node.name.clone(),
                    tool_id: wasm_node.tool_id.clone(),
                    fuel_limit: fuel,
                    executor: wasm_node.executor.clone(),
                })
            } else {
                node.clone()
            };

            if needs_retry {
                evolved_node = Arc::new(sisyphus_types::RetrySOPNode {
                    name: format!("{}_retry", name),
                    child: evolved_node,
                    max_retries: 3,
                });
            }

            let new_idx = new_graph.add_node(evolved_node);
            node_map.insert(node_idx, new_idx);
        }

        for edge in graph.graph.edge_indices() {
            let (source, target) = graph.graph.edge_endpoints(edge).unwrap();
            let new_source = node_map.get(&source).ok_or("Source node not found in evolved graph")?;
            let new_target = node_map.get(&target).ok_or("Target node not found in evolved graph")?;
            new_graph.add_edge(*new_source, *new_target);
        }

        Ok(new_graph)
    }
}

// ---------------------------------------------------------------------------
// Intel orchestrator
// ---------------------------------------------------------------------------

pub struct SisyphusIntel {
    pub simulator: Arc<dyn Simulator>,
    pub reflector: Arc<dyn Reflector>,
    pub evolver: Arc<dyn SOPEvolver>,
}

impl SisyphusIntel {
    pub fn new(simulator: Arc<dyn Simulator>, reflector: Arc<dyn Reflector>, evolver: Arc<dyn SOPEvolver>) -> Self {
        Self { simulator, reflector, evolver }
    }
}


// ---------------------------------------------------------------------------
// MonteCarloSimulator — Experience-weighted random-walk simulation
// ---------------------------------------------------------------------------
//
// Combines Monte Carlo sampling with historical experience:
// - When state_store is available, queries past execution records to bias
//   edge transition probabilities and cost estimates.
// - Without history, falls back to a uniform random walk.
// - Uses Rayon for parallel iterations.

pub struct MonteCarloSimulator {
    pub iterations: usize,
    pub state_store: Option<Arc<dyn sisyphus_types::StateStore>>,
}

impl MonteCarloSimulator {
    pub fn new(iterations: usize) -> Self {
        Self { iterations, state_store: None }
    }

    pub fn with_state_store(iterations: usize, store: Arc<dyn sisyphus_types::StateStore>) -> Self {
        Self { iterations, state_store: Some(store) }
    }
}

#[async_trait]
impl Simulator for MonteCarloSimulator {
    async fn simulate(&self, graph: &SOPGraph, _context: &ExecutionContext, sop_id: &str) -> Result<SimulationResult, String> {
        let num_nodes = graph.graph.node_count();
        if num_nodes == 0 {
            return Ok(SimulationResult {
                path_id: "empty".into(),
                success_probability: 0.0,
                estimated_cost: 0,
                predicted_outcome: "Empty graph".into(),
            });
        }

        let nodes: Vec<_> = graph.graph.node_indices().collect();

        // --- Experience prior (async, outside rayon) ---
        let (hist_success_rate, hist_avg_cost) = if let Some(ref store) = self.state_store {
            let history_key = format!("history:{}", sop_id);
            match store.get_state_json(&history_key).await {
                Ok(Some(val)) => {
                    if let Ok(hist) = serde_json::from_value::<sisyphus_types::ExecutionHistory>(val) {
                        let total = hist.records.len() as f64;
                        let successes = hist.records.iter().filter(|r| r.success).count() as f64;
                        let avg_cost = if !hist.records.is_empty() {
                            hist.records.iter().map(|r| r.cost).sum::<u64>() / hist.records.len() as u64
                        } else {
                            0
                        };
                        (successes / total.max(1.0), avg_cost)
                    } else {
                        (0.5, 100u64)
                    }
                }
                _ => (0.5, 100u64),
            }
        } else {
            (0.5, 100u64)
        };

        let (best_path, max_score, final_cost) = (0..self.iterations)
            .into_par_iter()
            .map(|_| {
                let mut rng = SmallRng::from_entropy();
                let mut path_parts: Vec<String> = Vec::new();
                let mut prob = 1.0;
                let mut cost = 0u64;

                let start = rng.gen_range(0..nodes.len());
                let mut current = nodes[start];

                loop {
                    let name = graph.graph.node_weight(current).unwrap().name().to_string();
                    path_parts.push(name);
                    let step_prob = 0.9 * hist_success_rate + 0.1 * 0.9;
                    prob *= step_prob;
                    cost += 10;

                    let neighbors: Vec<_> = graph.graph.neighbors(current).collect();
                    if neighbors.is_empty() {
                        break;
                    }
                    current = neighbors[rng.gen_range(0..neighbors.len())];
                }

                let score = prob / (cost as f64).max(1.0);
                (path_parts.join(" -> "), score, cost)
            })
            .reduce(
                || (String::new(), -1.0f64, 0u64),
                |a, b| if a.1 > b.1 { a } else { b },
            );

        let blended_cost = if final_cost > 0 { final_cost } else { hist_avg_cost };

        Ok(SimulationResult {
            path_id: best_path,
            success_probability: max_score.max(0.0),
            estimated_cost: blended_cost,
            predicted_outcome: format!("MC with history: {} iterations, prior_success={:.2}", self.iterations, hist_success_rate),
        })
    }
}

// ---------------------------------------------------------------------------
// StandardReflector — error-categorizing trace analysis
// ---------------------------------------------------------------------------

/// Error classification for targeted reflection and evolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorCategory {
    ResourceExhausted,
    Timeout,
    TypeError,
    Crash,
    Unknown,
}

impl ErrorCategory {
    /// Classify an error message string into an ErrorCategory.
    pub fn classify(output: &str) -> Self {
        if output.contains("ResourceExhausted") || output.contains("fuel") || output.contains("out of memory") {
            ErrorCategory::ResourceExhausted
        } else if output.contains("timeout") || output.contains("Timeout") || output.contains("deadline") {
            ErrorCategory::Timeout
        } else if output.contains("TypeError") || output.contains("type") || output.contains("mismatch") {
            ErrorCategory::TypeError
        } else if output.contains("panic") || output.contains("crash") || output.contains("segfault") {
            ErrorCategory::Crash
        } else {
            ErrorCategory::Unknown
        }
    }

    /// Severity weight for deviation calculation.
    pub fn severity(&self) -> f64 {
        match self {
            ErrorCategory::Crash => 1.0,
            ErrorCategory::ResourceExhausted => 0.8,
            ErrorCategory::TypeError => 0.6,
            ErrorCategory::Timeout => 0.5,
            ErrorCategory::Unknown => 0.7,
        }
    }
}

// Keep internal helpers for backward compat
fn classify_error(output: &str) -> ErrorCategory {
    ErrorCategory::classify(output)
}

fn severity_weight(category: &ErrorCategory) -> f64 {
    category.severity()
}

pub struct StandardReflector;

impl StandardReflector {
    fn parse_trace(trace: &str) -> Vec<RecordedNode> {
        trace
            .lines()
            .filter_map(|line| serde_json::from_str::<RecordedNode>(line).ok())
            .collect()
    }

    fn analyze(records: &[RecordedNode]) -> (f64, Vec<String>) {
        if records.is_empty() {
            return (0.0, vec!["No execution records available".into()]);
        }

        let total = records.len() as f64;
        let error_count = records.iter().filter(|r| r.status == "error").count();
        let failure_rate = error_count as f64 / total;

        let mut suggestions: Vec<String> = Vec::new();
        let mut severity_sum = 0.0;

        if error_count > 0 {
            let errors: Vec<&str> = records
                .iter()
                .filter(|r| r.status == "error")
                .map(|r| r.output.as_str())
                .collect();

            // Categorize errors and suggest targeted mitigations
            let mut categories: Vec<ErrorCategory> = errors.iter().map(|e| classify_error(e)).collect();
            categories.dedup();

            let avg_severity: f64 = categories.iter().map(severity_weight).sum::<f64>()
                / categories.len() as f64;
            severity_sum = avg_severity;

            for cat in &categories {
                match cat {
                    ErrorCategory::ResourceExhausted => {
                        suggestions.push(
                            "Resource exhaustion — increase fuel or split long-running nodes".into()
                        );
                    }
                    ErrorCategory::Timeout => {
                        suggestions.push(
                            "Timeout — shorten execution path or raise timeout ceiling".into()
                        );
                    }
                    ErrorCategory::TypeError => {
                        suggestions.push(
                            "Type mismatch — verify node input/output contracts".into()
                        );
                    }
                    ErrorCategory::Crash => {
                        suggestions.push(
                            "Node crash — review tool implementation for panics or segfaults".into()
                        );
                    }
                    ErrorCategory::Unknown => {
                        suggestions.push(
                            "Unknown error — add structured error handling to node".into()
                        );
                    }
                }
            }

            let error_summary = if errors.len() <= 3 {
                errors.join("; ")
            } else {
                format!("{} errors (first 3: {})", errors.len(), errors[..3].join("; "))
            };
            suggestions.push(format!("Node failures detected: {}", error_summary));
        }

        if failure_rate > 0.5 {
            suggestions.push("Critical failure rate — SOP structure may need redesign".into());
        }

        let empty_outputs = records
            .iter()
            .filter(|r| r.status == "ok" && (r.output.is_empty() || r.output == "null"))
            .count();
        if empty_outputs > records.len() / 2 {
            suggestions.push("Most nodes returned empty output — check tool implementations".into());
        }

        // Deviation blends failure rate with categorized severity
        let deviation = failure_rate * 0.6 + severity_sum * 0.3;

        if deviation < 0.3 {
            suggestions.push("SOP is performing within expected parameters".into());
        }

        (deviation.min(1.0), suggestions)
    }
}

#[async_trait]
impl Reflector for StandardReflector {
    async fn reflect(&self, trace: &str, _predicted: &SimulationResult) -> Result<ReflectionReport, String> {
        let records = Self::parse_trace(trace);
        let (deviation, suggestions) = if records.is_empty() {
            // Fallback: basic string analysis when no structured records found
            let deviation = if trace.contains("ResourceExhausted") || trace.contains("Error") {
                0.8
            } else {
                0.1
            };
            (
                deviation,
                if deviation > 0.5 {
                    vec![
                        "Add retry logic to unstable nodes".into(),
                        "Increase fuel limit".into(),
                    ]
                } else {
                    vec!["SOP is performing as expected".into()]
                },
            )
        } else {
            Self::analyze(&records)
        };

        Ok(ReflectionReport {
            original_sop_id: "current_sop".into(),
            deviation_score: deviation,
            suggested_improvements: suggestions,
            evolved_sop_structure: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_record(node: &str, status: &str, output: &str) -> RecordedNode {
        RecordedNode { node: node.into(), status: status.into(), output: output.into() }
    }

    // StandardReflector::parse_trace
    #[test]
    fn test_parse_valid_json_lines() {
        let trace = r#"{"node":"a","status":"ok","output":"done"}
{"node":"b","status":"error","output":"crash"}"#;
        let records = StandardReflector::parse_trace(trace);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].node, "a");
        assert_eq!(records[1].status, "error");
    }

    #[test]
    fn test_parse_skip_invalid() {
        let trace = "bad\n{\"node\":\"a\",\"status\":\"ok\",\"output\":\"\"}\ngarbage";
        let records = StandardReflector::parse_trace(trace);
        assert_eq!(records.len(), 1);
    }

    #[test]
    fn test_parse_empty() {
        assert!(StandardReflector::parse_trace("").is_empty());
    }

    // StandardReflector::analyze
    #[test]
    fn test_analyze_empty() {
        let (dev, s) = StandardReflector::analyze(&[]);
        assert_eq!(dev, 0.0);
        assert!(s.iter().any(|x| x.contains("No execution records")));
    }

    #[test]
    fn test_analyze_all_ok() {
        let r = vec![make_record("a", "ok", "out"), make_record("b", "ok", "out")];
        let (dev, s) = StandardReflector::analyze(&r);
        assert_eq!(dev, 0.0);
        assert!(s.iter().any(|x| x.contains("within expected parameters")));
    }

    #[test]
    fn test_analyze_errors() {
        let r = vec![make_record("a", "ok", "out"), make_record("b", "error", "boom")];
        let (dev, s) = StandardReflector::analyze(&r);
        assert!(dev > 0.3);
        assert!(s.iter().any(|x| x.contains("Node failures")));
    }

    #[test]
    fn test_analyze_resource_exhausted() {
        let r = vec![make_record("a", "error", "ResourceExhausted")];
        let (_, s) = StandardReflector::analyze(&r);
        assert!(s.iter().any(|x| x.contains("Resource exhaustion")));
    }

    #[test]
    fn test_analyze_empty_outputs() {
        let r = vec![make_record("a", "ok", ""), make_record("b", "ok", "null"), make_record("c", "ok", "real")];
        let (_, s) = StandardReflector::analyze(&r);
        assert!(s.iter().any(|x| x.contains("empty output")));
    }

    // StandardReflector::reflect (async)
    #[tokio::test]
    async fn test_reflect_structured() {
        let trace = r#"{"node":"x","status":"ok","output":"data"}"#;
        let pred = SimulationResult { path_id: "t".into(), success_probability: 0.9, estimated_cost: 10, predicted_outcome: "ok".into() };
        let report = StandardReflector.reflect(trace, &pred).await.unwrap();
        assert_eq!(report.deviation_score, 0.0);
    }

    #[tokio::test]
    async fn test_reflect_fallback() {
        let trace = "ResourceExhausted";
        let pred = SimulationResult { path_id: "t".into(), success_probability: 0.5, estimated_cost: 100, predicted_outcome: "?".into() };
        let report = StandardReflector.reflect(trace, &pred).await.unwrap();
        assert_eq!(report.deviation_score, 0.8);
    }

    // MonteCarloSimulator
    #[tokio::test]
    async fn test_monte_carlo_empty_graph() {
        let graph = SOPGraph::new();
        let sim = MonteCarloSimulator::new(10);
        let ctx = ExecutionContext::new();
        let result = sim.simulate(&graph, &ctx, "empty").await.unwrap();
        assert_eq!(result.success_probability, 0.0);
        assert_eq!(result.estimated_cost, 0);
    }

    // ErrorCategory::classify
    #[test]
    fn test_error_category_classify() {
        assert_eq!(ErrorCategory::classify("ResourceExhausted: fuel limit"), ErrorCategory::ResourceExhausted);
        assert_eq!(ErrorCategory::classify("fuel exhausted"), ErrorCategory::ResourceExhausted);
        assert_eq!(ErrorCategory::classify("out of memory error"), ErrorCategory::ResourceExhausted);
        
        assert_eq!(ErrorCategory::classify("timeout exceeded"), ErrorCategory::Timeout);
        assert_eq!(ErrorCategory::classify("deadline exceeded"), ErrorCategory::Timeout);
        assert_eq!(ErrorCategory::classify("Timeout after 30s"), ErrorCategory::Timeout);
        
        assert_eq!(ErrorCategory::classify("TypeError: mismatch"), ErrorCategory::TypeError);
        assert_eq!(ErrorCategory::classify("type mismatch"), ErrorCategory::TypeError);
        
        assert_eq!(ErrorCategory::classify("panic: index out of bounds"), ErrorCategory::Crash);
        assert_eq!(ErrorCategory::classify("segfault"), ErrorCategory::Crash);
        assert_eq!(ErrorCategory::classify("crash in module"), ErrorCategory::Crash);
        
        assert_eq!(ErrorCategory::classify("unknown error"), ErrorCategory::Unknown);
    }

    #[test]
    fn test_error_category_severity() {
        assert_eq!(ErrorCategory::Crash.severity(), 1.0);
        assert_eq!(ErrorCategory::ResourceExhausted.severity(), 0.8);
        assert_eq!(ErrorCategory::TypeError.severity(), 0.6);
        assert_eq!(ErrorCategory::Timeout.severity(), 0.5);
        assert_eq!(ErrorCategory::Unknown.severity(), 0.7);
    }
}
