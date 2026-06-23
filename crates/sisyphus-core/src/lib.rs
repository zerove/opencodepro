use petgraph::algo::toposort;
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use thiserror::Error;

use sisyphus_intel::SisyphusIntel;
use sisyphus_mem::MfsStore;
use sisyphus_metrics::record_cognitive_iteration;
pub use sisyphus_types::{
    CompositeGraph, DataMapping, ExecutionContext, ExecutionEvent, GraphDependency, NodeOutput, SOPGraph,
    SOPGraphConfig, StateStore, ToolExecutor, WasmSOPNode,
};

#[derive(Error, Debug)]
pub enum CoreError {
    #[error("SOPGraph contains cycles")]
    CycleDetected,
    #[error("Cross-graph dependency cycle detected")]
    DependencyCycleDetected,
    #[error("Missing source node")]
    MissingSourceNode,
    #[error("Missing target node")]
    MissingTargetNode,
    #[error("MFS save error: {0}")]
    MfsSaveError(String),
    #[error("MFS load error: {0}")]
    MfsLoadError(String),
    #[error("SOP {0} not found")]
    SopNotFound(String),
    #[error("Execution failed to produce result")]
    NoResultProduced,
    #[error("Graph {0} not found in composite")]
    GraphNotFound(String),
    #[error("Graph {0} has no nodes")]
    GraphEmpty(String),
    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<CoreError> for String {
    fn from(err: CoreError) -> Self {
        err.to_string()
    }
}

pub struct Executor;

impl Default for Executor {
    fn default() -> Self {
        Self::new()
    }
}

impl Executor {
    pub fn new() -> Self {
        Self
    }

    /// Execute a SOP graph, optionally starting with a pre-populated context
    /// (e.g. with cross-graph dependency outputs injected).
    pub async fn execute_with_context(
        &self,
        sop_graph: &SOPGraph,
        _start_node: NodeIndex,
        initial_ctx: Option<Arc<ExecutionContext>>,
        event_tx: Option<mpsc::UnboundedSender<ExecutionEvent>>,
    ) -> Result<Arc<ExecutionContext>, CoreError> {
        let span = tracing::info_span!(
            "executor.execute",
            graph_nodes = sop_graph.graph.node_count()
        );
        let _enter = span.enter();

        let start = Instant::now();
        if toposort(&sop_graph.graph, None).is_err() {
            tracing::error!("SOPGraph contains cycles");
            return Err(CoreError::CycleDetected);
        }
        let ctx = initial_ctx.unwrap_or_else(|| Arc::new(ExecutionContext::new()));

        let mut join_set = JoinSet::new();
        let mut in_degree = HashMap::new();
        let nodes = sop_graph.graph.node_indices();
        for node in nodes {
            let degree = sop_graph
                .graph
                .edges_directed(node, petgraph::Direction::Incoming)
                .count();
            in_degree.insert(node, degree);
        }

        tracing::debug!(
            "Starting DAG execution with {} ready nodes",
            in_degree.values().filter(|&&d| d == 0).count()
        );

        for (&node, &degree) in in_degree.iter() {
            if degree == 0 {
                let node_ref = Arc::clone(sop_graph.graph.node_weight(node).unwrap());
                let ctx_ref = Arc::clone(&ctx);
                let tx = event_tx.clone();
                join_set.spawn(async move {
                    let node_span = tracing::info_span!("executor.node", node = node_ref.name());
                    let _enter = node_span.enter();
                    let name = node_ref.name().to_string();
                    
                    if let Some(ref tx) = tx {
                        let _ = tx.send(ExecutionEvent::NodeStarted { node_name: name.clone() });
                    }

                    let result = node_ref.execute(&ctx_ref).await;
                    
                    if let Some(ref tx) = tx {
                        match &result {
                            Ok(out) => {
                                let _ = tx.send(ExecutionEvent::NodeCompleted { 
                                    node_name: name.clone(), 
                                    output: out.clone() 
                                });
                            }
                            Err(e) => {
                                let _ = tx.send(ExecutionEvent::NodeFailed { 
                                    node_name: name.clone(), 
                                    error: e.clone() 
                                });
                            }
                        }
                    }

                    tracing::debug!(node = %name, "Node execution completed");
                    (node, name, result)
                });
            }
        }

                while let Some(res) = join_set.join_next().await {
                    let (node, name, result) = res.map_err(|e| CoreError::Internal(e.to_string()))?;
                    let output = match result {
                        Ok(out) => out,
                        Err(e) => {
                            tracing::warn!(node = %name, error = %e, "Node execution failed");
                            Value::String(e)
                        }
                    };
                    ctx.set_result(name.clone(), output);
                    tracing::debug!(node = %name, "Node result stored");


            for edge in sop_graph.graph.edges(node) {
                let target = edge.target();
                if let Some(degree) = in_degree.get_mut(&target) {
                    *degree -= 1;
                    if *degree == 0 {
                        let node_ref = Arc::clone(sop_graph.graph.node_weight(target).unwrap());
                        let ctx_ref = Arc::clone(&ctx);
                        let tx = event_tx.clone();
                        join_set.spawn(async move {
                            let node_span =
                                tracing::info_span!("executor.node", node = node_ref.name());
                            let _enter = node_span.enter();
                            let name = node_ref.name().to_string();

                            if let Some(ref tx) = tx {
                                let _ = tx.send(ExecutionEvent::NodeStarted { node_name: name.clone() });
                            }

                            let result = node_ref.execute(&ctx_ref).await;

                            if let Some(ref tx) = tx {
                                match &result {
                                    Ok(out) => {
                                        let _ = tx.send(ExecutionEvent::NodeCompleted { 
                                            node_name: name.clone(), 
                                            output: out.clone() 
                                        });
                                    }
                                    Err(e) => {
                                        let _ = tx.send(ExecutionEvent::NodeFailed { 
                                            node_name: name.clone(), 
                                            error: e.clone() 
                                        });
                                    }
                                }
                            }

                            tracing::debug!(node = %name, "Node execution completed");
                            (target, name, result)
                        });
                    }
                }
            }
        }

        let duration = start.elapsed();
        tracing::info!(
            duration_ms = duration.as_millis(),
            "DAG execution completed"
        );
        Ok(ctx)
    }

    /// Execute a SOP graph (backward-compatible wrapper).
    pub async fn execute(
        &self,
        sop_graph: &SOPGraph,
        start_node: NodeIndex,
    ) -> Result<Arc<ExecutionContext>, CoreError> {
        self.execute_with_context(sop_graph, start_node, None, None).await
    }
}

// ---------------------------------------------------------------------------
// Cognitive execution — iterative predict → execute → reflect → evolve
// ---------------------------------------------------------------------------

const MAX_COGNITIVE_ITERATIONS: usize = 3;
const CONVERGENCE_THRESHOLD: f64 = 0.3;

/// A single iteration in a cognitive execution cycle.
pub struct CognitiveIteration {
    pub iteration: usize,
    pub prediction: sisyphus_intel::SimulationResult,
    pub deviation_score: f64,
    pub suggestions: Vec<String>,
    pub evolved: bool,
}

/// Full report from a recursive cognitive execution.
pub struct CognitiveReport {
    pub iterations: usize,
    pub converged: bool,
    pub final_deviation: f64,
    pub history: Vec<CognitiveIteration>,
}

// ---------------------------------------------------------------------------
// Composite Execution — cross-graph dependency resolution
// ---------------------------------------------------------------------------

/// Resolves execution order for a CompositeGraph via topological sort
/// over the dependency graph.
pub struct DependencyResolver;

impl Default for DependencyResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl DependencyResolver {
    pub fn new() -> Self {
        Self
    }

    /// Given a CompositeGraph, return sop_ids in execution order.
    /// Uses petgraph to build a dependency DAG and performs topological sort.
    pub fn resolve(composite: &CompositeGraph) -> Result<Vec<String>, CoreError> {
        let mut dep_graph = petgraph::graph::DiGraph::<String, ()>::new();
        let mut node_map = HashMap::new();

        // Ensure all referenced sop_ids exist as nodes in the dep graph
        for sop_id in composite.graphs.keys() {
            if !node_map.contains_key(sop_id) {
                let idx = dep_graph.add_node(sop_id.clone());
                node_map.insert(sop_id.clone(), idx);
            }
        }
        for dep in &composite.dependencies {
            if !node_map.contains_key(&dep.source_sop_id) {
                let idx = dep_graph.add_node(dep.source_sop_id.clone());
                node_map.insert(dep.source_sop_id.clone(), idx);
            }
            if !node_map.contains_key(&dep.target_sop_id) {
                let idx = dep_graph.add_node(dep.target_sop_id.clone());
                node_map.insert(dep.target_sop_id.clone(), idx);
            }
        }

        // Add edges: source -> target (source executes BEFORE target)
        for dep in &composite.dependencies {
            let src = node_map
                .get(&dep.source_sop_id)
                .ok_or(CoreError::MissingSourceNode)?;
            let tgt = node_map
                .get(&dep.target_sop_id)
                .ok_or(CoreError::MissingTargetNode)?;
            dep_graph.add_edge(*src, *tgt, ());
        }

        // Topological sort
        let order = toposort(&dep_graph, None)
            .map_err(|_| CoreError::DependencyCycleDetected)?;

        Ok(order
            .into_iter()
            .filter_map(|idx| dep_graph.node_weight(idx).cloned())
            .filter(|id| composite.graphs.contains_key(id))
            .collect())
    }
}

/// Output from a composite execution run.
pub struct CompositeOutput {
    /// sop_id -> (node_output, report_string) for each executed graph.
    pub results: HashMap<String, (NodeOutput, String)>,
    /// Order in which graphs were executed.
    pub execution_order: Vec<String>,
}

pub struct SisyphusKernel {
    pub executor: Executor,
    pub state_store: Arc<MfsStore>,
    pub tool_executor: Arc<dyn ToolExecutor>,
    pub intel: Arc<SisyphusIntel>,
    pub synthesizer: Arc<dyn sisyphus_intel::FlowSynthesizer>,
}

impl SisyphusKernel {
    pub fn new(
        state_store: Arc<MfsStore>,
        tool_executor: Arc<dyn ToolExecutor>,
        intel: Arc<SisyphusIntel>,
        synthesizer: Arc<dyn sisyphus_intel::FlowSynthesizer>,
    ) -> Self {
        Self {
            executor: Executor::new(),
            state_store,
            tool_executor,
            intel,
            synthesizer,
        }
    }

    pub async fn save_sop(&self, sop_id: &str, graph: &SOPGraph) -> Result<(), CoreError> {
        let config = sisyphus_types::SOPGraphConfig::from_graph(graph);
        let data = serde_json::to_string(&config).map_err(|e| CoreError::Internal(e.to_string()))?;
        self.state_store
            .set_state_json(&format!("sop:{}", sop_id), serde_json::Value::String(data))
            .await
            .map_err(CoreError::MfsSaveError)
    }

    pub async fn load_sop(&self, sop_id: &str) -> Result<SOPGraph, CoreError> {
        let data_val = self
            .state_store
            .get_state_json(&format!("sop:{}", sop_id))
            .await
            .map_err(CoreError::MfsLoadError)?;

        let data = data_val
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .ok_or_else(|| CoreError::SopNotFound(sop_id.to_string()))?;
        let config: sisyphus_types::SOPGraphConfig =
            serde_json::from_str(&data).map_err(|e| CoreError::Internal(e.to_string()))?;
        Ok(SOPGraph::from_config(&config, self.tool_executor.clone()))
    }

    /// Execute multiple natural-language goals as a composite workflow.
    ///
    /// Each goal is synthesized independently into a sub-graph, then all
    /// are executed as a CompositeGraph (no cross-dependencies, so they
    /// run as independent parallel pipelines).
    pub async fn execute_goals(&self, goals: &[&str]) -> Result<CompositeOutput, String> {
        let ctx = ExecutionContext::new();
        let composite =
            sisyphus_intel::synthesize_composite(goals, &*self.synthesizer, &ctx).await?;
        self.execute_composite(&composite, None).await
    }

    /// Execute a natural-language goal through the full cognitive pipeline.
    ///
    /// 1. Synthesize a SOP graph from the goal string
    /// 2. Execute with recursive cognition (auto-retry on high deviation)
    /// 3. Return the result plus a detailed cognitive report
    pub async fn execute_goal(
        &self,
        goal: &str,
        sop_id: &str,
        event_tx: Option<mpsc::UnboundedSender<ExecutionEvent>>,
    ) -> Result<(NodeOutput, String), String> {
        let ctx = ExecutionContext::new();

        let synthesis = self.synthesizer.synthesize(goal, &ctx).await?;
        let graph = synthesis.graph;
        let start_node = synthesis.start_node;
        let plan_desc = synthesis.plan_description;

        let (res, report, _) = self
            .execute_with_cognition(&graph, start_node, sop_id, None, event_tx)
            .await?;
        Ok((res, format!("Plan: {} | {}", plan_desc, report)))
    }

    /// Execute a SOP graph with recursive cognitive feedback.
    ///
    /// Loop: predict → execute → reflect → evolve
    ///   - If deviation < CONVERGENCE_THRESHOLD (0.3): stop, converged.
    ///   - If deviation >= threshold AND evolved graph available: re-execute.
    ///   - Max MAX_COGNITIVE_ITERATIONS (3) iterations.
    ///
    /// `initial_ctx` can carry cross-graph dependency outputs into the
    /// first iteration's execution context.
    ///
    /// Returns (result, report_string, opt_evolved_graph).
    #[allow(unused_assignments)]
    pub async fn execute_with_cognition(
        &self,
        sop_graph: &SOPGraph,
        start_node: NodeIndex,
        sop_id: &str,
        initial_ctx: Option<Arc<ExecutionContext>>,
        event_tx: Option<mpsc::UnboundedSender<ExecutionEvent>>,
    ) -> Result<(NodeOutput, String, Option<SOPGraph>), String> {
        let span = tracing::info_span!("kernel.execute_with_cognition", sop_id = %sop_id, graph_nodes = sop_graph.graph.node_count());
        let _enter = span.enter();

        let mut own_graph: Option<SOPGraph> = None;
        let mut current_ref: &SOPGraph = sop_graph;
        let mut current_start = start_node;
        let mut iterations: Vec<CognitiveIteration> = Vec::new();
        let mut final_result: NodeOutput = serde_json::Value::Null;
        let mut final_evolved: Option<SOPGraph> = None;
        // initial_ctx is passed to the executor on the first iteration only;
        // subsequent iterations (evolved graphs) use fresh contexts.
        let initial_ctx_for_iter = initial_ctx;

        for iteration in 0..MAX_COGNITIVE_ITERATIONS {
            let iter_start = Instant::now();
            let iter_span = tracing::info_span!("cognitive_iteration", iteration, sop_id = %sop_id);
            let _iter_enter = iter_span.enter();

            let ctx = ExecutionContext::new();

            // 1. Predict
            let pred_start = Instant::now();
            let prediction = self
                .intel
                .simulator
                .simulate(current_ref, &ctx, sop_id)
                .await?;
            let pred_duration = pred_start.elapsed();
            tracing::debug!(path_id = %prediction.path_id, success_prob = prediction.success_probability, estimated_cost = prediction.estimated_cost, duration_ms = pred_duration.as_millis(), "Simulation completed");

            let exec_ctx = if iteration == 0 {
                initial_ctx_for_iter.clone()
            } else {
                None
            };
            let final_ctx = self
                .executor
                .execute_with_context(current_ref, current_start, exec_ctx, event_tx.clone())
                .await?;
            let result = final_ctx
                .get_result(current_ref.graph.node_weight(current_start).unwrap().name())
                .ok_or("Execution failed to produce result")?;
            final_result = result.clone();

            // 3. Build per-node trace records
            let mut trace_lines: Vec<String> = Vec::new();
            for node in current_ref.graph.node_indices() {
                let name = current_ref
                    .graph
                    .node_weight(node)
                    .unwrap()
                    .name()
                    .to_string();
                let output = final_ctx
                    .get_result(&name)
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "no_result".into());
                let status = if output.contains("ResourceExhausted") {
                    "error"
                } else {
                    "ok"
                };
                let record = serde_json::json!({
                    "node": name,
                    "status": status,
                    "output": output,
                });
                trace_lines.push(record.to_string());
            }
            let trace = trace_lines.join("\n");

            // 4. Reflect
            let reflect_start = Instant::now();
            let reflection = self.intel.reflector.reflect(&trace, &prediction).await?;
            let reflect_duration = reflect_start.elapsed();
            tracing::debug!(
                deviation = reflection.deviation_score,
                suggestions = reflection.suggested_improvements.len(),
                duration_ms = reflect_duration.as_millis(),
                "Reflection completed"
            );

            // Persist history
            let history_key = format!("history:{}", sop_id);
            let mut history: sisyphus_types::ExecutionHistory = self
                .state_store
                .get_state_json(&history_key)
                .await
                .map_err(|e| format!("MFS history load error: {}", e))?
                .and_then(|val| serde_json::from_value(val).ok())
                .unwrap_or_else(|| sisyphus_types::ExecutionHistory {
                    sop_id: sop_id.to_string(),
                    records: Vec::new(),
                });

            history.records.push(sisyphus_types::HistoryRecord {
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                trace: trace.clone(),
                deviation_score: reflection.deviation_score,
                success: !trace.contains("ResourceExhausted"),
                cost: 10 * (trace.lines().count() as u64),
            });
            let _ = self
                .state_store
                .set_state_json(&history_key, serde_json::to_value(history).unwrap())
                .await;

            // 5. Evolve
            let evolve_start = Instant::now();
            let evolved = self
                .intel
                .evolver
                .evolve(current_ref, &reflection, &prediction)
                .ok();
            let evolve_duration = evolve_start.elapsed();
            let has_evolved = evolved.is_some();
            tracing::debug!(
                evolved = has_evolved,
                duration_ms = evolve_duration.as_millis(),
                "Evolution completed"
            );

            if let Some(ref eg) = evolved {
                let _ = self.save_sop(sop_id, eg).await;
            }

            let iter_duration = iter_start.elapsed();
            iterations.push(CognitiveIteration {
                iteration,
                prediction,
                deviation_score: reflection.deviation_score,
                suggestions: reflection.suggested_improvements,
                evolved: has_evolved,
            });

            // Record cognitive iteration metric
            record_cognitive_iteration(
                sop_id,
                iteration,
                reflection.deviation_score,
                reflection.deviation_score < CONVERGENCE_THRESHOLD,
                iter_duration.as_secs_f64(),
            );

            // Check convergence
            if reflection.deviation_score < CONVERGENCE_THRESHOLD || !has_evolved {
                final_evolved = evolved;
                break;
            }

            // Re-execute with evolved graph
            if let Some(eg) = evolved {
                current_start = eg.graph.node_indices().next().unwrap_or(start_node);
                own_graph = Some(eg);
                current_ref = own_graph.as_ref().unwrap();
            } else {
                break;
            }
        }

        let converged = iterations
            .last()
            .map(|i| i.deviation_score < CONVERGENCE_THRESHOLD)
            .unwrap_or(false);

        let iteration_details: Vec<String> = iterations
            .iter()
            .map(|i| {
                format!(
                    "iter[{}]: deviation={:.2}, evolved={}, suggestions={:?}",
                    i.iteration, i.deviation_score, i.evolved, i.suggestions
                )
            })
            .collect();

        let report = format!(
            "CognitiveLoop: {} iterations, converged={} | {}",
            iterations.len(),
            converged,
            iteration_details.join(" -> ")
        );

        tracing::info!(
            iterations = iterations.len(),
            converged,
            final_deviation = iterations.last().map(|i| i.deviation_score).unwrap_or(0.0),
            "Cognitive loop completed"
        );

        Ok((final_result, report, final_evolved))
    }

    /// Execute a composite workflow of multiple SOP graphs with cross-graph
    /// data dependencies.
    ///
    /// 1. Resolves topological execution order from the dependency DAG.
    /// 2. For each graph, pre-populates its ExecutionContext with outputs
    ///    from dependency graphs (via DataMapping).
    /// 3. Executes each graph through the cognitive loop.
    /// 4. Returns results for all graphs in the composite.
    pub async fn execute_composite(
        &self,
        composite: &CompositeGraph,
        event_tx: Option<mpsc::UnboundedSender<ExecutionEvent>>,
    ) -> Result<CompositeOutput, String> {
        let order = if composite.execution_order.is_empty() {
            DependencyResolver::resolve(composite)?
        } else {
            composite.execution_order.clone()
        };

        let mut results: HashMap<String, (NodeOutput, String)> = HashMap::new();
        let mut accumulated: HashMap<String, NodeOutput> = HashMap::new();

        for sop_id in &order {
            let config = composite
                .graphs
                .get(sop_id)
                .ok_or_else(|| format!("Graph {} not found in composite", sop_id))?;
            let graph = SOPGraph::from_config(config, self.tool_executor.clone());
            let start_node = graph
                .graph
                .node_indices()
                .next()
                .ok_or_else(|| format!("Graph {} has no nodes", sop_id))?;

            let dep_ctx: Option<Arc<ExecutionContext>> = {
                let ctx = ExecutionContext::new();
                let mut has_deps = false;
                for dep in &composite.dependencies {
                    if dep.target_sop_id == *sop_id {
                        for mapping in &dep.mappings {
                            if let Some(output) = accumulated.get(&mapping.source_node) {
                                ctx.set_result(mapping.target_placeholder.clone(), output.clone());
                                has_deps = true;
                            }
                        }
                    }
                }
                if has_deps {
                    Some(Arc::new(ctx))
                } else {
                    None
                }
            };

            let (result, report, _evolved) = self
                .execute_with_cognition(&graph, start_node, sop_id, dep_ctx, event_tx.clone())
                .await?;

            accumulated.insert(sop_id.clone(), result.clone());
            results.insert(sop_id.clone(), (result, report));
        }

        Ok(CompositeOutput {
            results,
            execution_order: order,
        })
    }
}
