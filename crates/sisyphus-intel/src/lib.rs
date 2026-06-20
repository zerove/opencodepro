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

pub struct TemplateSynthesizer {
    pub tool_executor: Arc<dyn sisyphus_types::ToolExecutor>,
}

impl TemplateSynthesizer {
    pub fn new(tool_executor: Arc<dyn sisyphus_types::ToolExecutor>) -> Self {
        Self { tool_executor }
    }
}

#[async_trait]
impl FlowSynthesizer for TemplateSynthesizer {
    async fn synthesize(&self, goal: &str, _context: &ExecutionContext) -> Result<SynthesisResult, String> {
        let mut graph = SOPGraph::new();
        
        if goal.contains("complex") {
            let n1 = graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
                name: "fetch".into(),
                tool_id: "fast_tool".into(),
                fuel_limit: 1_000_000,
                executor: self.tool_executor.clone(),
            }));
            let n2 = graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
                name: "process".into(),
                tool_id: "fast_tool".into(),
                fuel_limit: 1_000_000,
                executor: self.tool_executor.clone(),
            }));
            let n3 = graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
                name: "sink".into(),
                tool_id: "fast_tool".into(),
                fuel_limit: 1_000_000,
                executor: self.tool_executor.clone(),
            }));
            graph.add_edge(n1, n2);
            graph.add_edge(n2, n3);
            
            Ok(SynthesisResult {
                graph,
                start_node: n1,
                plan_description: "Linear Pipeline: Fetch -> Process -> Sink".into(),
            })
        } else {
            let n1 = graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
                name: "default".into(),
                tool_id: "fast_tool".into(),
                fuel_limit: 1_000_000,
                executor: self.tool_executor.clone(),
            }));
            
            Ok(SynthesisResult {
                graph,
                start_node: n1,
                plan_description: "Simple Single-Node Task".into(),
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


pub struct ExperienceSimulator {
    pub state_store: Arc<dyn sisyphus_types::StateStore>,
}

impl ExperienceSimulator {
    pub fn new(state_store: Arc<dyn sisyphus_types::StateStore>) -> Self {
        Self { state_store }
    }
}

#[async_trait]
impl Simulator for ExperienceSimulator {
    async fn simulate(&self, _graph: &SOPGraph, _context: &ExecutionContext, sop_id: &str) -> Result<SimulationResult, String> {
        let history_key = format!("history:{}", sop_id);
        let history_val = self.state_store.get_state_json(&history_key).await
            .map_err(|e| e.to_string())?;
        
        if let Some(val) = history_val {
            let hist: sisyphus_types::ExecutionHistory = serde_json::from_value(val).map_err(|e| e.to_string())?;
            if hist.records.is_empty() {
                return Err("No historical records available".into());
            }
            
            let total = hist.records.len() as f64;
            let successes = hist.records.iter().filter(|r| r.success).count() as f64;
            let avg_cost = hist.records.iter().map(|r| r.cost).sum::<u64>() / hist.records.len() as u64;
            
            Ok(SimulationResult {
                path_id: "experience-driven".into(),
                success_probability: successes / total,
                estimated_cost: avg_cost,
                predicted_outcome: format!("Based on {} historical runs", hist.records.len()),
            })
        } else {
            // Fallback: If no experience, return a neutral prediction instead of error
            Ok(SimulationResult {
                path_id: "cold-start".into(),
                success_probability: 0.5,
                estimated_cost: 100,
                predicted_outcome: "Cold start: no historical data".into(),
            })
        }
    }
}

// ---------------------------------------------------------------------------
// MonteCarloSimulator — Rayon-parallel random-walk simulation
// ---------------------------------------------------------------------------

pub struct MonteCarloSimulator {
    pub iterations: usize,
}

impl MonteCarloSimulator {
    pub fn new(iterations: usize) -> Self {
        Self { iterations }
    }
}

#[async_trait]
impl Simulator for MonteCarloSimulator {
    async fn simulate(&self, graph: &SOPGraph, _context: &ExecutionContext, _sop_id: &str) -> Result<SimulationResult, String> {
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

        // Each iteration is an independent random walk → trivial parallelism.
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
                    prob *= 0.9;
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

        Ok(SimulationResult {
            path_id: best_path,
            success_probability: max_score.max(0.0),
            estimated_cost: final_cost,
            predicted_outcome: "Optimal Path Found".into(),
        })
    }
}

// ---------------------------------------------------------------------------
// StandardReflector — data-driven trace analysis
// ---------------------------------------------------------------------------

pub struct StandardReflector;

impl StandardReflector {
    /// Parse a trace string into structured node records.
    /// Supports JSON-line format: one `RecordedNode` per line.
    fn parse_trace(trace: &str) -> Vec<RecordedNode> {
        trace
            .lines()
            .filter_map(|line| serde_json::from_str::<RecordedNode>(line).ok())
            .collect()
    }

    /// Analyse structured records and produce (deviation, suggestions).
    fn analyze(records: &[RecordedNode]) -> (f64, Vec<String>) {
        if records.is_empty() {
            return (0.0, vec!["No execution records available".into()]);
        }

        let total = records.len() as f64;
        let error_count = records.iter().filter(|r| r.status == "error").count();
        let failure_rate = error_count as f64 / total;

        // Deviation is a weighted combination of failure rate and error severity
        let deviation = failure_rate * 0.7
            + (error_count as f64 / total).min(1.0) * 0.3;

        let mut suggestions: Vec<String> = Vec::new();

        if error_count > 0 {
            let errors: Vec<&str> = records
                .iter()
                .filter(|r| r.status == "error")
                .map(|r| r.output.as_str())
                .collect();

            let error_summary = if errors.len() <= 3 {
                errors.join("; ")
            } else {
                format!(
                    "{} errors (first 3: {})",
                    errors.len(),
                    errors[..3].join("; ")
                )
            };
            suggestions.push(format!("Node failures detected: {}", error_summary));

            if errors.iter().any(|e| e.contains("ResourceExhausted")) {
                suggestions.push(
                    "Resource exhaustion detected — consider increasing fuel limits or splitting long-running nodes"
                        .into(),
                );
            }
            if errors.iter().any(|e| e.contains("timeout") || e.contains("Timeout")) {
                suggestions.push("Timeouts detected — consider shorter execution paths or higher timeouts".into());
            }
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
}
