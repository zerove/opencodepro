//! StrategicManager handles the high-level cognitive loop (predict -> execute -> reflect -> evolve).
//! It coordinates between the IntelligenceManager and ExecutionManager to achieve convergence.

use parking_lot::{Mutex, RwLock};
use serde_json::Value;
use std::sync::Arc;
use std::time::{Instant, SystemTime};
use tokio::sync::mpsc;
use tracing::error;

use sisyphus_intel::{SimulationResult, SisyphusSupervisor};
use sisyphus_types::SOPGraph;
use sisyphus_types::{CognitiveEvent, CognitiveMetrics, ExecutionTrace};

use crate::{
    execution_manager::ExecutionManager, CoreError, ExecutionContext, ExecutionEvent,
    ExecutionStatus, Executor, IntelligenceManager, NodeOutput,
};
use sisyphus_intel::{
    DeepReflector, FailurePatternEvolver, FailurePatternLibrary, PlannerConfig,
    ProbabilisticPlanner,
};
use sisyphus_types::config::CognitiveLoopConfig;

/// A single iteration in a cognitive execution cycle.
#[derive(Clone, Debug)]
pub struct CognitiveIteration {
    pub iteration: usize,
    pub prediction: SimulationResult,
    pub deviation_score: f64,
    pub suggestions: Vec<String>,
    pub evolved: bool,
}

/// Full report from a recursive cognitive execution.
#[derive(Debug, Clone)]
pub struct CognitiveReport {
    pub iterations: usize,
    pub converged: bool,
    pub final_deviation: f64,
    pub history: Vec<CognitiveIteration>,
    pub traces: Vec<ExecutionTrace>,
}

impl CognitiveReport {
    pub fn to_mermaid(&self) -> String {
        let mut mermaid = String::from("graph TD\n");
        for (i, trace) in self.traces.iter().enumerate() {
            let iter_label = format!("Iter{}", i + 1);
            mermaid.push_str(&format!("  subgraph {}[Iteration {}]\n", iter_label, i + 1));
            let mut prev_node: Option<String> = None;
            for point in &trace.points {
                let node_id = format!("{}_{}", iter_label, point.node_name);
                let node_label = format!("{}({:.2})", point.node_name, point.deviation_score);
                mermaid.push_str(&format!("    {}[\"{}\"]\n", node_id, node_label));
                if let Some(prev) = prev_node {
                    mermaid.push_str(&format!("    {} --> {}\n", prev, node_id));
                }
                prev_node = Some(node_id);
            }
            if i < self.traces.len() - 1 || !self.converged {
                if let Some(last_node) = prev_node {
                    mermaid.push_str(&format!(
                        "    {} -- PIVOT --> Evolution_{}\n",
                        last_node,
                        i + 1
                    ));
                }
            }
            mermaid.push_str("  end\n");
        }
        for i in 1..self.traces.len() {
            mermaid.push_str(&format!("  Evolution_{} --> Iter{}_Start\n", i, i + 1));
        }
        mermaid
    }
}

pub struct StrategicManager {
    pub intelligence_manager: Arc<IntelligenceManager>,
    pub execution_manager: Arc<ExecutionManager>,
    pub config: RwLock<CognitiveLoopConfig>,
    pub channel: Arc<crate::AgentChannel>,
    pub benchmark: Arc<Mutex<crate::ContinuousBenchmark>>,
    pub meta_learner: Arc<Mutex<crate::MetaLearner>>,
    pub experience_bank: Arc<dyn sisyphus_types::experience::ExperienceBank>,
    pub pattern_analyzer: sisyphus_intel::pattern_analyzer::SemanticPatternAnalyzer,
    pub roadmap_manager: Arc<sisyphus_roadmap::RoadmapManager>,
    pub gap_analyzer: sisyphus_intel::GapAnalyzer,
    pub sp_manager: Arc<crate::self_programming_manager::SelfProgrammingManager>,
    pub intervention_manager: Arc<crate::intervention_manager::InterventionManager>,
    pub internalization_manager: Arc<crate::internalization_manager::InternalizationManager>,
    pub visual_manager: Option<Arc<sisyphus_visual::VisualPerceptionManager>>,
    /// Probabilistic path planner using Monte Carlo simulation results
    pub probabilistic_planner: ProbabilisticPlanner,
    /// Failure pattern library for targeted SOP evolution
    pub failure_pattern_library: Arc<parking_lot::RwLock<FailurePatternLibrary>>,
    /// Deep reflector for multi-dimensional deviation analysis
    pub deep_reflector: Arc<parking_lot::RwLock<DeepReflector>>,
}

impl StrategicManager {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        intelligence_manager: Arc<IntelligenceManager>,
        execution_manager: Arc<ExecutionManager>,
        config: CognitiveLoopConfig,
        channel: Arc<crate::AgentChannel>,
        benchmark: Arc<Mutex<crate::ContinuousBenchmark>>,
        meta_learner: Arc<Mutex<crate::MetaLearner>>,
        experience_bank: Arc<dyn sisyphus_types::experience::ExperienceBank>,
        knowledge_manager: Arc<sisyphus_knowledge::AssetManager>,
        roadmap_manager: Arc<sisyphus_roadmap::RoadmapManager>,
        intervention_manager: Arc<crate::intervention_manager::InterventionManager>,
        internalization_manager: Arc<crate::internalization_manager::InternalizationManager>,
        visual_manager: Option<Arc<sisyphus_visual::VisualPerceptionManager>>,
    ) -> Self {
        Self {
            intelligence_manager: intelligence_manager.clone(),
            execution_manager: execution_manager.clone(),
            config: RwLock::new(config),
            channel,
            benchmark,
            meta_learner,
            experience_bank,
            pattern_analyzer: sisyphus_intel::pattern_analyzer::SemanticPatternAnalyzer::new(3),
            roadmap_manager,
            gap_analyzer: sisyphus_intel::GapAnalyzer::new(intelligence_manager.llm_client.clone()),
            sp_manager: Arc::new(
                crate::self_programming_manager::SelfProgrammingManager::new(
                    intelligence_manager.llm_client.clone(),
                    execution_manager.clone(),
                    knowledge_manager.clone(),
                ),
            ),
            intervention_manager,
            internalization_manager,
            visual_manager,
            probabilistic_planner: ProbabilisticPlanner::new(PlannerConfig::default()),
            failure_pattern_library: Arc::new(parking_lot::RwLock::new(
                FailurePatternLibrary::new(),
            )),
            deep_reflector: Arc::new(parking_lot::RwLock::new(DeepReflector::new())),
        }
    }

    fn build_cog_map(
        sop_id: &str,
        goal_text: &str,
        graph: &SOPGraph,
        ctx: &ExecutionContext,
        iteration: usize,
        active_node: Option<String>,
    ) -> Result<sisyphus_types::CogMap, CoreError> {
        let mut nodes = Vec::new();
        for node_idx in graph.graph.node_indices() {
            let nw = graph
                .graph
                .node_weight(node_idx)
                .ok_or_else(|| CoreError::Internal("Node weight missing".to_string()))?;
            let name = nw.name();

            let status = if let Some(out) = ctx.get_result(name) {
                if out.to_string().contains("Error") || out.to_string().contains("Failed") {
                    "error".into()
                } else {
                    "ok".into()
                }
            } else if Some(name.to_string()) == active_node {
                "running".into()
            } else {
                "pending".into()
            };

            let deviation = 0.0; // In a real system, we'd track per-node deviation from the simulator

            nodes.push(sisyphus_types::CogNode {
                id: node_idx.index().to_string(),
                label: name.to_string(),
                status,
                deviation,
                position: (0.0, 0.0), // Frontend will handle layout (e.g., Dagre)
            });
        }

        let mut edges = Vec::new();
        for edge_idx in graph.graph.edge_indices() {
            if let Some((source, target)) = graph.graph.edge_endpoints(edge_idx) {
                edges.push(sisyphus_types::CogEdge {
                    source: source.index().to_string(),
                    target: target.index().to_string(),
                    label: None,
                });
            }
        }

        Ok(sisyphus_types::CogMap {
            sop_id: sop_id.to_string(),
            goal_text: goal_text.to_string(),
            nodes,
            edges,
            active_node,
            iteration,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn run_cognitive_loop(
        &self,
        sop_graph: Arc<SOPGraph>,
        sop_id: &str,
        goal_text: Option<&str>,
        project_id: Option<&str>,
        initial_ctx: Option<Arc<ExecutionContext>>,
        event_tx: Option<mpsc::UnboundedSender<ExecutionEvent>>,
        broadcast_fn: impl Fn(&str, ExecutionEvent),
        stream_tx: Option<mpsc::UnboundedSender<CognitiveEvent>>,
        micro_reflector: Option<Arc<dyn sisyphus_types::MicroReflector>>,
    ) -> Result<(NodeOutput, CognitiveReport, Option<Arc<SOPGraph>>), CoreError> {
        let _start_time = std::time::Instant::now();

        // --- HOT START PHASE ---
        let mut current_ref = Arc::clone(&sop_graph);
        let mut hot_start_active = false;

        if let Some(goal) = goal_text {
            // 1. Visual Embedding Retrieval
            let mut visual_embedding = None;
            if let Some(ref v_manager) = self.visual_manager {
                // In a real system, we would capture the current screenshot from the executor/OS.
                // For now, we simulate a visual state.
                let dummy_image = vec![0u8; 1024];
                if let Ok(emb) = v_manager.get_embedding(&dummy_image).await {
                    visual_embedding = Some(emb);
                }
            }

            let query = sisyphus_types::experience::ExperienceQuery {
                query_text: goal.to_string(),
                context_hash: format!("{:x}", md5::compute(goal)),
                project_id: project_id.map(|s| s.to_string()),
                action_id: None,
                visual_embedding,
                limit: 5,
            };

            if let Ok(Some(_asset)) = self
                .sp_manager
                .knowledge_manager
                .find_best_asset(goal, self.intelligence_manager.llm_client.clone())
                .await
            {
                // We also consider the multi-modal experience bank for a more nuanced Hot-Start.
                // (Actually, find_best_asset is a higher-level abstraction, we'll use experience_bank directly for multi-modal)
            }

            // Direct multi-modal experience retrieval
            if let Ok(exps) = self.experience_bank.query_similar(query).await {
                if let Some(best_exp) = exps.into_iter().min_by(|a, b| {
                    a.deviation_score
                        .partial_cmp(&b.deviation_score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                }) {
                    tracing::info!(goal = %goal, asset_id = %best_exp.action_id, "Multi-modal hot-start match found!");
                    if let Ok(graph) = self
                        .execution_manager
                        .load_sop(
                            &best_exp.action_id,
                            best_exp.project_id.as_deref().unwrap_or("default"),
                        )
                        .await
                    {
                        current_ref = graph;
                        hot_start_active = true;
                    }
                }
            }

            if let Some(ref tx) = stream_tx {
                let _ = tx.send(CognitiveEvent::HotStart {
                    asset_id: "multi-modal-bank".into(),
                    confidence: 1.0,
                });
            }

            // Bind cognitive event stream to self-programming manager for observability
            if let Some(ref tx) = stream_tx {
                self.sp_manager.set_event_sender(tx.clone());
            }
            // -----------------------
        }
        // -----------------------

        let (_current_strategy, _current_params) = {
            let context = serde_json::json!({
                "sop_id": sop_id,
                "node_count": current_ref.graph.node_count(),
            });
            let ml = self.meta_learner.lock();
            ml.best_strategy_params(&context)
                .unwrap_or_else(|| ("balanced".to_string(), serde_json::json!({})))
        };

        let (max_iterations, convergence_threshold) = {
            // --- Dynamic Depth Adjustment ---
            let depth = sisyphus_intel::ComplexityAnalyzer::analyze(
                self.experience_bank.clone(),
                &current_ref,
                goal_text.unwrap_or(sop_id),
            )
            .await;

            let (mut d_max, d_thresh) = depth.to_config();

            // If hot-start is active, we can significantly reduce iterations
            if hot_start_active {
                d_max = (d_max / 2).max(2);
            }

            let mut config_lock = self.config.write();
            tracing::info!(
                "Updating loop parameters: max_iterations {} -> {}, threshold -> {}",
                config_lock.max_iterations,
                d_max,
                d_thresh
            );

            // Override strategy with dynamic depth
            config_lock.max_iterations = d_max;
            config_lock.convergence_threshold = d_thresh;

            (
                config_lock.max_iterations as usize,
                config_lock.convergence_threshold,
            )
        };

        let span = tracing::info_span!("strategic.run_cognitive_loop", sop_id = %sop_id, graph_nodes = sop_graph.graph.node_count());
        let _enter = span.enter();

        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.execution_manager.active_executions.insert(
            sop_id.to_string(),
            ExecutionStatus {
                goal_id: sop_id.to_string(),
                status: "Initializing".into(),
                current_node: None,
                thought_logs: vec!["Starting cognitive loop...".into()],
                progress: 0.0,
                last_updated: now,
            },
        );

        broadcast_fn(
            sop_id,
            ExecutionEvent::CognitiveStep {
                step: "Initializing".into(),
                message: "Starting cognitive loop...".into(),
                metrics: Some(CognitiveMetrics {
                    iteration: 0,
                    deviation: 0.0,
                    confidence: 0.0,
                    progress: 0.0,
                    token_cost: 0,
                    stability: 1.0,
                    info_gain: 0.0,
                }),
            },
        );

        let executor = Executor::new(
            Arc::new(sisyphus_tools_executor::executor::test_cmds::TestExecutor::new()),
            self.intervention_manager.clone(),
            self.internalization_manager.clone(),
            self.visual_manager.clone(),
        );
        let mut own_graph: Option<Arc<SOPGraph>>;
        let mut current_ref: Arc<SOPGraph> = Arc::clone(&sop_graph);
        let mut current_start = sop_graph.start_node;
        let mut iterations: Vec<CognitiveIteration> = Vec::with_capacity(max_iterations);
        let _traces: Vec<ExecutionTrace> = Vec::with_capacity(max_iterations);
        let mut final_result = NodeOutput::default();
        let mut final_evolved: Option<Arc<SOPGraph>> = None;
        let initial_ctx_for_iter = initial_ctx;

        for iteration in 0..max_iterations {
            let iter_start = Instant::now();
            let iter_span = tracing::info_span!("cognitive_iteration", iteration, sop_id = %sop_id);
            let _iter_enter = iter_span.enter();
            let mut pivot_reason: Option<String> = None;

            let ctx = ExecutionContext::with_capacity(current_ref.graph.node_count());
            let iter_now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            // 1. Predict
            let depth = sisyphus_intel::ComplexityAnalyzer::analyze(
                self.experience_bank.clone(),
                &current_ref,
                goal_text.unwrap_or(sop_id),
            )
            .await;

            let prediction = if !depth.should_simulate() {
                tracing::info!(sop_id, "Skipping simulation phase due to low complexity");
                sisyphus_intel::simulation::SimulationResult {
                    success_probability: 1.0,
                    predicted_stability: 1.0,
                    path_id: "default".into(),
                    estimated_cost: 0,
                    predicted_effort: 0,
                    predicted_outcome: "success".into(),
                }
            } else {
                if let Some(mut status) = self.execution_manager.active_executions.get_mut(sop_id) {
                    status.status = "Predicting".into();
                    status.thought_logs.push(format!(
                        "Iteration {}: Simulating potential paths...",
                        iteration
                    ));
                    status.last_updated = iter_now;
                }
                broadcast_fn(
                    sop_id,
                    ExecutionEvent::CognitiveStep {
                        step: "Predicting".into(),
                        message: format!("Iteration {}: Simulating potential paths...", iteration),
                        metrics: Some(CognitiveMetrics {
                            iteration,
                            deviation: 0.0,
                            confidence: 0.0,
                            progress: (iteration as f32 / max_iterations as f32) * 100.0,
                            token_cost: 0,
                            stability: 1.0,
                            info_gain: 0.0,
                        }),
                    },
                );
                self.intelligence_manager
                    .intel
                    .simulator
                    .simulate(
                        Arc::clone(&current_ref),
                        &ctx,
                        project_id.unwrap_or("default"),
                        sop_id,
                        goal_text,
                    )
                    .await
                    .map_err(|e| CoreError::Internal(e.to_string()))?
            };

            // 1.5 Logical Simulation (White-box)
            if let Some(mut status) = self.execution_manager.active_executions.get_mut(sop_id) {
                status.status = "Simulating Logic".into();
                status.thought_logs.push(format!(
                    "Iteration {}: Auditing logical consistency...",
                    iteration
                ));
                status.last_updated = iter_now;
            }
            let log_sim_result = self
                .intelligence_manager
                .intel
                .logical_simulator
                .simulate_logic(Arc::clone(&current_ref), &ctx, sop_id, goal_text)
                .await;

            if let Ok(sim_res) = log_sim_result {
                if !sim_res.is_valid {
                    let critical_flaws: Vec<_> = sim_res
                        .flaws
                        .iter()
                        .filter(|f| {
                            matches!(
                                f.severity,
                                sisyphus_intel::simulation::FlawSeverity::Critical
                            )
                        })
                        .collect();

                    if !critical_flaws.is_empty() {
                        let flaw_msg = critical_flaws[0].description.clone();
                        pivot_reason = Some(format!("Logical Flaw Detected: {}", flaw_msg));
                    }
                }
            }

            // ── Probabilistic Path Ranking ──────────────────────────────────
            // Use the probabilistic planner to rank paths and select the optimal one
            let ranked_path = self
                .probabilistic_planner
                .select_optimal_path(std::slice::from_ref(&prediction));
            if let Some(ref rp) = ranked_path {
                tracing::info!(
                    sop_id,
                    path_id = %rp.path_id,
                    composite_score = rp.composite_score,
                    risk = ?rp.risk_level,
                    "Probabilistic planner: optimal path selected"
                );
                if let Some(ref tx) = stream_tx {
                    let _ = tx.send(CognitiveEvent::CognitiveStep {
                        step: "Planning".into(),
                        message: format!(
                            "Optimal path: {} (score: {:.3}, risk: {:?})",
                            rp.path_id, rp.composite_score, rp.risk_level
                        ),
                        metrics: Some(CognitiveMetrics {
                            iteration,
                            deviation: 0.0,
                            confidence: rp.success_probability,
                            progress: (iteration as f32 / max_iterations as f32) * 100.0,
                            token_cost: 0,
                            stability: rp.confidence,
                            info_gain: 0.0,
                        }),
                    });
                }
            }
            // ──────────────────────────────────────────────────────────────────

            // Push Predict Event
            if let Some(ref tx) = stream_tx {
                let _ = tx.send(CognitiveEvent::Predict {
                    prediction: serde_json::to_value(&prediction).unwrap_or_default(),
                });
            }

            let exec_ctx = if iteration == 0 {
                initial_ctx_for_iter.clone()
            } else {
                None
            };

            if let Some(mut status) = self.execution_manager.active_executions.get_mut(sop_id) {
                status.status = "Executing".into();
                status
                    .thought_logs
                    .push(format!("Iteration {}: Executing SOP nodes...", iteration));
                status.last_updated = iter_now;
            }
            broadcast_fn(
                sop_id,
                ExecutionEvent::CognitiveStep {
                    step: "Executing".into(),
                    message: format!("Iteration {}: Executing SOP nodes...", iteration),
                    metrics: Some(CognitiveMetrics {
                        iteration,
                        deviation: 0.0,
                        confidence: prediction.success_probability,
                        progress: ((iteration as f32 + 0.5) / max_iterations as f32) * 100.0,
                        token_cost: 0,
                        stability: prediction.predicted_stability,
                        info_gain: 0.0,
                    }),
                },
            );

            // ── Trace Recording Setup ──────────────────────────────────────
            let (trace_tx, mut trace_rx) = mpsc::unbounded_channel::<ExecutionEvent>();
            let store_clone = self.execution_manager.state_store.clone();
            let sop_id_clone = sop_id.to_string();
            let user_event_tx = event_tx.clone();

            tokio::spawn(async move {
                while let Some(event) = trace_rx.recv().await {
                    // 1. Record to structured storage
                    match &event {
                        ExecutionEvent::NodeCompleted {
                            node_name,
                            output,
                            fuel_consumed,
                            duration_ms,
                        } => {
                            let _ = store_clone
                                .record_trace_event(
                                    &sop_id_clone,
                                    node_name,
                                    "ok",
                                    Some(output.to_string()),
                                    *fuel_consumed,
                                    *duration_ms,
                                )
                                .await;
                        }
                        ExecutionEvent::NodeFailed {
                            node_name,
                            error,
                            fuel_consumed,
                            duration_ms,
                        } => {
                            let _ = store_clone
                                .record_trace_event(
                                    &sop_id_clone,
                                    node_name,
                                    "error",
                                    Some(error.clone()),
                                    *fuel_consumed,
                                    *duration_ms,
                                )
                                .await;
                        }
                        _ => {}
                    }
                    // 2. Forward to original event stream
                    if let Some(ref tx) = user_event_tx {
                        let _ = tx.send(event);
                    }
                }
            });

            let final_ctx = match executor
                .execute_with_context(
                    Arc::clone(&current_ref),
                    current_start,
                    exec_ctx,
                    Some(trace_tx),
                    self.execution_manager.tool_router.clone(),
                    Some(Arc::new(SisyphusSupervisor::new(
                        self.intelligence_manager.intel.reflector.clone(),
                        prediction.clone(),
                        Arc::clone(&current_ref),
                        convergence_threshold,
                        self.experience_bank.clone(),
                        goal_text.unwrap_or(sop_id).to_string(),
                    ))),
                    micro_reflector.clone(),
                    self.visual_manager.clone(),
                )
                .await
            {
                Ok(ctx) => ctx,
                Err(CoreError::StrategicPivot(reason)) => {
                    pivot_reason = Some(reason);
                    Arc::new(ExecutionContext::new())
                }
                Err(e) => return Err(CoreError::Internal(e.to_string())),
            };

            let result = if let Some(ref reason) = pivot_reason {
                NodeOutput::from(Value::String(format!("Strategic Pivot: {}", reason)))
            } else {
                let start_name = current_ref
                    .graph
                    .node_weight(current_start)
                    .map(|n| n.name().to_string())
                    .ok_or_else(|| CoreError::Internal("Start node not found".into()))?;
                final_ctx
                    .get_result(&start_name)
                    .ok_or(CoreError::NoResultProduced)?
            };

            // Push Execute Event
            if let Some(ref tx) = stream_tx {
                let _ = tx.send(CognitiveEvent::Execute {
                    node: current_start.index().to_string(),
                    result: result.clone(),
                });
            }

            final_result = result.clone();

            // 3. Reflect
            let mut trace = String::with_capacity(current_ref.graph.node_count() * 64);
            for node in current_ref.graph.node_indices() {
                if let Some(nw) = current_ref.graph.node_weight(node) {
                    trace.push_str(nw.name());
                    trace.push(':');
                    if let Some(out) = final_ctx.get_result(nw.name()) {
                        trace.push_str(&out.to_string());
                    }
                    trace.push('\n');
                }
            }

            let mut reflection = if let Some(ref reason) = pivot_reason {
                sisyphus_intel::reflect::ReflectionReport {
                    deviation_score: convergence_threshold + 0.1,
                    suggested_improvements: vec![reason.clone()],
                    original_sop_id: sop_id.to_string(),
                    evolved_sop_structure: None,
                    records: vec![],
                }
            } else {
                self.intelligence_manager
                    .reflect_on_trace(&trace, &prediction, &final_ctx)
                    .await
                    .map_err(|e| CoreError::Internal(e.to_string()))?
            };

            // ── Deep Reflection Analysis ──────────────────────────────────────
            // Use the deep reflector for multi-dimensional deviation analysis
            {
                let mut deep_reflector = self.deep_reflector.write();
                let deep_report = deep_reflector.analyze(&reflection, &prediction);

                tracing::info!(
                    sop_id,
                    overall_score = deep_report.overall_score,
                    corrections = deep_report.corrections.len(),
                    patterns = deep_report.patterns.len(),
                    "Deep reflection: {} dimensions analyzed, {} corrections suggested",
                    deep_report.dimensions.len(),
                    deep_report.corrections.len()
                );

                // Apply high-priority corrections as additional suggestions
                for correction in &deep_report.corrections {
                    if correction.priority
                        <= sisyphus_intel::deep_reflector::CorrectionPriority::High
                    {
                        reflection.suggested_improvements.push(format!(
                            "[DEEP {}] {}: {}",
                            format!("{:?}", correction.priority).to_uppercase(),
                            correction.target_node,
                            correction.description
                        ));
                    }
                }

                // Record failure patterns from the reflection
                if let Some(ref reason) = pivot_reason {
                    let mut lib = self.failure_pattern_library.write();
                    lib.record_failure(
                        reflection
                            .records
                            .first()
                            .map(|r| r.node.as_str())
                            .unwrap_or("unknown"),
                        sisyphus_intel::failure_pattern_evolver::FailureCategory::LogicError,
                        reason,
                    );
                }

                // Push Deep Reflect Event
                if let Some(ref tx) = stream_tx {
                    let _ = tx.send(CognitiveEvent::CognitiveStep {
                        step: "DeepReflect".into(),
                        message: format!(
                            "Score: {:.3} | {} corrections, {} patterns detected",
                            deep_report.overall_score,
                            deep_report.corrections.len(),
                            deep_report.patterns.len()
                        ),
                        metrics: Some(CognitiveMetrics {
                            iteration,
                            deviation: deep_report.overall_score,
                            confidence: 1.0 - deep_report.overall_score,
                            progress: (iteration as f32 / max_iterations as f32) * 100.0,
                            token_cost: 0,
                            stability: prediction.predicted_stability,
                            info_gain: 0.0,
                        }),
                    });
                }
            }
            // ──────────────────────────────────────────────────────────────────

            // Push Reflect Event
            if let Some(ref tx) = stream_tx {
                let _ = tx.send(CognitiveEvent::Reflect {
                    report: serde_json::to_value(&reflection).unwrap_or_default(),
                });
            }

            // Critique
            let critique = self
                .intelligence_manager
                .critique_reflection(&reflection, &prediction)
                .await
                .map_err(|e| CoreError::Internal(e.to_string()))?;
            reflection
                .suggested_improvements
                .push(format!("[CRITIQUE] {}", critique.suggested_refinement));

            // Distill semantic insights from reflection and record to bank
            let insight = if !reflection.suggested_improvements.is_empty() {
                Some(format!(
                    "Deviation {:.4}. Key lessons: {}",
                    reflection.deviation_score,
                    reflection
                        .suggested_improvements
                        .iter()
                        .take(3)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("; ")
                ))
            } else {
                None
            };

            let exp_to_record = sisyphus_types::experience::ExperienceTuple {
                context_text: goal_text.unwrap_or(sop_id).to_string(),
                context_hash: format!("{:x}", md5::compute(goal_text.unwrap_or(sop_id))),
                action_id: sop_id.to_string(),
                outcome_success: reflection.deviation_score < 0.2,
                deviation_score: reflection.deviation_score,
                cost: 0,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                project_id: None,
                visual_embedding: None,
                embedding: None,
                insight,
            };
            let _ = self.experience_bank.record(exp_to_record).await;

            // 5. Evolve
            let mut evolution_insights = ctx
                .notepad
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .read_section("cognitive.insights")
                .cloned();

            // Enhance evolution insights using the experience bank
            let exp_query = sisyphus_types::experience::ExperienceQuery {
                query_text: goal_text.unwrap_or(sop_id).to_string(),
                context_hash: format!("{:x}", md5::compute(goal_text.unwrap_or(sop_id))),
                action_id: None,
                project_id: None,
                visual_embedding: None,
                limit: 5,
            };

            if let Ok(exps) = self.experience_bank.query_similar(exp_query).await {
                let mut exp_hints = Vec::new();
                for exp in exps {
                    if exp.outcome_success {
                        exp_hints.push(format!(
                            "Success Case [{}]: Goal: '{}', Deviation: {:.4}{}",
                            exp.action_id,
                            exp.context_text,
                            exp.deviation_score,
                            exp.insight
                                .as_ref()
                                .map(|i| format!(" | Insight: {}", i))
                                .unwrap_or_default()
                        ));
                    } else {
                        exp_hints.push(format!(
                            "Failure Case [{}]: Goal: '{}', Deviation: {:.4}{}",
                            exp.action_id,
                            exp.context_text,
                            exp.deviation_score,
                            exp.insight
                                .as_ref()
                                .map(|i| format!(" | Insight: {}", i))
                                .unwrap_or_default()
                        ));
                    }
                }
                if !exp_hints.is_empty() {
                    let hints_str =
                        format!("Semantic Historical Guidance:\n{}", exp_hints.join("\n"));
                    evolution_insights = Some(match evolution_insights {
                        Some(existing) => format!("{}\n{}", existing, hints_str),
                        None => hints_str,
                    });
                }
            }

            // ── Failure Pattern Evolution ──────────────────────────────────────
            // Use the failure pattern evolver to generate targeted evolution operations
            // based on accumulated failure patterns from the failure pattern library
            {
                let evolver = FailurePatternEvolver::new(self.failure_pattern_library.clone());

                // Generate evolution operations from failure patterns using the reflection
                let failure_ops = evolver.generate_evolution_ops(&reflection);
                if !failure_ops.is_empty() {
                    tracing::info!(
                        sop_id,
                        ops = failure_ops.len(),
                        "Failure pattern evolver generated {} evolution operations",
                        failure_ops.len()
                    );

                    // Append failure-informed hints to evolution insights
                    let failure_hints: Vec<String> = failure_ops
                        .iter()
                        .map(|op| format!("Failure-Driven Evolution: {:?}", op))
                        .collect();

                    evolution_insights = Some(match evolution_insights {
                        Some(existing) => format!(
                            "{}\nFailure-Driven Evolution:\n{}",
                            existing,
                            failure_hints.join("\n")
                        ),
                        None => format!("Failure-Driven Evolution:\n{}", failure_hints.join("\n")),
                    });
                }
            }
            // ──────────────────────────────────────────────────────────────────

            let evolved_graph = self
                .intelligence_manager
                .intel
                .evolver
                .evolve(
                    Arc::clone(&current_ref),
                    &reflection,
                    &prediction,
                    evolution_insights.as_deref(),
                )
                .await
                .ok();

            // --- VERIFICATION STEP ---
            // Verify if the evolved graph is actually an improvement over the current one.
            let mut final_evolved_graph = None;
            if let Some(eg) = evolved_graph {
                let verification_prediction = self
                    .intelligence_manager
                    .intel
                    .simulator
                    .simulate(
                        Arc::clone(&eg),
                        &ctx,
                        project_id.unwrap_or("default"),
                        sop_id,
                        goal_text,
                    )
                    .await
                    .ok();

                if let Some(v_pred) = verification_prediction {
                    // Only accept evolution if the success probability increases or stability improves significantly.
                    if v_pred.success_probability > prediction.success_probability
                        || (v_pred.success_probability >= prediction.success_probability
                            && v_pred.predicted_stability > prediction.predicted_stability)
                    {
                        final_evolved_graph = Some(eg);

                        // Log verification success
                        if let Some(mut status) =
                            self.execution_manager.active_executions.get_mut(sop_id)
                        {
                            status.thought_logs.push(format!(
                                "Iteration {}: Evolution verified. Prob: {:.2} -> {:.2}",
                                iteration,
                                prediction.success_probability,
                                v_pred.success_probability
                            ));
                        }
                    } else {
                        // Log verification rejection
                        if let Some(mut status) =
                            self.execution_manager.active_executions.get_mut(sop_id)
                        {
                            status.thought_logs.push(format!(
                                "Iteration {}: Evolution rejected. New Prob ({:.2}) <= Old Prob ({:.2})", 
                                iteration, v_pred.success_probability, prediction.success_probability
                            ));
                        }
                    }
                } else {
                    // Simulation failed, treat as rejected to be safe
                    if let Some(mut status) =
                        self.execution_manager.active_executions.get_mut(sop_id)
                    {
                        status.thought_logs.push(format!(
                            "Iteration {}: Evolution verification failed (simulation error).",
                            iteration
                        ));
                    }
                }
            }
            // -------------------------

            // Push Evolve Event
            if let Some(ref tx) = stream_tx {
                let _ = tx.send(CognitiveEvent::Evolve {
                    revised_plan: final_evolved_graph
                        .as_ref()
                        .and_then(|g| sisyphus_types::SOPGraphConfig::from_graph(g).ok())
                        .unwrap_or_default(),
                });
            }

            let has_evolved = final_evolved_graph.is_some();
            let _iter_duration = iter_start.elapsed();
            iterations.push(CognitiveIteration {
                iteration,
                prediction: prediction.clone(),
                deviation_score: reflection.deviation_score,
                suggestions: reflection.suggested_improvements.clone(),
                evolved: has_evolved,
            });

            if reflection.deviation_score < convergence_threshold || !has_evolved {
                if !has_evolved && reflection.deviation_score >= convergence_threshold {
                    // --- Tool Gap Detection ---
                    if let Ok(Some(gap)) = self
                        .gap_analyzer
                        .detect_gap(&reflection, &current_ref, goal_text.unwrap_or(sop_id))
                        .await
                    {
                        tracing::warn!(gap_id = %gap.gap_id, "Tool gap detected! Capability missing: {}", gap.required_capability);

                        if let Some(ref tx) = stream_tx {
                            let _ = tx.send(CognitiveEvent::ToolGapDetected {
                                gap: serde_json::to_value(&gap).unwrap_or_default(),
                            });
                        }

                        // --- Trigger Self-Programming Loop ---
                        let sp_manager = Arc::clone(&self.sp_manager);
                        tokio::spawn(async move {
                            if let Err(e) = sp_manager.resolve_gap(gap).await {
                                error!("Self-programming loop failed: {e}");
                            }
                        });
                    }
                }
                final_evolved = final_evolved_graph;
                break;
            }

            if let Some(eg) = final_evolved_graph {
                current_start = eg.start_node;
                own_graph = Some(eg);
                current_ref =
                    Arc::clone(own_graph.as_ref().ok_or_else(|| {
                        CoreError::Internal("own_graph was just set".to_string())
                    })?);
            } else {
                break;
            }

            // Update Cognitive Map for visualization
            let cog_map = Self::build_cog_map(
                sop_id,
                goal_text.unwrap_or(sop_id),
                &current_ref,
                &final_ctx,
                iteration,
                None, // Active node would be the one currently being executed
            )?;
            self.execution_manager
                .cognitive_maps
                .insert(sop_id.to_string(), cog_map);

            // Strategic Shift Analysis: Check for semantic failure clusters
            if let Ok(Some(cluster)) = self
                .pattern_analyzer
                .analyze_failure_clusters(
                    self.experience_bank.as_ref(),
                    goal_text.unwrap_or(sop_id),
                )
                .await
            {
                if let Some(ref tx) = stream_tx {
                    let _ = tx.send(CognitiveEvent::StrategicShift {
                        analysis: serde_json::json!({
                            "cluster": cluster.goal_cluster,
                            "mode": cluster.common_failure_mode,
                            "count": cluster.evidence_count,
                            "suggestion": cluster.suggested_shift
                        }),
                    });
                }

                // Formally submit as a strategic proposal to the roadmap
                let _ = self
                    .roadmap_manager
                    .submit_strategic_shift(
                        project_id.unwrap_or("default"),
                        &cluster.common_failure_mode,
                        cluster.evidence_count,
                        &cluster.suggested_shift,
                    )
                    .await;

                tracing::warn!(
                    cluster = %cluster.goal_cluster,
                    mode = %cluster.common_failure_mode,
                    "Strategic failure cluster detected! Proposal submitted: {}", cluster.suggested_shift
                );
            }
        }

        let converged = iterations
            .last()
            .map(|i| i.deviation_score < convergence_threshold)
            .unwrap_or(false);

        // Push Complete Event
        if let Some(ref tx) = stream_tx {
            let _ = tx.send(CognitiveEvent::Complete {
                final_result: final_result.clone(),
                converged,
            });
        }

        let report_obj = CognitiveReport {
            iterations: iterations.len(),
            converged,
            final_deviation: iterations.last().map(|i| i.deviation_score).unwrap_or(0.0),
            history: iterations,
            traces: vec![], // Simplified for rewrite
        };

        Ok((final_result, report_obj, final_evolved))
    }
}
