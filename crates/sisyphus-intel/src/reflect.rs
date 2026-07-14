use crate::fuel::FuelPricer;
use crate::simulation::SimulationResult;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sisyphus_metrics::{record_evolution, record_reflection};
use sisyphus_types::SOPGraph;
use sisyphus_tools_llm::metis::LlmClient;
use std::sync::Arc;
use std::time::Instant;

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
    pub records: Vec<RecordedNode>,
}

#[async_trait]
pub trait Reflector: Send + Sync {
    async fn reflect(
        &self,
        trace: &str,
        predicted: &SimulationResult,
        experiences: Option<&[sisyphus_types::experience::ExperienceTuple]>,
    ) -> Result<ReflectionReport, String>;
}

#[async_trait]
pub trait SOPEvolver: Send + Sync {
    async fn evolve(
        &self,
        graph: Arc<SOPGraph>,
        report: &ReflectionReport,
        prediction: &SimulationResult,
        insights: Option<&str>,
    ) -> Result<Arc<SOPGraph>, String>;
}

// ---------------------------------------------------------------------------
// StandardEvolver — retry wrapping + fuel evolution
// ---------------------------------------------------------------------------

pub struct StandardEvolver {
    fuel_pricer: Arc<dyn FuelPricer>,
}

impl Default for StandardEvolver {
    fn default() -> Self {
        Self::new()
    }
}

impl StandardEvolver {
    pub fn new() -> Self {
        Self {
            fuel_pricer: Arc::new(crate::SimpleFuelPricer),
        }
    }

    pub fn with_fuel_pricer(fuel_pricer: Arc<dyn FuelPricer>) -> Self {
        Self { fuel_pricer }
    }
}

#[async_trait]
impl SOPEvolver for StandardEvolver {
    async fn evolve(
        &self,
        graph: Arc<SOPGraph>,
        report: &ReflectionReport,
        prediction: &SimulationResult,
        _insights: Option<&str>,
    ) -> Result<Arc<SOPGraph>, String> {
        let span = tracing::info_span!(
            "intel.evolve",
            graph_nodes = graph.graph.node_count(),
            deviation = report.deviation_score
        );
        let _enter = span.enter();

        let start = Instant::now();
        let mut new_graph = SOPGraph::new();
        let mut node_map = std::collections::HashMap::new();

        let needs_retry = report
            .suggested_improvements
            .iter()
            .any(|s| s.contains("retry logic"));
        let needs_fuel = report
            .suggested_improvements
            .iter()
            .any(|s| s.contains("ResourceExhausted") || s.contains("fuel limit"));

        tracing::debug!(needs_retry, needs_fuel, "Evolution analysis");

        for node_idx in graph.graph.node_indices() {
            let node = graph
                .graph
                .node_weight(node_idx)
                 .ok_or_else(|| "invalid node index in graph".to_string())?;
            let name = node.name().to_string();

            let mut evolved_node: Arc<dyn sisyphus_types::SOPNode> = if let Some(wasm_node) =
                node.as_any().downcast_ref::<sisyphus_types::WasmSOPNode>()
            {
                let mut fuel = wasm_node.fuel_limit;
                if needs_fuel {
                    // Use adaptive fuel pricing
                    fuel = self.fuel_pricer.price_fuel(&name, fuel, prediction);
                    tracing::debug!(node = %name, new_fuel = fuel, "Fuel adjusted");
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
                tracing::debug!(node = %name, "Retry wrapper added");
            }

            let new_idx = new_graph.add_node(evolved_node);
            node_map.insert(node_idx, new_idx);
        }

        for edge in graph.graph.edge_indices() {
            let (source, target) = graph
                .graph
                .edge_endpoints(edge)
                 .ok_or_else(|| "invalid edge endpoint in graph".to_string())?;
            let new_source = node_map
                .get(&source)
                .ok_or("Source node not found in evolved graph")?;
            let new_target = node_map
                .get(&target)
                .ok_or("Target node not found in evolved graph")?;
            new_graph.add_edge(*new_source, *new_target);
        }

        let duration = start.elapsed();
        let has_evolved = needs_retry || needs_fuel;
        tracing::info!(
            evolved = has_evolved,
            needs_retry,
            needs_fuel,
            duration_ms = duration.as_millis(),
            "Evolution completed"
        );

        record_evolution(
            "current_sop",
            has_evolved,
            duration.as_secs_f64(),
            needs_fuel,
        );

        Ok(Arc::new(new_graph))
    }
}

// ---------------------------------------------------------------------------
// Error classification for targeted reflection and evolution.
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
        if output.contains("ResourceExhausted")
            || output.contains("fuel")
            || output.contains("out of memory")
        {
            ErrorCategory::ResourceExhausted
        } else if output.contains("timeout")
            || output.contains("Timeout")
            || output.contains("deadline")
        {
            ErrorCategory::Timeout
        } else if output.contains("TypeError")
            || output.contains("type")
            || output.contains("mismatch")
        {
            ErrorCategory::TypeError
        } else if output.contains("panic")
            || output.contains("crash")
            || output.contains("segfault")
        {
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

pub struct LlmReflector {
    pub llm_client: Arc<dyn LlmClient>,
}

impl LlmReflector {
    pub fn new(llm_client: Arc<dyn LlmClient>) -> Self {
        Self { llm_client }
    }
}

#[async_trait]
impl Reflector for LlmReflector {
    async fn reflect(
        &self,
        trace: &str,
        predicted: &SimulationResult,
        experiences: Option<&[sisyphus_types::experience::ExperienceTuple]>,
    ) -> Result<ReflectionReport, String> {
        let span = tracing::info_span!("intel.reflect.llm", trace_lines = trace.lines().count());
        let _enter = span.enter();

        let start = Instant::now();
        
        let exp_context = if let Some(exps) = experiences {
            let mut hints = Vec::new();
            for exp in exps {
                hints.push(format!(
                    "Task: {}, Outcome: {}, Deviation: {:.4}{}", 
                    exp.context_text, 
                    if exp.outcome_success { "Success" } else { "Failure" }, 
                    exp.deviation_score,
                    exp.insight.as_ref().map(|i| format!(" | Insight: {}", i)).unwrap_or_default()
                ));
            }
            format!("Historical Context (Similar Tasks):\n{}", hints.join("\n"))
        } else {
            "No historical context available".to_string()
        };

        let prompt = format!(
            "Analyze the following execution trace and compare it with the predicted outcome.\
            \n\n{}\n\nTrace:\n{}\n\nPredicted Outcome: {}\n\n\
            Please provide:\
            1. A deviation score (0.0 to 1.0).\
            2. A list of specific suggested improvements (e.g., 'Add retry to node X', 'Increase fuel for node Y').\
            3. An analysis of the root cause of any failures.\
            Return as JSON: {{ \"deviation\": f64, \"suggestions\": [string], \"analysis\": string }}",
            exp_context, trace, predicted.predicted_outcome
        );

        let response = self.llm_client.generate(&prompt).await
            .map_err(|e| format!("LLM reflection failed: {e}"))?;

        let parsed: serde_json::Value = serde_json::from_str(&response)
            .map_err(|e| format!("Failed to parse LLM reflection JSON: {e}"))?;

        let deviation = parsed["deviation"].as_f64().unwrap_or(0.5);
        let suggestions: Vec<String> = parsed["suggestions"]
            .as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();

        let duration = start.elapsed();
        tracing::info!(
            deviation = deviation,
            suggestions = suggestions.len(),
            duration_ms = duration.as_millis(),
            "LLM Reflection completed"
        );

        record_reflection("current_sop", deviation, duration.as_secs_f64());

        Ok(ReflectionReport {
            original_sop_id: "current_sop".into(),
            deviation_score: deviation,
            suggested_improvements: suggestions,
            evolved_sop_structure: None,
            records: StandardReflector::parse_trace(trace),
        })
    }
}

pub struct HybridReflector {
    pub standard: Arc<dyn Reflector>,
    pub llm: Arc<dyn Reflector>,
}

impl HybridReflector {
    pub fn new(standard: Arc<dyn Reflector>, llm: Arc<dyn Reflector>) -> Self {
        Self { standard, llm }
    }
}

#[async_trait]
impl Reflector for HybridReflector {
    async fn reflect(
        &self,
        trace: &str,
        predicted: &SimulationResult,
        experiences: Option<&[sisyphus_types::experience::ExperienceTuple]>,
    ) -> Result<ReflectionReport, String> {
        let span = tracing::info_span!("intel.reflect.hybrid", trace_lines = trace.lines().count());
        let _enter = span.enter();

        let start = Instant::now();
        
        // Run both reflectors in parallel
        let (std_res, llm_res) = tokio::join!(
            self.standard.reflect(trace, predicted, experiences),
            self.llm.reflect(trace, predicted, experiences)
        );

        let std_report = std_res.map_err(|e| format!("Standard reflection failed: {e}"))?;
        let llm_report = llm_res.map_err(|e| format!("LLM reflection failed: {e}"))?;

        // Merge scores: weighted average (LLM is usually more nuanced, Standard is more objective)
        let deviation = (std_report.deviation_score * 0.4) + (llm_report.deviation_score * 0.6);
        
        // Merge improvements: Deduplicate and combine
        let mut all_improvements = std_report.suggested_improvements;
        for imp in llm_report.suggested_improvements {
            if !all_improvements.contains(&imp) {
                all_improvements.push(imp);
            }
        }

        let duration = start.elapsed();
        tracing::info!(
            deviation = deviation,
            suggestions = all_improvements.len(),
            duration_ms = duration.as_millis(),
            "Hybrid Reflection completed"
        );

        record_reflection("current_sop", deviation, duration.as_secs_f64());

        Ok(ReflectionReport {
            original_sop_id: std_report.original_sop_id,
            deviation_score: deviation,
            suggested_improvements: all_improvements,
            evolved_sop_structure: None,
            records: std_report.records, // Use structured records from StandardReflector
        })
    }
}

pub struct LlmEvolver {
    pub llm_client: Arc<dyn LlmClient>,
    pub topo_evolver: Arc<crate::topological_evolver::TopologicalSOPEvolver>,
    pub tool_executor: Arc<dyn sisyphus_types::ToolExecutor>,
}



impl LlmEvolver {
    pub fn new(
        llm_client: Arc<dyn LlmClient>,
        topo_evolver: Arc<crate::topological_evolver::TopologicalSOPEvolver>,
        tool_executor: Arc<dyn sisyphus_types::ToolExecutor>,
    ) -> Self {
        Self {
            llm_client,
            topo_evolver,
            tool_executor,
        }
    }
}

#[async_trait]
impl SOPEvolver for LlmEvolver {
    async fn evolve(
        &self,
        graph: Arc<SOPGraph>,
        report: &ReflectionReport,
        _prediction: &SimulationResult,
        insights: Option<&str>,
    ) -> Result<Arc<SOPGraph>, String> {
        let rt = tokio::runtime::Handle::current();
        
        let prompt = format!(
            "Given the following SOP structure and suggested improvements, generate a list of EvolutionOps.\
            SOP Nodes: {:?}\
            Improvements: {:?}\
            Insights: {:?}\
            
            Available Ops: \
            - AdjustFuel {{ node_name, delta }}\
            - AddRetry {{ node_name, max_retries }}\
            - InsertNode {{ after_node, new_name, tool_id }}\
            - RemoveNode {{ node_name }}\
            - SwapNodes {{ node_a, node_b }}\
            
            Return a JSON array of ops.",
            graph.graph.node_indices().map(|i| graph.graph.node_weight(i).map(|n| n.name().to_string())).collect::<Vec<_>>(),
            report.suggested_improvements,
            insights.unwrap_or("None")
        );

        let response = self.llm_client.generate(&prompt).await
            .map_err(|e| format!("LLM evolution failed: {e}"))?;

        let ops: Vec<sisyphus_types::EvolutionOp> = serde_json::from_str(&response)
            .map_err(|e| format!("Failed to parse evolution ops JSON: {e}. Response: {response}"))?;

        self.topo_evolver.apply_ops(&graph, &ops, self.tool_executor.clone()).map(|g| Arc::new(g)).map_err(|e| e.to_string())
    }
}

// ---------------------------------------------------------------------------
// StandardReflector — error-categorizing trace analysis
// ---------------------------------------------------------------------------

pub struct StandardReflector;

impl StandardReflector {
    pub(crate) fn parse_trace(trace: &str) -> Vec<RecordedNode> {
        trace
            .lines()
            .filter_map(|line| serde_json::from_str::<RecordedNode>(line).ok())
            .collect()
    }

    pub(crate) fn analyze(records: &[RecordedNode]) -> (f64, Vec<String>) {
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
            let mut categories: Vec<ErrorCategory> =
                errors.iter().map(|e| ErrorCategory::classify(e)).collect();
            categories.dedup();

            let avg_severity: f64 =
                categories.iter().map(|cat| cat.severity()).sum::<f64>() / categories.len() as f64;
            severity_sum = avg_severity;

            for cat in &categories {
                match cat {
                    ErrorCategory::ResourceExhausted => {
                        suggestions.push(
                            "Resource exhaustion — increase fuel or split long-running nodes"
                                .into(),
                        );
                    }
                    ErrorCategory::Timeout => {
                        suggestions.push(
                            "Timeout — shorten execution path or raise timeout ceiling".into(),
                        );
                    }
                    ErrorCategory::TypeError => {
                        suggestions
                            .push("Type mismatch — verify node input/output contracts".into());
                    }
                    ErrorCategory::Crash => {
                        suggestions.push(
                            "Node crash — review tool implementation for panics or segfaults"
                                .into(),
                        );
                    }
                    ErrorCategory::Unknown => {
                        suggestions
                            .push("Unknown error — add structured error handling to node".into());
                    }
                }
            }

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
        }

        if failure_rate > 0.5 {
            suggestions.push("Critical failure rate — SOP structure may need redesign".into());
        }

        let empty_outputs = records
            .iter()
            .filter(|r| r.status == "ok" && (r.output.is_empty() || r.output == "null"))
            .count();
        if empty_outputs > records.len() / 2 {
            suggestions
                .push("Most nodes returned empty output — check tool implementations".into());
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
    async fn reflect(
        &self,
        trace: &str,
        _predicted: &SimulationResult,
        _experiences: Option<&[sisyphus_types::experience::ExperienceTuple]>,
    ) -> Result<ReflectionReport, String> {
        let span = tracing::info_span!("intel.reflect", trace_lines = trace.lines().count());
        let _enter = span.enter();

        let start = Instant::now();
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

        let duration = start.elapsed();
        tracing::info!(
            deviation = deviation,
            suggestions = suggestions.len(),
            duration_ms = duration.as_millis(),
            "Reflection completed"
        );

        record_reflection("current_sop", deviation, duration.as_secs_f64());

        Ok(ReflectionReport {
            original_sop_id: "current_sop".into(),
            deviation_score: deviation,
            suggested_improvements: suggestions,
            evolved_sop_structure: None,
            records,
        })
    }
}

/// Classifies execution errors into a numeric severity weight.
fn severity_weight(e: &str) -> f64 {
    if e.contains("ResourceExhausted") {
        0.8
    } else if e.contains("Timeout") {
        0.5
    } else if e.contains("Internal") {
        0.3
    } else {
        0.2
    }
}

#[cfg(test)]
#[path = "reflect_tests.rs"]
mod tests;
