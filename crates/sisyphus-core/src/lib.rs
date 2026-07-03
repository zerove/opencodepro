use dashmap::DashMap;
use petgraph::graph::NodeIndex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::task::JoinSet;

use sisyphus_intel::SisyphusIntel;
use sisyphus_mem::MfsStore;

use sisyphus_roadmap::RoadmapManager;
use sisyphus_tools_build::benchmark::ContinuousBenchmark;
use sisyphus_tools_llm_tools::meta_learner::MetaLearner;
use sisyphus_tools_system::agent_channel::AgentChannel;
use sisyphus_types::AgentMessage;

use crate::intent_gate::IntentGate;
pub use sisyphus_types::{
    CompositeGraph, DataMapping, ExecutionContext, ExecutionEvent, GraphDependency, NodeOutput,
    SOPGraph, SOPGraphConfig, StateStore, ToolExecutor, WasmSOPNode, ExecutorRouter,
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
    async fn propose(&self, key: &str, value: serde_json::Value) -> Result<(), String>;
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ExecutionStatus {
    pub goal_id: String,
    pub status: String,
    pub current_node: Option<String>,
    pub thought_logs: Vec<String>,
    pub progress: f32,
    pub last_updated: u64,
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

impl From<String> for CoreError {
    fn from(err: String) -> Self {
        CoreError::Internal(err)
    }
}

pub struct SisyphusKernel {
    pub executor: Executor,
    pub state_store: Arc<MfsStore>,
    pub tool_router: Arc<ExecutorRouter>,
    pub intel: Arc<SisyphusIntel>,
    pub llm_client: Arc<dyn sisyphus_tools_llm::LlmClient>,
    pub synthesizer: Arc<dyn sisyphus_intel::FlowSynthesizer>,
    pub consensus: Arc<dyn Consensus>,
    /// Inter-agent message bus for pub/sub communication during execution.
    pub channel: AgentChannel,
    /// Performance benchmark tracker that records cognitive iteration timings.
    pub benchmark: Arc<std::sync::Mutex<ContinuousBenchmark>>,
    /// Meta-learner that records execution outcomes for strategy optimization.
    pub meta_learner: Arc<std::sync::Mutex<MetaLearner>>,
    /// Strategic planning manager for Project → Milestone → Phase → Goal hierarchy.
    pub roadmap_manager: Arc<RoadmapManager>,
    /// Proposal manager for handling human-in-the-loop strategic pivots.
    pub proposal_manager: Arc<sisyphus_roadmap::proposal_manager::ProposalManager>,
    /// Phase 3 — Strategic reflection engine (LLM-augmented roadmap analysis).
    pub strategic_reflector:
        Option<Arc<sisyphus_roadmap::strategic_reflection::StrategicReflector>>,
    /// Phase 3 — Autonomous milestone generator (LLM-driven roadmap creation).
    pub milestone_generator: Option<Arc<sisyphus_roadmap::milestone_generator::MilestoneGenerator>>,
    /// Phase 3 — Resource / cost projection engine (analytics-only).
    pub resource_optimizer: sisyphus_roadmap::resource_optimizer::ResourceOptimizer,
    /// Cognitive loop parameters (max_iterations, convergence_threshold).
    pub cognitive_loop_config: sisyphus_types::config::CognitiveLoopConfig,
    /// Registry of active goal executions for real-time monitoring.
    pub active_executions: Arc<DashMap<String, ExecutionStatus>>,
    /// Event subscribers for real-time streaming of goal execution events.
    pub event_subscribers: Arc<DashMap<String, Vec<mpsc::UnboundedSender<ExecutionEvent>>>>,
}

impl SisyphusKernel {
    pub fn new(
        state_store: Arc<MfsStore>,
        tool_executor: Arc<dyn ToolExecutor>,
        intel: Arc<SisyphusIntel>,
        llm_client: Arc<dyn sisyphus_tools_llm::LlmClient>,
        synthesizer: Arc<dyn sisyphus_intel::FlowSynthesizer>,
        meta_learner: Arc<std::sync::Mutex<MetaLearner>>,
        consensus: Arc<dyn Consensus>,
    ) -> Self {
        let rm = Arc::new(RoadmapManager::new(state_store.clone()));
        let pm = Arc::new(sisyphus_roadmap::proposal_manager::ProposalManager::new(state_store.clone()));
        Self {
            executor: Executor::new(),
            state_store,
            tool_router: Arc::new(ExecutorRouter::new(tool_executor)),
            intel,
            llm_client,
            synthesizer,
            consensus,
            channel: AgentChannel::new(),
            benchmark: Arc::new(std::sync::Mutex::new(ContinuousBenchmark::new(
                ".omo/benchmarks",
            ))),
            meta_learner,
            roadmap_manager: rm,
            proposal_manager: pm,
            strategic_reflector: None,
            milestone_generator: None,
            resource_optimizer: sisyphus_roadmap::resource_optimizer::ResourceOptimizer::new(),
            cognitive_loop_config: sisyphus_types::config::CognitiveLoopConfig::default(),
            active_executions: Arc::new(DashMap::new()),
            event_subscribers: Arc::new(DashMap::new()),
        }
    }

    /// Override the cognitive loop configuration (max_iterations, convergence_threshold).
    pub fn with_cognitive_loop_config(mut self, config: sisyphus_types::config::CognitiveLoopConfig) -> Self {
        self.cognitive_loop_config = config;
        self
    }

    /// Attach a StrategicReflector for LLM-augmented roadmap analysis.
    pub fn with_strategic_reflector(
        mut self,
        llm: Arc<dyn sisyphus_tools_llm::metis::LlmClient>,
    ) -> Self {
        self.strategic_reflector = Some(Arc::new(
            sisyphus_roadmap::strategic_reflection::StrategicReflector::new(llm),
        ));
        self
    }

    /// Attach a MilestoneGenerator for LLM-driven roadmap creation.
    pub fn with_milestone_generator(
        mut self,
        llm: Arc<dyn sisyphus_tools_llm::metis::LlmClient>,
    ) -> Self {
        self.milestone_generator = Some(Arc::new(
            sisyphus_roadmap::milestone_generator::MilestoneGenerator::new(llm),
        ));
        self
    }

    /// Register a specialized tool executor under a given prefix (e.g., "fs" for file system).
    pub fn register_tool_executor(
        &self,
        prefix: &str,
        executor: Arc<dyn ToolExecutor>,
    ) -> Result<(), String> {
        self.tool_router.register(prefix, executor);
        Ok(())
    }

    pub async fn save_sop(&self, sop_id: &str, graph: &SOPGraph) -> Result<(), CoreError> {
        let config = sisyphus_types::SOPGraphConfig::from_graph(graph);
        let value =
            serde_json::to_value(&config).map_err(|e| CoreError::Internal(e.to_string()))?;
        
        // 分布式同步：首先通过共识协议提案
        if self.consensus.is_leader().await {
            if let Err(e) = self.consensus.propose(sop_id, value.clone()).await {
                tracing::warn!(sop_id = %sop_id, error = %e, "Consensus proposal failed, falling back to local save");
            }
        }

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
                Ok(SOPGraph::from_config(&config, self.tool_router.clone())?)
            }
            Some(v) => {
                // new format: direct JSON value
                let config: sisyphus_types::SOPGraphConfig =
                    serde_json::from_value(v).map_err(|e| CoreError::Internal(e.to_string()))?;
                Ok(SOPGraph::from_config(&config, self.tool_router.clone())?)
            }
        }
    }

    /// Broadcast an execution event to all subscribers of a specific SOP.
    pub fn broadcast_event(&self, sop_id: &str, event: ExecutionEvent) {
        if let Some(subscribers) = self.event_subscribers.get(sop_id) {
            let mut subs = subscribers.clone();
            subs.retain(|tx| {
                tx.send(event.clone()).is_ok()
            });
            // Update the registry to remove closed channels
            self.event_subscribers.insert(sop_id.to_string(), subs);
        }
    }

    /// Execute multiple natural-language goals as a composite workflow.
    ///
    /// Each goal is synthesized independently into a sub-graph, then all
    /// are executed as a CompositeGraph (no cross-dependencies, so they
    /// run as independent parallel pipelines).
    pub async fn execute_goals(&self, goals: &[&str]) -> Result<CompositeOutput, CoreError> {
        let ctx = ExecutionContext::new();
        let composite =
            sisyphus_intel::synthesize_composite(goals, &*self.synthesizer, &ctx).await
                .map_err(|e| CoreError::Internal(e.to_string()))?;
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
    ) -> Result<(NodeOutput, String), CoreError> {
        let ctx = ExecutionContext::new();
        
        println!("DEBUG: [execute_goal] Analyzing intent for goal: {}", goal);
        let mode = IntentGate::analyze(goal, &self.llm_client).await;
        println!("DEBUG: [execute_goal] Intent analyzed. Mode: {:?}", mode);
        
        println!("DEBUG: [execute_goal] Synthesizing SOP...");
        let synthesis = self.synthesizer.synthesize(goal, &ctx, mode).await
            .map_err(|e| CoreError::Internal(e.to_string()))?;
        println!("DEBUG: [execute_goal] Synthesis complete. Plan: {}", synthesis.plan_description);
        
        let graph = synthesis.graph;
        let plan_desc = synthesis.plan_description;
    
        println!("DEBUG: [execute_goal] Entering cognitive loop...");
        let (res, report, _) = self
            .execute_with_cognition(&graph, sop_id, None, event_tx)
            .await
            .map_err(|e| CoreError::Internal(e.to_string()))?;
        println!("DEBUG: [execute_goal] Cognitive loop completed.");
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
    ) -> Result<(NodeOutput, String), CoreError> {
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
    ) -> Result<(NodeOutput, String, Option<sisyphus_roadmap::RoadmapHealth>), CoreError> {
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
            Err(e) => (false, None, Some(e.to_string())),
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
                // 1. Notify that reflection is complete
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

                // 2. Attempt to generate a formal proposal for a pivot
                if let Some(proposal) = reflector.propose_pivot(&reflection, project_id) {
                    let prop_id = proposal.proposal_id.clone();
                    if let Err(e) = self.proposal_manager.submit_proposal(proposal.clone()).await {
                        tracing::warn!(project_id = %project_id, error = %e, "Failed to submit strategic proposal");
                    } else {
                        let _ = self.channel.publish(
                            "strategic_events",
                            AgentMessage {
                                from: "kernel".into(),
                                to: "*".into(),
                                kind: "StrategicProposalCreated".into(),
                                payload: serde_json::json!({
                                    "proposal_id": prop_id,
                                    "project_id": project_id,
                                    "description": proposal.expected_impact,
                                }),
                                correlation_id: format!("{}-proposal-{}", project_id, prop_id),
                            },
                        );
                    }
                }

                Some(reflection)
            }
            Err(e) => {
                tracing::warn!(project_id = %project_id, error = %e, "Strategic reflection failed");
                None
            }
        }
    }

    /// Initialize MCP tool executors from the provided configuration.
    pub async fn init_mcp_executors(&self, config: &sisyphus_types::config::McpConfig) -> Result<(), String> {
        // 1. Handle stdio servers
        for (prefix, cmd_str) in &config.stdio_servers {
            let parts: Vec<&str> = cmd_str.split_whitespace().collect();
            if parts.is_empty() {
                tracing::warn!(prefix = %prefix, "Empty MCP stdio command, skipping");
                continue;
            }
            let command = parts[0];
            let args = &parts[1..];

            match sisyphus_tools_executor::executor_mcp::MCPToolExecutor::from_stdio(command, args).await {
                Ok(executor) => {
                    self.register_tool_executor(prefix, Arc::new(executor))?;
                    tracing::info!(prefix = %prefix, "Registered MCP stdio executor");
                }
                Err(e) => {
                    tracing::error!(prefix = %prefix, error = %e, "Failed to initialize MCP stdio executor");
                }
            }
        }

        // 2. Handle HTTP servers
        for (prefix, url) in &config.http_servers {
            match sisyphus_tools_executor::executor_mcp::MCPToolExecutor::from_url(url).await {
                Ok(executor) => {
                    self.register_tool_executor(prefix, Arc::new(executor))?;
                    tracing::info!(prefix = %prefix, "Registered MCP HTTP executor");
                }
                Err(e) => {
                    tracing::error!(prefix = %prefix, error = %e, "Failed to initialize MCP HTTP executor");
                }
            }
        }
        Ok(())
    }

    /// Approve a strategic proposal and automatically apply its changes to the roadmap.
    pub async fn approve_and_apply_proposal(
        &self,
        proposal_id: &str,
        reviewer: &str,
    ) -> Result<(), CoreError> {
        // 1. Retrieve the proposal
        let proposal = self.proposal_manager.get_proposal(proposal_id).await
            .map_err(|e| CoreError::Internal(e.to_string()))?
            .ok_or_else(|| CoreError::Internal(format!("Proposal {} not found", proposal_id)))?;

        // 2. Mark as approved
        self.proposal_manager.approve_proposal(proposal_id, reviewer).await
            .map_err(|e| CoreError::Internal(e.to_string()))?;

        // 3. Apply the structured action to the roadmap
        self.roadmap_manager.apply_proposal_action(&proposal.project_id, &proposal.action)
            .await
            .map_err(|e| CoreError::Internal(e.to_string()))?;

        // 4. Notify the system
        let _ = self.channel.publish(
            "strategic_events",
            AgentMessage {
                from: "kernel".into(),
                to: "*".into(),
                kind: "StrategicProposalApplied".into(),
                payload: serde_json::json!({
                    "proposal_id": proposal_id,
                    "project_id": proposal.project_id,
                    "action": proposal.action,
                }),
                correlation_id: format!("{}-apply-{}", proposal.project_id, proposal_id),
            },
        );

        Ok(())
    }
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
    ) -> Result<CompositeOutput, CoreError> {
        let order = if composite.execution_order.is_empty() {
            DependencyResolver::resolve(composite)?
        } else {
            composite.execution_order.clone()
        };

        let order_len = order.len();
        let mut results: HashMap<String, (NodeOutput, String)> = HashMap::with_capacity(order_len);
        let accumulated: Arc<DashMap<String, NodeOutput>> =
            Arc::new(DashMap::with_capacity(order_len));
        #[allow(clippy::type_complexity)]
        // Complex tuple type is required because JoinSet element type must be uniform
        let mut join_set: JoinSet<(
            String,
            Result<(NodeOutput, String, Option<SOPGraph>), CoreError>,
        )> = JoinSet::new();

        // Build in-degree map from the dependency DAG
        let in_degree: Arc<DashMap<String, usize>> =
            Arc::new(DashMap::with_capacity(order_len));
        for sop_id in &order {
            in_degree.entry(sop_id.clone()).or_insert(0);
        }
        for dep in &composite.dependencies {
            *in_degree.entry(dep.target_sop_id.clone()).or_insert(0) += 1;
        }

        // Pre-build all SOPGraphs so they are not rebuilt per spawn
        let mut graph_cache: HashMap<String, (SOPGraph, NodeIndex)> =
            HashMap::with_capacity(order_len);
        for sop_id in &order {
            let config = composite
                .graphs
                .get(sop_id)
                .ok_or_else(|| CoreError::Internal(format!("Graph {} not found in composite", sop_id)))?;
            let graph = SOPGraph::from_config(config, self.tool_router.clone())
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
            tool_router: self.tool_router.clone(),
            intel: self.intel.clone(),
            llm_client: self.llm_client.clone(),
            synthesizer: self.synthesizer.clone(),
            consensus: self.consensus.clone(),
            channel: AgentChannel::new(),
            benchmark: self.benchmark.clone(),
            meta_learner: self.meta_learner.clone(),
            roadmap_manager: self.roadmap_manager.clone(),
            proposal_manager: self.proposal_manager.clone(),
            strategic_reflector: self.strategic_reflector.clone(),
            milestone_generator: self.milestone_generator.clone(),
            resource_optimizer: sisyphus_roadmap::resource_optimizer::ResourceOptimizer::new(),
            cognitive_loop_config: self.cognitive_loop_config.clone(),
            active_executions: Arc::new(DashMap::new()),
            event_subscribers: Arc::new(DashMap::new()),
        });

        // Spawn all ready graphs (in-degree == 0)
        for sop_id in &order {
            if in_degree.get(sop_id).map(|d| *d).unwrap_or(0) == 0 {
                if let Some((graph, start_node)) = graph_cache.remove(sop_id) {
                    let kernel = Arc::clone(&self_arc);
                    let acc = Arc::clone(&accumulated);
                    let composite_ref = Arc::clone(&composite);
                    let tx = event_tx.clone();
                    let sid = sop_id.clone();
                    join_set.spawn(async move {
                        let dep_ctx = build_dep_ctx_for(&sid, &composite_ref, &*acc);
                        let result = kernel
                            .execute_with_cognition(&graph, &sid, dep_ctx, tx)
                            .await;
                        (sid, result)
                    });
                }
            }
        }

        // Process completed graphs and spawn newly-ready dependents
        while let Some(res) = join_set.join_next().await {
            let (completed_id, outcome) = res.map_err(|e| CoreError::Internal(e.to_string()))?;
            let (node_output, report, _evolved) = outcome?;

            accumulated.insert(completed_id.clone(), node_output.clone());
            results.insert(completed_id.clone(), (node_output, report));

            // Decrement in-degree of dependents and spawn ready ones
            let ready: Vec<String> = {
                let mut ready = Vec::new();
                for dep in &composite.dependencies {
                    if dep.source_sop_id == completed_id {
                        if let Some(mut d) = in_degree.get_mut(&dep.target_sop_id) {
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
                        let dep_ctx = build_dep_ctx_for(&sid, &composite_ref, &*acc);
                        let result = kernel
                            .execute_with_cognition(&graph, &sid, dep_ctx, tx)
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
