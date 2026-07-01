use petgraph::graph::NodeIndex;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::task::JoinSet;

use sisyphus_intel::SisyphusIntel;
use sisyphus_mem::MfsStore;

use sisyphus_roadmap::RoadmapManager;
use sisyphus_tools::{
    agent_channel::AgentChannel, meta_learner::MetaLearner, ContinuousBenchmark,
};
use sisyphus_types::AgentMessage;

use crate::intent_gate::IntentGate;
pub use sisyphus_types::{
    CompositeGraph, DataMapping, ExecutionContext, ExecutionEvent, GraphDependency, NodeOutput,
    SOPGraph, SOPGraphConfig, StateStore, ToolExecutor, WasmSOPNode,
};

pub mod dag;
pub use dag::*;

pub mod executor;
pub use executor::*;

pub mod cognitive_loop;
pub use cognitive_loop::{CognitiveIteration, CognitiveReport};

pub mod intent_gate;

pub mod budget;

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

impl From<sisyphus_types::TypesError> for CoreError {
    fn from(err: sisyphus_types::TypesError) -> Self {
        CoreError::Internal(err.to_string())
    }
}
pub struct SisyphusKernel {
    pub executor: Executor,
    pub state_store: Arc<MfsStore>,
    pub tool_executor: Arc<dyn ToolExecutor>,
    pub intel: Arc<SisyphusIntel>,
    pub llm_client: Arc<dyn sisyphus_tools::LlmClient>,
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
        llm_client: Arc<dyn sisyphus_tools::LlmClient>,
        synthesizer: Arc<dyn sisyphus_intel::FlowSynthesizer>,
    ) -> Self {
        let rm = Arc::new(RoadmapManager::new(state_store.clone()));
        Self {
            executor: Executor::new(),
            state_store,
            tool_executor,
            intel,
            llm_client,
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
            .map_err(|e| CoreError::MfsSaveError(e.to_string()))
    }

    pub async fn load_sop(&self, sop_id: &str) -> Result<SOPGraph, CoreError> {
        let data_val = self
            .state_store
            .get_state_json(&format!("sop:{}", sop_id))
            .await
            .map_err(|e| CoreError::MfsLoadError(e.to_string()))?;

        match data_val {
            None => Err(CoreError::SopNotFound(sop_id.to_string())),
            Some(Value::String(s)) => {
                // legacy format: double-wrapped JSON string
                let config: sisyphus_types::SOPGraphConfig =
                    serde_json::from_str(&s).map_err(|e| CoreError::Internal(e.to_string()))?;
                Ok(SOPGraph::from_config(&config, self.tool_executor.clone())?)
            }
            Some(v) => {
                // new format: direct JSON value
                let config: sisyphus_types::SOPGraphConfig =
                    serde_json::from_value(v).map_err(|e| CoreError::Internal(e.to_string()))?;
                Ok(SOPGraph::from_config(&config, self.tool_executor.clone())?)
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
        
        // 1. Intent Gate: Determine WorkMode based on goal
        let mode = IntentGate::analyze(goal, &self.llm_client).await;
        
        // 2. Synthesize SOP with a specific mode
        let synthesis = self.synthesizer.synthesize(goal, &ctx, mode).await?;
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
            .map_err(|e| CoreError::Internal(format!("Failed to synthesize project context: {}", e)))?;
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
        // Complex tuple type is required because JoinSet element type must be uniform
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
                .ok_or_else(|| CoreError::Internal(format!("Graph {} not found in composite", sop_id)))?;
            let graph = SOPGraph::from_config(config, self.tool_executor.clone())
                .map_err(|e| CoreError::Internal(e.to_string()))?;
            let start_node = graph
                .graph
                .node_indices()
                .next()
                .ok_or_else(|| CoreError::Internal(format!("Graph {} has no nodes", sop_id)))?;
            graph_cache.insert(sop_id.clone(), (graph, start_node));
        }

        // Wrap composite + self in Arcs for parallel spawning
        let composite = Arc::new(composite.clone());
        let self_arc = Arc::new(SisyphusKernel {
            executor: Executor::new(),
            state_store: self.state_store.clone(),
            tool_executor: self.tool_executor.clone(),
            intel: self.intel.clone(),
            llm_client: self.llm_client.clone(),
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
            let (completed_id, outcome) = res.map_err(|e| CoreError::Internal(e.to_string()))?;
            let (node_output, report, _evolved) = outcome?;

            {
                let mut acc = accumulated.lock().unwrap_or_else(|e| e.into_inner());
                acc.insert(completed_id.clone(), node_output.clone());
            }
            results.insert(completed_id.clone(), (node_output, report));

            // Decrement in-degree of dependents and spawn ready ones
            let ready: Vec<String> = {
                let mut deg = in_degree.lock().unwrap_or_else(|e| e.into_inner());
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
                        let dep_ctx = build_dep_ctx_for(&sid, &composite_ref, &acc.lock().unwrap_or_else(|e| e.into_inner()));
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

#[cfg(test)]
mod tests;

