use petgraph::algo::toposort;
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::task::JoinSet;

use sisyphus_intel::SisyphusIntel;
use sisyphus_mem::MfsStore;
use sisyphus_metrics::record_cognitive_iteration;
use sisyphus_roadmap::RoadmapManager;
use sisyphus_tools::{
    agent_channel::AgentChannel, llm::BudgetProvider, meta_learner::MetaLearner,
    ContinuousBenchmark,
};
use sisyphus_types::AgentMessage;
pub use sisyphus_types::{
    CompositeGraph, DataMapping, ExecutionContext, ExecutionEvent, GraphDependency, NodeOutput,
    SOPGraph, SOPGraphConfig, StateStore, ToolExecutor, WasmSOPNode,
};

#[async_trait::async_trait]
pub trait Consensus: Send + Sync {
    async fn is_leader(&self) -> bool;
    async fn get_leader(&self) -> String;
}

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

#[async_trait::async_trait]
impl BudgetProvider for SisyphusKernel {
    async fn check_budget(
        &self,
        project_id: &str,
        _goal_id: &str,
        estimated_cost: f64,
    ) -> Result<(), String> {
        let project = self
            .roadmap_manager
            .get_project(project_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Project {} not found", project_id))?;

        let total_goals = project
            .roadmap
            .milestones
            .iter()
            .flat_map(|m| &m.phases)
            .flat_map(|p| &p.goals)
            .count();

        let completed_goals = project
            .roadmap
            .milestones
            .iter()
            .flat_map(|m| &m.phases)
            .flat_map(|p| &p.goals)
            .filter(|g| g.status == sisyphus_types::MilestoneStatus::Completed)
            .count();

        let projection = self.resource_optimizer.project_costs(
            &self
                .roadmap_manager
                .get_outcomes(project_id)
                .await
                .unwrap_or_default(),
            total_goals,
            completed_goals,
        );

        if estimated_cost > projection.avg_cost_per_goal * 3.0 && projection.confidence > 0.5 {
            return Err(format!(
                "Estimated cost {:.2} is significantly higher than project average {:.2}",
                estimated_cost, projection.avg_cost_per_goal
            ));
        }
        Ok(())
    }

    async fn record_actual_cost(
        &self,
        project_id: &str,
        goal_id: &str,
        actual_cost: f64,
        duration_secs: f64,
    ) -> Result<(), String> {
        let project = self
            .roadmap_manager
            .get_project(project_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Project {} not found", project_id))?;

        // Find milestone and phase for the goal
        let mut location = None;
        for ms in &project.roadmap.milestones {
            for ph in &ms.phases {
                if ph.goals.iter().any(|g| g.goal_id == goal_id) {
                    location = Some((ms.id.clone(), ph.id.clone()));
                    break;
                }
            }
            if location.is_some() {
                break;
            }
        }

        let (milestone_id, phase_id) = location
            .ok_or_else(|| format!("Goal {} not found in project {}", goal_id, project_id))?;

        let outcome = sisyphus_roadmap::ExecutionOutcome {
            goal_id: goal_id.to_string(),
            project_id: project_id.to_string(),
            milestone_id,
            phase_id,
            success: true, // Costs are recorded for the call itself
            deviation_score: None,
            error: None,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            actual_cost,
            duration_secs,
        };

        self.roadmap_manager
            .record_outcome(&outcome)
            .await
            .map_err(|e| e.to_string())
    }
}

pub struct Executor;
// ... (rest of file)

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
                let node_ref = Arc::clone(
                    sop_graph
                        .graph
                        .node_weight(node)
                        .expect("node should exist in graph"),
                );
                let ctx_ref = Arc::clone(&ctx);
                let tx = event_tx.clone();
                join_set.spawn(async move {
                    let node_span = tracing::info_span!("executor.node", node = node_ref.name());
                    let _enter = node_span.enter();
                    let name = node_ref.name().to_string();

                    if let Some(ref tx) = tx {
                        if let Err(e) = tx.send(ExecutionEvent::NodeStarted { node_name: name.clone() }) {
                            tracing::warn!(node = %name, error = %e, "Failed to send NodeStarted event");
                        }
                    }

                    let result = node_ref.execute(&ctx_ref).await;

                    if let Some(ref tx) = tx {
                        match &result {
                            Ok(out) => {
                                if let Err(e) = tx.send(ExecutionEvent::NodeCompleted {
                                    node_name: name.clone(),
                                    output: out.clone()
                                }) {
                                    tracing::warn!(node = %name, error = %e, "Failed to send NodeCompleted event");
                                }
                            }
                            Err(e) => {
                                if let Err(send_err) = tx.send(ExecutionEvent::NodeFailed {
                                    node_name: name.clone(),
                                    error: e.clone()
                                }) {
                                    tracing::warn!(node = %name, error = %send_err, "Failed to send NodeFailed event");
                                }
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
                    NodeOutput(Value::String(e))
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
                                if let Err(e) = tx.send(ExecutionEvent::NodeStarted { node_name: name.clone() }) {
                                    tracing::warn!(node = %name, error = %e, "Failed to send NodeStarted event");
                                }
                            }

                            let result = node_ref.execute(&ctx_ref).await;

                            if let Some(ref tx) = tx {
                                match &result {
                                    Ok(out) => {
                                        if let Err(e) = tx.send(ExecutionEvent::NodeCompleted {
                                            node_name: name.clone(),
                                            output: out.clone()
                                        }) {
                                            tracing::warn!(node = %name, error = %e, "Failed to send NodeCompleted event");
                                        }
                                    }
                                    Err(e) => {
                                        if let Err(send_err) = tx.send(ExecutionEvent::NodeFailed {
                                            node_name: name.clone(),
                                            error: e.clone()
                                        }) {
                                            tracing::warn!(node = %name, error = %send_err, "Failed to send NodeFailed event");
                                        }
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
        self.execute_with_context(sop_graph, start_node, None, None)
            .await
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
        let estimated_nodes = composite.graphs.len() + composite.dependencies.len();
        let mut node_map: HashMap<String, _> = HashMap::with_capacity(estimated_nodes);
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
        let order = toposort(&dep_graph, None).map_err(|_| CoreError::DependencyCycleDetected)?;

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
    /// Inter-agent message bus for pub/sub communication during execution.
    pub channel: AgentChannel,
    /// Performance benchmark tracker that records cognitive iteration timings.
    pub benchmark: Arc<std::sync::Mutex<ContinuousBenchmark>>,
    /// Meta-learner that records execution outcomes for strategy optimization.
    pub meta_learner: Arc<std::sync::Mutex<MetaLearner>>,
    /// Strategic planning manager for Project → Milestone → Phase → Goal hierarchy.
    pub roadmap_manager: Arc<RoadmapManager>,
    /// Phase 3 — Strategic reflection engine (LLM-augmented roadmap analysis).
    pub strategic_reflector:
        Option<Arc<sisyphus_roadmap::strategic_reflection::StrategicReflector>>,
    /// Phase 3 — Autonomous milestone generator (LLM-driven roadmap creation).
    pub milestone_generator: Option<Arc<sisyphus_roadmap::milestone_generator::MilestoneGenerator>>,
    /// Phase 3 — Resource / cost projection engine (analytics-only).
    pub resource_optimizer: sisyphus_roadmap::resource_optimizer::ResourceOptimizer,
}

impl SisyphusKernel {
    pub fn new(
        state_store: Arc<MfsStore>,
        tool_executor: Arc<dyn ToolExecutor>,
        intel: Arc<SisyphusIntel>,
        synthesizer: Arc<dyn sisyphus_intel::FlowSynthesizer>,
    ) -> Self {
        let rm = Arc::new(RoadmapManager::new(state_store.clone()));
        Self {
            executor: Executor::new(),
            state_store,
            tool_executor,
            intel,
            synthesizer,
            channel: AgentChannel::new(),
            benchmark: Arc::new(std::sync::Mutex::new(ContinuousBenchmark::new(
                ".omo/benchmarks",
            ))),
            meta_learner: Arc::new(std::sync::Mutex::new(MetaLearner::new(".omo/meta"))),
            roadmap_manager: rm,
            strategic_reflector: None,
            milestone_generator: None,
            resource_optimizer: sisyphus_roadmap::resource_optimizer::ResourceOptimizer::new(),
        }
    }

    /// Attach a StrategicReflector for LLM-augmented roadmap analysis.
    pub fn with_strategic_reflector(
        mut self,
        llm: Arc<dyn sisyphus_tools::metis::LlmClient>,
    ) -> Self {
        self.strategic_reflector = Some(Arc::new(
            sisyphus_roadmap::strategic_reflection::StrategicReflector::new(llm),
        ));
        self
    }

    /// Attach a MilestoneGenerator for LLM-driven roadmap creation.
    pub fn with_milestone_generator(
        mut self,
        llm: Arc<dyn sisyphus_tools::metis::LlmClient>,
    ) -> Self {
        self.milestone_generator = Some(Arc::new(
            sisyphus_roadmap::milestone_generator::MilestoneGenerator::new(llm),
        ));
        self
    }

    pub async fn save_sop(&self, sop_id: &str, graph: &SOPGraph) -> Result<(), CoreError> {
        let config = sisyphus_types::SOPGraphConfig::from_graph(graph);
        let value =
            serde_json::to_value(&config).map_err(|e| CoreError::Internal(e.to_string()))?;
        self.state_store
            .set_state_json(&format!("sop:{}", sop_id), value)
            .await
            .map_err(CoreError::MfsSaveError)
    }

    pub async fn load_sop(&self, sop_id: &str) -> Result<SOPGraph, CoreError> {
        let data_val = self
            .state_store
            .get_state_json(&format!("sop:{}", sop_id))
            .await
            .map_err(CoreError::MfsLoadError)?;

        match data_val {
            None => Err(CoreError::SopNotFound(sop_id.to_string())),
            Some(Value::String(s)) => {
                // legacy format: double-wrapped JSON string
                let config: sisyphus_types::SOPGraphConfig =
                    serde_json::from_str(&s).map_err(|e| CoreError::Internal(e.to_string()))?;
                Ok(SOPGraph::from_config(&config, self.tool_executor.clone()))
            }
            Some(v) => {
                // new format: direct JSON value
                let config: sisyphus_types::SOPGraphConfig =
                    serde_json::from_value(v).map_err(|e| CoreError::Internal(e.to_string()))?;
                Ok(SOPGraph::from_config(&config, self.tool_executor.clone()))
            }
        }
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

    /// Execute a goal with project-level roadmap context injected.
    ///
    /// Fetches the current progress report for `project_id` from the
    /// `RoadmapManager` and prepends it as context so the synthesis and
    /// execution pipeline is aware of the broader project state.
    pub async fn execute_goal_with_project_context(
        &self,
        goal: &str,
        sop_id: &str,
        project_id: &str,
        event_tx: Option<mpsc::UnboundedSender<ExecutionEvent>>,
    ) -> Result<(NodeOutput, String), String> {
        let ctx_string = self
            .roadmap_manager
            .synthesize_project_context(project_id)
            .await
            .map_err(|e| format!("Failed to synthesize project context: {}", e))?;
        let augmented_goal = format!("{}\n\n---\nCurrent project context:\n{}", goal, ctx_string);
        self.execute_goal(&augmented_goal, sop_id, event_tx).await
    }

    /// Execute a project-context-aware goal and automatically record the
    /// execution outcome, evaluate roadmap health, and apply blocking.
    ///
    /// Returns the execution result together with an optional health report.
    #[allow(clippy::too_many_arguments)]
    pub async fn execute_and_record(
        &self,
        goal: &str,
        sop_id: &str,
        project_id: &str,
        milestone_id: &str,
        phase_id: &str,
        goal_id: &str,
        event_tx: Option<mpsc::UnboundedSender<ExecutionEvent>>,
    ) -> Result<(NodeOutput, String, Option<sisyphus_roadmap::RoadmapHealth>), String> {
        let start = std::time::Instant::now();
        let result = self
            .execute_goal_with_project_context(goal, sop_id, project_id, event_tx)
            .await;

        let (success, deviation, error) = match &result {
            Ok((_output, report)) => {
                let dev = if report.contains("deviation=") || report.contains("deviation_score") {
                    Some(0.5) // moderate deviation as fallback
                } else {
                    None
                };
                (true, dev, None)
            }
            Err(e) => (false, None, Some(e.clone())),
        };

        let elapsed = start.elapsed();
        let outcome = sisyphus_roadmap::ExecutionOutcome {
            goal_id: goal_id.to_string(),
            project_id: project_id.to_string(),
            milestone_id: milestone_id.to_string(),
            phase_id: phase_id.to_string(),
            success,
            deviation_score: deviation,
            error,
            timestamp: elapsed.as_secs(),
            actual_cost: 0.0, // actual cost tracking TBD at this layer
            duration_secs: elapsed.as_secs_f64(),
        };

        let _ = self.roadmap_manager.record_outcome(&outcome).await;

        // Evaluate health and optionally apply blocking on failure
        if !success {
            let health = self.roadmap_manager.apply_blocking(project_id).await.ok();
            return result.map(|(o, r)| (o, r, health));
        }

        let health = self.roadmap_manager.evaluate_health(project_id).await.ok();
        result.map(|(o, r)| (o, r, health))
    }

    /// Run strategic reflection on a project's roadmap.
    ///
    /// Requires the kernel to have been configured with `with_strategic_reflector()`.
    /// Returns `None` if no reflector is configured.
    pub async fn strategic_reflect(
        &self,
        project_id: &str,
    ) -> Option<sisyphus_roadmap::strategic_reflection::StrategicReflection> {
        let reflector = self.strategic_reflector.as_ref()?;
        match reflector.reflect(&self.roadmap_manager, project_id).await {
            Ok(reflection) => {
                let _ = self.channel.publish(
                    "strategic_events",
                    AgentMessage {
                        from: "kernel".into(),
                        to: "*".into(),
                        kind: "StrategicReflectionCompleted".into(),
                        payload: serde_json::json!({
                            "project_id": reflection.project_id,
                            "health_assessment": reflection.health_assessment,
                            "trajectory": reflection.trajectory,
                            "recommendations": reflection.recommendations.len(),
                            "risk_factors": reflection.risk_factors.len(),
                            "confidence": reflection.confidence,
                        }),
                        correlation_id: format!("{}-strategic-reflect", project_id),
                    },
                );
                Some(reflection)
            }
            Err(e) => {
                tracing::warn!(project_id = %project_id, error = %e, "Strategic reflection failed");
                None
            }
        }
    }

    /// Generate and persist milestones from a project description.
    ///
    /// Requires the kernel to have been configured with `with_milestone_generator()`.
    /// Returns `None` if no generator is configured.
    pub async fn generate_milestones(
        &self,
        project_id: &str,
        description: &str,
    ) -> Option<Vec<sisyphus_types::Milestone>> {
        let generator = self.milestone_generator.as_ref()?;
        match generator
            .generate_milestones(&self.roadmap_manager, project_id, description)
            .await
        {
            Ok(milestones) => {
                let _ = self.channel.publish(
                    "strategic_events",
                    AgentMessage {
                        from: "kernel".into(),
                        to: "*".into(),
                        kind: "MilestonesGenerated".into(),
                        payload: serde_json::json!({
                            "project_id": project_id,
                            "milestone_count": milestones.len(),
                        }),
                        correlation_id: format!("{}-milestones", project_id),
                    },
                );
                Some(milestones)
            }
            Err(e) => {
                tracing::warn!(project_id = %project_id, error = %e, "Milestone generation failed");
                None
            }
        }
    }

    /// Project remaining costs using the resource optimizer.
    pub async fn project_costs(
        &self,
        project_id: &str,
        total_goals: usize,
        completed_goals: usize,
    ) -> sisyphus_roadmap::resource_optimizer::CostProjection {
        let outcomes = self
            .roadmap_manager
            .get_outcomes(project_id)
            .await
            .unwrap_or_default();
        self.resource_optimizer
            .project_costs(&outcomes, total_goals, completed_goals)
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
        let mut final_result = NodeOutput::default();
        let mut final_evolved: Option<SOPGraph> = None;
        // initial_ctx is passed to the executor on the first iteration only;
        // subsequent iterations (evolved graphs) use fresh contexts.
        let initial_ctx_for_iter = initial_ctx;

        // Publish cognitive loop start event via AgentChannel
        let _ = self.channel.publish(
            "cognitive_events",
            AgentMessage {
                from: "kernel".into(),
                to: "*".into(),
                kind: "CognitiveLoopStarted".into(),
                payload: serde_json::json!({
                    "sop_id": sop_id,
                    "graph_nodes": sop_graph.graph.node_count(),
                    "max_iterations": MAX_COGNITIVE_ITERATIONS,
                }),
                correlation_id: format!("{}-start", sop_id),
            },
        );

        for iteration in 0..MAX_COGNITIVE_ITERATIONS {
            let iter_start = Instant::now();
            let iter_span = tracing::info_span!("cognitive_iteration", iteration, sop_id = %sop_id);
            let _iter_enter = iter_span.enter();

            // Publish iteration start via AgentChannel
            let _ = self.channel.publish(
                "cognitive_events",
                AgentMessage {
                    from: "kernel".into(),
                    to: "*".into(),
                    kind: "IterationStarted".into(),
                    payload: serde_json::json!({
                        "sop_id": sop_id,
                        "iteration": iteration,
                    }),
                    correlation_id: format!("{}-iter-{}", sop_id, iteration),
                },
            );

            let ctx = ExecutionContext::with_capacity(current_ref.graph.node_count());

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
                .get_result(
                    current_ref
                        .graph
                        .node_weight(current_start)
                        .expect("start node should exist")
                        .name(),
                )
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

            // Keep only the last 100 records to bound storage growth
            const MAX_HISTORY_RECORDS: usize = 100;
            if history.records.len() > MAX_HISTORY_RECORDS {
                let excess = history.records.len() - MAX_HISTORY_RECORDS;
                history.records.drain(0..excess);
            }

            if let Err(e) = self
                .state_store
                .set_state_json(&history_key, serde_json::to_value(&history).unwrap())
                .await
            {
                tracing::warn!(
                    sop_id = %sop_id,
                    error = %e,
                    "Failed to persist execution history"
                );
            }

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
                if let Err(e) = self.save_sop(sop_id, eg).await {
                    tracing::warn!(sop_id = %sop_id, error = %e, "Failed to persist evolved graph");
                }
            }

            let iter_duration = iter_start.elapsed();
            iterations.push(CognitiveIteration {
                iteration,
                prediction: prediction.clone(),
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

            // Record iteration timing to benchmark
            let success = !trace.contains("ResourceExhausted");
            if let Ok(mut bm) = self.benchmark.lock() {
                let run = sisyphus_types::BenchmarkRun {
                    name: format!("cognitive_iter.{}.{}", sop_id, iteration),
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    duration_secs: iter_duration.as_secs_f64(),
                    success,
                    metrics: serde_json::json!({
                        "deviation": reflection.deviation_score,
                        "has_evolved": has_evolved,
                        "prediction_success_prob": prediction.success_probability,
                    }),
                };
                let _ = bm.record_result(run);
            }

            // Record iteration outcome to meta_learner
            if let Ok(mut ml) = self.meta_learner.lock() {
                let sample = sisyphus_types::MetaLearningSample {
                    context: serde_json::json!({
                        "sop_id": sop_id,
                        "iteration": iteration,
                        "deviation_score": reflection.deviation_score,
                        "graph_nodes": current_ref.graph.node_count(),
                        "estimated_cost": prediction.estimated_cost,
                    }),
                    strategy: if has_evolved { "evolve" } else { "keep" }.into(),
                    success,
                    duration_secs: iter_duration.as_secs_f64(),
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                };
                let _ = ml.record_outcome(sample);
            }

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

        // Publish cognitive loop completion via AgentChannel
        let _ = self.channel.publish(
            "cognitive_events",
            AgentMessage {
                from: "kernel".into(),
                to: "*".into(),
                kind: "CognitiveLoopCompleted".into(),
                payload: serde_json::json!({
                    "sop_id": sop_id,
                    "iterations": iterations.len(),
                    "converged": converged,
                    "final_deviation": iterations.last().map(|i| i.deviation_score).unwrap_or(0.0),
                }),
                correlation_id: format!("{}-complete", sop_id),
            },
        );

        // Check benchmark baseline for regression
        if let Ok(bm) = self.benchmark.lock() {
            if let Some(comparison) = bm.compare_with_baseline(&format!(
                "cognitive_iter.{}.{}",
                sop_id,
                iterations.len().saturating_sub(1)
            )) {
                if comparison.is_significant {
                    tracing::warn!(
                        sop_id = %sop_id,
                        regression_pct = comparison.regression_pct,
                        "Performance regression detected in cognitive iteration"
                    );
                }
            }
        }

        // Learn from this execution and persist insight
        if let Ok(ml) = self.meta_learner.lock() {
            let insight = ml.learn();
            let _ = ml.save_insight(&insight);
            // Publish insight summary via AgentChannel
            let _ = self.channel.publish(
                "cognitive_events",
                AgentMessage {
                    from: "kernel".into(),
                    to: "*".into(),
                    kind: "LearningInsight".into(),
                    payload: serde_json::json!({
                        "sop_id": sop_id,
                        "total_samples": insight.total_samples,
                        "overall_success_rate": insight.overall_success_rate,
                        "trend": insight.trend,
                        "strategies": insight.strategies.iter().map(|s| serde_json::json!({
                            "strategy": s.strategy,
                            "success_rate": s.success_rate,
                            "total_uses": s.total_uses,
                        })).collect::<Vec<_>>(),
                    }),
                    correlation_id: format!("{}-insight", sop_id),
                },
            );
        }

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
    /// 3. Executes independent graphs in parallel through the cognitive loop.
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

        let order_len = order.len();
        let mut results: HashMap<String, (NodeOutput, String)> = HashMap::with_capacity(order_len);
        let accumulated: Arc<std::sync::Mutex<HashMap<String, NodeOutput>>> =
            Arc::new(std::sync::Mutex::new(HashMap::with_capacity(order_len)));
        #[allow(clippy::type_complexity)]
        let mut join_set: JoinSet<(
            String,
            Result<(NodeOutput, String, Option<SOPGraph>), String>,
        )> = JoinSet::new();

        // Build in-degree map from the dependency DAG
        let in_degree: Arc<std::sync::Mutex<HashMap<String, usize>>> =
            Arc::new(std::sync::Mutex::new(HashMap::with_capacity(order_len)));
        {
            let mut deg = in_degree.lock().unwrap_or_else(|e| e.into_inner());
            for sop_id in &order {
                deg.entry(sop_id.clone()).or_insert(0);
            }
            for dep in &composite.dependencies {
                *deg.entry(dep.target_sop_id.clone()).or_insert(0) += 1;
            }
        }

        // Pre-build all SOPGraphs so they are not rebuilt per spawn
        let mut graph_cache: HashMap<String, (SOPGraph, NodeIndex)> =
            HashMap::with_capacity(order_len);
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
            graph_cache.insert(sop_id.clone(), (graph, start_node));
        }

        // Wrap composite + self in Arcs for parallel spawning
        let composite = Arc::new(composite.clone());
        let self_arc = Arc::new(SisyphusKernel {
            executor: Executor::new(),
            state_store: self.state_store.clone(),
            tool_executor: self.tool_executor.clone(),
            intel: self.intel.clone(),
            synthesizer: self.synthesizer.clone(),
            channel: AgentChannel::new(),
            benchmark: self.benchmark.clone(),
            meta_learner: self.meta_learner.clone(),
            roadmap_manager: self.roadmap_manager.clone(),
            strategic_reflector: self.strategic_reflector.clone(),
            milestone_generator: self.milestone_generator.clone(),
            resource_optimizer: sisyphus_roadmap::resource_optimizer::ResourceOptimizer::new(),
        });

        // Spawn all ready graphs (in-degree == 0)
        {
            let deg = in_degree.lock().unwrap_or_else(|e| e.into_inner());
            for sop_id in &order {
                if *deg.get(sop_id).unwrap_or(&0) == 0 {
                    if let Some((graph, start_node)) = graph_cache.remove(sop_id) {
                        let kernel = Arc::clone(&self_arc);
                        let acc = Arc::clone(&accumulated);
                        let composite_ref = Arc::clone(&composite);
                        let tx = event_tx.clone();
                        let sid = sop_id.clone();
                        join_set.spawn(async move {
                            let dep_ctx = build_dep_ctx_for(
                                &sid,
                                &composite_ref,
                                &acc.lock().unwrap_or_else(|e| e.into_inner()),
                            );
                            let result = kernel
                                .execute_with_cognition(&graph, start_node, &sid, dep_ctx, tx)
                                .await;
                            (sid, result)
                        });
                    }
                }
            }
        }

        // Process completed graphs and spawn newly-ready dependents
        while let Some(res) = join_set.join_next().await {
            let (completed_id, outcome) = res.map_err(|e| e.to_string())?;
            let (node_output, report, _evolved) = outcome?;

            {
                let mut acc = accumulated.lock().unwrap_or_else(|e| e.into_inner());
                acc.insert(completed_id.clone(), node_output.clone());
            }
            results.insert(completed_id.clone(), (node_output, report));

            // Decrement in-degree of dependents and spawn ready ones
            let ready: Vec<String> = {
                let mut deg = in_degree.lock().unwrap();
                let mut ready = Vec::new();
                for dep in &composite.dependencies {
                    if dep.source_sop_id == completed_id {
                        if let Some(d) = deg.get_mut(&dep.target_sop_id) {
                            *d -= 1;
                            if *d == 0 {
                                ready.push(dep.target_sop_id.clone());
                            }
                        }
                    }
                }
                ready
            };

            for sid in ready {
                if let Some((graph, start_node)) = graph_cache.remove(&sid) {
                    let kernel = Arc::clone(&self_arc);
                    let acc = Arc::clone(&accumulated);
                    let composite_ref = Arc::clone(&composite);
                    let tx = event_tx.clone();
                    join_set.spawn(async move {
                        let dep_ctx = build_dep_ctx_for(&sid, &composite_ref, &acc.lock().unwrap());
                        let result = kernel
                            .execute_with_cognition(&graph, start_node, &sid, dep_ctx, tx)
                            .await;
                        (sid, result)
                    });
                }
            }
        }

        Ok(CompositeOutput {
            results,
            execution_order: order,
        })
    }
}

/// Build an optional execution context populated with dependency outputs for a graph.
fn build_dep_ctx_for(
    sop_id: &str,
    composite: &CompositeGraph,
    accumulated: &HashMap<String, NodeOutput>,
) -> Option<Arc<ExecutionContext>> {
    let ctx = ExecutionContext::new();
    let mut has_deps = false;
    for dep in &composite.dependencies {
        if dep.target_sop_id == sop_id {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use sisyphus_types::{
        CompositeGraph, GraphDependency, NodeKind, NodeOutput, SOPGraphConfig, SOPNodeConfig,
    };
    use std::collections::HashMap;

    #[test]
    fn test_core_error_display() {
        assert_eq!(
            CoreError::CycleDetected.to_string(),
            "SOPGraph contains cycles"
        );
        assert_eq!(
            CoreError::DependencyCycleDetected.to_string(),
            "Cross-graph dependency cycle detected"
        );
        assert_eq!(
            CoreError::MissingSourceNode.to_string(),
            "Missing source node"
        );
        assert_eq!(
            CoreError::MissingTargetNode.to_string(),
            "Missing target node"
        );
        assert_eq!(
            CoreError::MfsSaveError("fail".into()).to_string(),
            "MFS save error: fail"
        );
        assert_eq!(
            CoreError::MfsLoadError("fail".into()).to_string(),
            "MFS load error: fail"
        );
        assert_eq!(
            CoreError::SopNotFound("sop1".into()).to_string(),
            "SOP sop1 not found"
        );
        assert_eq!(
            CoreError::NoResultProduced.to_string(),
            "Execution failed to produce result"
        );
        assert_eq!(
            CoreError::GraphNotFound("g1".into()).to_string(),
            "Graph g1 not found in composite"
        );
        assert_eq!(
            CoreError::GraphEmpty("g1".into()).to_string(),
            "Graph g1 has no nodes"
        );
        assert_eq!(
            CoreError::Internal("err".into()).to_string(),
            "Internal error: err"
        );
    }

    #[test]
    fn test_core_error_from_string() {
        let err = CoreError::CycleDetected;
        let s: String = err.into();
        assert_eq!(s, "SOPGraph contains cycles");
    }

    #[test]
    fn test_dependency_resolver_resolve() {
        // Happy path: A -> B
        let mut graphs = HashMap::new();
        graphs.insert(
            "A".to_string(),
            SOPGraphConfig {
                nodes: vec![],
                edges: vec![],
            },
        );
        graphs.insert(
            "B".to_string(),
            SOPGraphConfig {
                nodes: vec![],
                edges: vec![],
            },
        );

        let deps = vec![GraphDependency {
            source_sop_id: "A".to_string(),
            target_sop_id: "B".to_string(),
            mappings: vec![],
        }];

        let composite = CompositeGraph {
            graphs,
            dependencies: deps,
            execution_order: vec![],
        };

        let order = DependencyResolver::resolve(&composite).unwrap();
        assert_eq!(order, vec!["A", "B"]);

        // Happy path: No deps
        let mut graphs_no_deps = HashMap::new();
        graphs_no_deps.insert(
            "A".to_string(),
            SOPGraphConfig {
                nodes: vec![],
                edges: vec![],
            },
        );
        graphs_no_deps.insert(
            "B".to_string(),
            SOPGraphConfig {
                nodes: vec![],
                edges: vec![],
            },
        );
        let composite_no_deps = CompositeGraph {
            graphs: graphs_no_deps,
            dependencies: vec![],
            execution_order: vec![],
        };
        let order_no_deps = DependencyResolver::resolve(&composite_no_deps).unwrap();
        assert!(order_no_deps.contains(&"A".to_string()));
        assert!(order_no_deps.contains(&"B".to_string()));
        assert_eq!(order_no_deps.len(), 2);

        // Cycle detection: A -> B -> A
        let mut graphs_cycle = HashMap::new();
        graphs_cycle.insert(
            "A".to_string(),
            SOPGraphConfig {
                nodes: vec![],
                edges: vec![],
            },
        );
        graphs_cycle.insert(
            "B".to_string(),
            SOPGraphConfig {
                nodes: vec![],
                edges: vec![],
            },
        );
        let deps_cycle = vec![
            GraphDependency {
                source_sop_id: "A".to_string(),
                target_sop_id: "B".to_string(),
                mappings: vec![],
            },
            GraphDependency {
                source_sop_id: "B".to_string(),
                target_sop_id: "A".to_string(),
                mappings: vec![],
            },
        ];
        let composite_cycle = CompositeGraph {
            graphs: graphs_cycle,
            dependencies: deps_cycle,
            execution_order: vec![],
        };
        let res_cycle = DependencyResolver::resolve(&composite_cycle);
        assert!(matches!(res_cycle, Err(CoreError::DependencyCycleDetected)));

        // Missing source node
        let mut graphs_missing = HashMap::new();
        graphs_missing.insert(
            "B".to_string(),
            SOPGraphConfig {
                nodes: vec![],
                edges: vec![],
            },
        );
        let deps_missing = vec![GraphDependency {
            source_sop_id: "A".to_string(),
            target_sop_id: "B".to_string(),
            mappings: vec![],
        }];
        let composite_missing = CompositeGraph {
            graphs: graphs_missing,
            dependencies: deps_missing,
            execution_order: vec![],
        };
        let order_missing = DependencyResolver::resolve(&composite_missing).unwrap();
        assert_eq!(order_missing, vec!["B"]);

        // Empty composite
        let composite_empty = CompositeGraph {
            graphs: HashMap::new(),
            dependencies: vec![],
            execution_order: vec![],
        };
        let order_empty = DependencyResolver::resolve(&composite_empty).unwrap();
        assert!(order_empty.is_empty());
    }

    #[test]
    fn test_build_dep_ctx_for() {
        let mut graphs = HashMap::new();
        graphs.insert(
            "A".to_string(),
            SOPGraphConfig {
                nodes: vec![],
                edges: vec![],
            },
        );
        graphs.insert(
            "B".to_string(),
            SOPGraphConfig {
                nodes: vec![],
                edges: vec![],
            },
        );

        let deps = vec![GraphDependency {
            source_sop_id: "A".to_string(),
            target_sop_id: "B".to_string(),
            mappings: vec![DataMapping {
                source_node: "node_a".to_string(),
                target_placeholder: "placeholder_b".to_string(),
            }],
        }];

        let composite = CompositeGraph {
            graphs,
            dependencies: deps,
            execution_order: vec![],
        };

        let mut accumulated = HashMap::new();
        accumulated.insert(
            "node_a".to_string(),
            NodeOutput(serde_json::Value::String("val_a".into())),
        );

        // Test with deps
        let ctx = build_dep_ctx_for("B", &composite, &accumulated);
        assert!(ctx.is_some());
        let ctx = ctx.unwrap();
        assert_eq!(
            ctx.get_result("placeholder_b").unwrap().to_string(),
            "\"val_a\""
        );

        // Test without deps (wrong target)
        let ctx_none = build_dep_ctx_for("A", &composite, &accumulated);
        assert!(ctx_none.is_none());

        // Test with deps but missing source output
        let accumulated_empty = HashMap::new();
        let ctx_missing = build_dep_ctx_for("B", &composite, &accumulated_empty);
        assert!(ctx_missing.is_none());
    }

    struct MockToolExecutor;
    #[async_trait::async_trait]
    impl ToolExecutor for MockToolExecutor {
        async fn execute(
            &self,
            _tool_id: &str,
            _args: Vec<u8>,
            _fuel_limit: u64,
        ) -> Result<Vec<u8>, String> {
            Ok(b"mock".to_vec())
        }
    }

    #[tokio::test]
    async fn test_executor_execute_cycle() {
        let executor = Executor::new();

        // Create a config with a cycle: Node 0 -> Node 1 -> Node 0
        let config = SOPGraphConfig {
            nodes: vec![
                SOPNodeConfig {
                    name: "n0".into(),
                    kind: NodeKind::Wasm {
                        tool_id: "tool0".into(),
                        fuel_limit: 100,
                    },
                },
                SOPNodeConfig {
                    name: "n1".into(),
                    kind: NodeKind::Wasm {
                        tool_id: "tool1".into(),
                        fuel_limit: 100,
                    },
                },
            ],
            edges: vec![(0, 1), (1, 0)],
        };

        let graph = SOPGraph::from_config(&config, Arc::new(MockToolExecutor));
        let res = executor
            .execute(&graph, petgraph::graph::NodeIndex::new(0))
            .await;

        assert!(matches!(res, Err(CoreError::CycleDetected)));
    }

    #[test]
    fn test_struct_creation() {
        // CognitiveIteration
        let iter = CognitiveIteration {
            iteration: 1,
            prediction: sisyphus_intel::SimulationResult {
                path_id: "p1".into(),
                success_probability: 0.8,
                estimated_cost: 100u64,
                predicted_outcome: "ok".into(),
            },
            deviation_score: 0.1,
            suggestions: vec!["s1".into()],
            evolved: true,
        };
        assert_eq!(iter.iteration, 1);
        assert_eq!(iter.deviation_score, 0.1);

        // CognitiveReport
        let report = CognitiveReport {
            iterations: 1,
            converged: true,
            final_deviation: 0.1,
            history: vec![iter],
        };
        assert_eq!(report.iterations, 1);
        assert!(report.converged);

        // CompositeOutput
        let mut results = HashMap::new();
        results.insert(
            "sop1".into(),
            (
                NodeOutput(serde_json::Value::String("out".into())),
                "report".into(),
            ),
        );
        let output = CompositeOutput {
            results,
            execution_order: vec!["sop1".into()],
        };
        assert_eq!(output.execution_order.len(), 1);
        assert!(output.results.contains_key("sop1"));
    }

    #[tokio::test]
    async fn test_executor_execute_with_context() {
        let executor = Executor::new();

        // Simple DAG: n0 -> n1
        let config = SOPGraphConfig {
            nodes: vec![
                SOPNodeConfig {
                    name: "n0".into(),
                    kind: NodeKind::Wasm {
                        tool_id: "tool0".into(),
                        fuel_limit: 100,
                    },
                },
                SOPNodeConfig {
                    name: "n1".into(),
                    kind: NodeKind::Wasm {
                        tool_id: "tool1".into(),
                        fuel_limit: 100,
                    },
                },
            ],
            edges: vec![(0, 1)],
        };

        let graph = SOPGraph::from_config(&config, Arc::new(MockToolExecutor));
        let (tx, mut rx) = mpsc::unbounded_channel();

        let res = executor
            .execute_with_context(&graph, petgraph::graph::NodeIndex::new(0), None, Some(tx))
            .await;

        assert!(res.is_ok());
        let ctx = res.unwrap();

        // Verify results are stored
        assert!(ctx.get_result("n0").is_some());
        assert!(ctx.get_result("n1").is_some());

        // Verify events were sent
        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }

        // Expect NodeStarted and NodeCompleted for both nodes
        assert!(events
            .iter()
            .any(|e| matches!(e, ExecutionEvent::NodeStarted { node_name } if node_name == "n0")));
        assert!(events.iter().any(
            |e| matches!(e, ExecutionEvent::NodeCompleted { node_name, .. } if node_name == "n0")
        ));
        assert!(events
            .iter()
            .any(|e| matches!(e, ExecutionEvent::NodeStarted { node_name } if node_name == "n1")));
        assert!(events.iter().any(
            |e| matches!(e, ExecutionEvent::NodeCompleted { node_name, .. } if node_name == "n1")
        ));
    }

    #[tokio::test]
    async fn test_executor_diamond_dag() {
        let executor = Executor::new();

        // Diamond: n0 -> [n1, n2] -> n3
        let config = SOPGraphConfig {
            nodes: vec![
                SOPNodeConfig {
                    name: "n0".into(),
                    kind: NodeKind::Wasm {
                        tool_id: "t".into(),
                        fuel_limit: 100,
                    },
                },
                SOPNodeConfig {
                    name: "n1".into(),
                    kind: NodeKind::Wasm {
                        tool_id: "t".into(),
                        fuel_limit: 100,
                    },
                },
                SOPNodeConfig {
                    name: "n2".into(),
                    kind: NodeKind::Wasm {
                        tool_id: "t".into(),
                        fuel_limit: 100,
                    },
                },
                SOPNodeConfig {
                    name: "n3".into(),
                    kind: NodeKind::Wasm {
                        tool_id: "t".into(),
                        fuel_limit: 100,
                    },
                },
            ],
            edges: vec![(0, 1), (0, 2), (1, 3), (2, 3)],
        };

        let graph = SOPGraph::from_config(&config, Arc::new(MockToolExecutor));
        let res = executor
            .execute(&graph, petgraph::graph::NodeIndex::new(0))
            .await;
        assert!(res.is_ok());
        let ctx = res.unwrap();
        assert!(ctx.get_result("n0").is_some());
        assert!(ctx.get_result("n1").is_some());
        assert!(ctx.get_result("n2").is_some());
        assert!(ctx.get_result("n3").is_some());
    }

    #[tokio::test]
    async fn test_executor_node_failure_stored_as_value() {
        let executor = Executor::new();

        // Mock executor that fails on n0
        struct FailOnFirst;
        #[async_trait::async_trait]
        impl ToolExecutor for FailOnFirst {
            async fn execute(
                &self,
                tool_id: &str,
                _args: Vec<u8>,
                _fuel_limit: u64,
            ) -> Result<Vec<u8>, String> {
                if tool_id == "fail" {
                    Err("intentional failure".into())
                } else {
                    Ok(b"ok".to_vec())
                }
            }
        }

        let config = SOPGraphConfig {
            nodes: vec![
                SOPNodeConfig {
                    name: "n0".into(),
                    kind: NodeKind::Wasm {
                        tool_id: "fail".into(),
                        fuel_limit: 100,
                    },
                },
                SOPNodeConfig {
                    name: "n1".into(),
                    kind: NodeKind::Wasm {
                        tool_id: "ok".into(),
                        fuel_limit: 100,
                    },
                },
            ],
            edges: vec![(0, 1)],
        };

        let graph = SOPGraph::from_config(&config, Arc::new(FailOnFirst));
        let (tx, _rx) = mpsc::unbounded_channel();

        let res = executor
            .execute_with_context(&graph, petgraph::graph::NodeIndex::new(0), None, Some(tx))
            .await;

        assert!(res.is_ok());
        let ctx = res.unwrap();

        // Failed node stores error as Value::String
        let n0_out = ctx.get_result("n0").unwrap();
        assert_eq!(
            n0_out.0,
            serde_json::Value::String("intentional failure".into())
        );

        // n1 still executes because the executor continues after failures
        let n1_out = ctx.get_result("n1").unwrap();
        assert_eq!(n1_out.0, serde_json::Value::String("ok".into()));
    }

    #[tokio::test]
    async fn test_executor_empty_graph() {
        let executor = Executor::new();
        let config = SOPGraphConfig {
            nodes: vec![],
            edges: vec![],
        };
        let graph = SOPGraph::from_config(&config, Arc::new(MockToolExecutor));
        let res = executor
            .execute(&graph, petgraph::graph::NodeIndex::new(0))
            .await;
        assert!(res.is_ok());
        let ctx = res.unwrap();
        assert!(ctx.get_result("nonexistent").is_none());
    }

    #[test]
    fn test_dependency_resolver_diamond() {
        // A -> B, A -> C  (fan-out)
        let mut graphs = HashMap::new();
        graphs.insert(
            "A".into(),
            SOPGraphConfig {
                nodes: vec![],
                edges: vec![],
            },
        );
        graphs.insert(
            "B".into(),
            SOPGraphConfig {
                nodes: vec![],
                edges: vec![],
            },
        );
        graphs.insert(
            "C".into(),
            SOPGraphConfig {
                nodes: vec![],
                edges: vec![],
            },
        );

        let deps = vec![
            GraphDependency {
                source_sop_id: "A".into(),
                target_sop_id: "B".into(),
                mappings: vec![],
            },
            GraphDependency {
                source_sop_id: "A".into(),
                target_sop_id: "C".into(),
                mappings: vec![],
            },
        ];

        let composite = CompositeGraph {
            graphs,
            dependencies: deps,
            execution_order: vec![],
        };
        let order = DependencyResolver::resolve(&composite).unwrap();

        assert_eq!(order[0], "A");
        assert!(order.contains(&"B".into()));
        assert!(order.contains(&"C".into()));
        assert_eq!(order.len(), 3);
    }

    #[test]
    fn test_dependency_resolver_chain() {
        // A -> B -> C
        let mut graphs = HashMap::new();
        graphs.insert(
            "A".into(),
            SOPGraphConfig {
                nodes: vec![],
                edges: vec![],
            },
        );
        graphs.insert(
            "B".into(),
            SOPGraphConfig {
                nodes: vec![],
                edges: vec![],
            },
        );
        graphs.insert(
            "C".into(),
            SOPGraphConfig {
                nodes: vec![],
                edges: vec![],
            },
        );

        let deps = vec![
            GraphDependency {
                source_sop_id: "A".into(),
                target_sop_id: "B".into(),
                mappings: vec![],
            },
            GraphDependency {
                source_sop_id: "B".into(),
                target_sop_id: "C".into(),
                mappings: vec![],
            },
        ];

        let composite = CompositeGraph {
            graphs,
            dependencies: deps,
            execution_order: vec![],
        };
        let order = DependencyResolver::resolve(&composite).unwrap();
        assert_eq!(order, vec!["A", "B", "C"]);
    }

    #[test]
    fn test_build_dep_ctx_for_multiple_sources() {
        // SOP C depends on both A and B with different mappings
        let mut graphs = HashMap::new();
        graphs.insert(
            "A".into(),
            SOPGraphConfig {
                nodes: vec![],
                edges: vec![],
            },
        );
        graphs.insert(
            "B".into(),
            SOPGraphConfig {
                nodes: vec![],
                edges: vec![],
            },
        );
        graphs.insert(
            "C".into(),
            SOPGraphConfig {
                nodes: vec![],
                edges: vec![],
            },
        );

        let deps = vec![
            GraphDependency {
                source_sop_id: "A".into(),
                target_sop_id: "C".into(),
                mappings: vec![DataMapping {
                    source_node: "a_node".into(),
                    target_placeholder: "c_from_a".into(),
                }],
            },
            GraphDependency {
                source_sop_id: "B".into(),
                target_sop_id: "C".into(),
                mappings: vec![DataMapping {
                    source_node: "b_node".into(),
                    target_placeholder: "c_from_b".into(),
                }],
            },
        ];

        let composite = CompositeGraph {
            graphs,
            dependencies: deps,
            execution_order: vec![],
        };

        let mut accumulated = HashMap::new();
        accumulated.insert(
            "a_node".into(),
            NodeOutput(serde_json::Value::String("val_a".into())),
        );
        accumulated.insert(
            "b_node".into(),
            NodeOutput(serde_json::Value::String("val_b".into())),
        );

        let ctx = build_dep_ctx_for("C", &composite, &accumulated).unwrap();
        assert_eq!(ctx.get_result("c_from_a").unwrap().to_string(), "\"val_a\"");
        assert_eq!(ctx.get_result("c_from_b").unwrap().to_string(), "\"val_b\"");
    }

    #[test]
    fn test_build_dep_ctx_for_single_source_multi_mapping() {
        // A -> B with two DataMapping entries from different source nodes
        let mut graphs = HashMap::new();
        graphs.insert(
            "A".into(),
            SOPGraphConfig {
                nodes: vec![],
                edges: vec![],
            },
        );
        graphs.insert(
            "B".into(),
            SOPGraphConfig {
                nodes: vec![],
                edges: vec![],
            },
        );

        let deps = vec![GraphDependency {
            source_sop_id: "A".into(),
            target_sop_id: "B".into(),
            mappings: vec![
                DataMapping {
                    source_node: "n1".into(),
                    target_placeholder: "p1".into(),
                },
                DataMapping {
                    source_node: "n2".into(),
                    target_placeholder: "p2".into(),
                },
            ],
        }];

        let composite = CompositeGraph {
            graphs,
            dependencies: deps,
            execution_order: vec![],
        };

        let mut accumulated = HashMap::new();
        accumulated.insert(
            "n1".into(),
            NodeOutput(serde_json::Value::String("v1".into())),
        );
        accumulated.insert(
            "n2".into(),
            NodeOutput(serde_json::Value::String("v2".into())),
        );

        let ctx = build_dep_ctx_for("B", &composite, &accumulated).unwrap();
        assert_eq!(ctx.get_result("p1").unwrap().to_string(), "\"v1\"");
        assert_eq!(ctx.get_result("p2").unwrap().to_string(), "\"v2\"");
    }

    async fn build_test_kernel() -> SisyphusKernel {
        let state_store = Arc::new(
            sisyphus_mem::MfsStore::new("sqlite::memory:")
                .await
                .expect("in-memory MfsStore"),
        );
        let tool_executor: Arc<dyn ToolExecutor> = Arc::new(MockToolExecutor);

        let intel = Arc::new(sisyphus_intel::SisyphusIntel::new(
            Arc::new(sisyphus_intel::MonteCarloSimulator::new(1)),
            Arc::new(sisyphus_intel::StandardReflector),
            Arc::new(sisyphus_intel::TopologicalSOPEvolver::new()),
        ));

        let synthesizer: Arc<dyn sisyphus_intel::FlowSynthesizer> = Arc::new(
            sisyphus_intel::GoalDrivenSynthesizer::new(tool_executor.clone()),
        );

        SisyphusKernel::new(state_store, tool_executor, intel, synthesizer)
    }

    #[tokio::test]
    async fn test_kernel_save_load_roundtrip() {
        let kernel = build_test_kernel().await;

        let config = SOPGraphConfig {
            nodes: vec![
                SOPNodeConfig {
                    name: "n0".into(),
                    kind: NodeKind::Wasm {
                        tool_id: "t0".into(),
                        fuel_limit: 100,
                    },
                },
                SOPNodeConfig {
                    name: "n1".into(),
                    kind: NodeKind::Wasm {
                        tool_id: "t1".into(),
                        fuel_limit: 200,
                    },
                },
            ],
            edges: vec![(0, 1)],
        };
        let graph = SOPGraph::from_config(&config, Arc::new(MockToolExecutor));

        kernel.save_sop("test_roundtrip", &graph).await.unwrap();
        let loaded = kernel.load_sop("test_roundtrip").await.unwrap();

        assert_eq!(loaded.graph.node_count(), 2);
        let names: Vec<String> = loaded
            .graph
            .node_weights()
            .map(|n| n.name().to_string())
            .collect();
        assert!(names.contains(&"n0".to_string()));
        assert!(names.contains(&"n1".to_string()));

        let order =
            petgraph::algo::toposort(&loaded.graph, None).expect("loaded graph must be acyclic");
        assert_eq!(order.len(), 2);
    }

    #[tokio::test]
    async fn test_kernel_save_load_empty_graph() {
        let kernel = build_test_kernel().await;

        let config = SOPGraphConfig {
            nodes: vec![],
            edges: vec![],
        };
        let graph = SOPGraph::from_config(&config, Arc::new(MockToolExecutor));

        kernel.save_sop("test_empty", &graph).await.unwrap();
        let loaded = kernel.load_sop("test_empty").await.unwrap();

        assert_eq!(loaded.graph.node_count(), 0);
    }

    #[tokio::test]
    async fn test_kernel_load_nonexistent() {
        let kernel = build_test_kernel().await;

        match kernel.load_sop("nonexistent_sop").await {
            Err(e) => {
                assert!(
                    e.to_string().contains("nonexistent_sop"),
                    "error should mention the sop_id: got {}",
                    e
                );
            }
            Ok(_) => panic!("expected SopNotFound error"),
        }
    }
}
