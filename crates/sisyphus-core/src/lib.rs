use dashmap::DashMap;

use petgraph::graph::NodeIndex;

use serde::{Deserialize, Serialize};


use std::collections::HashMap;

use std::sync::Arc;

use thiserror::Error;

use tokio::sync::mpsc;

use tokio::task::JoinSet;



use sisyphus_intel::SisyphusIntel;

use sisyphus_mem::{MfsStore, SopSqlStore};



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



pub mod supervisor;

pub use supervisor::*;



pub mod cognitive_loop;

pub use strategic_manager::{CognitiveIteration, CognitiveReport};



pub mod execution_manager;

pub mod strategic_manager;
pub mod intervention_manager;
pub use intervention_manager::*;
pub mod internalization_manager;
pub use internalization_manager::*;
pub mod self_programming_manager;
pub use self_programming_manager::*;

pub mod swarm_coordinator;
pub use swarm_coordinator::*;





pub mod cognitive_dashboard;

pub use execution_manager::*;



pub mod intelligence_manager;

pub use intelligence_manager::*;



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

    #[error("Strategic Pivot triggered: {0}")]

    StrategicPivot(String),

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

    pub intelligence_manager: Arc<IntelligenceManager>,

    pub execution_manager: Arc<ExecutionManager>,

    pub strategic_manager: Arc<strategic_manager::StrategicManager>,
    pub swarm_coordinator: Arc<SwarmCoordinator>,
    pub logical_simulator: Arc<dyn sisyphus_intel::simulation::LogicalSimulator>,
    pub consensus: Arc<dyn Consensus>,
    pub intervention_manager: Arc<crate::intervention_manager::InterventionManager>,

    /// Inter-agent message bus for pub/sub communication during execution.

    pub channel: Arc<AgentChannel>,

    /// Performance benchmark tracker that records cognitive iteration timings.

    pub benchmark: Arc<parking_lot::Mutex<ContinuousBenchmark>>,

    /// Meta-learner that records execution outcomes for strategy optimization.

    pub meta_learner: Arc<parking_lot::Mutex<MetaLearner>>,

    /// Strategic planning manager for Project → Milestone → Phase → Goal hierarchy.

    pub roadmap_manager: Arc<RoadmapManager>,

    /// Proposal manager for handling human-in-the-loop strategic pivots.

    pub proposal_manager: Arc<sisyphus_roadmap::proposal_manager::ProposalManager>,

    pub experience_bank: Arc<dyn sisyphus_types::experience::ExperienceBank>,

    pub micro_reflector: Option<Arc<dyn sisyphus_types::MicroReflector>>,

    pub test_executor: Arc<dyn sisyphus_types::TestExecutor>,

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
        sop_store: Arc<SopSqlStore>,
        _vector_store: Arc<sisyphus_mem::VectorMemoryStore>,
        experience_bank: Arc<dyn sisyphus_types::experience::ExperienceBank>,
        tool_executor: Arc<dyn ToolExecutor>,
        intel: Arc<SisyphusIntel>,
        llm_client: Arc<dyn sisyphus_tools_llm::LlmClient>,
        fast_llm_client: Arc<dyn sisyphus_tools_llm::LlmClient>,
        synthesizer: Arc<dyn sisyphus_intel::FlowSynthesizer>,
        meta_learner: Arc<parking_lot::Mutex<MetaLearner>>,
        consensus: Arc<dyn Consensus>,
        sql_store: Arc<sisyphus_mem::RoadmapSqlStore>,
        _embedding_client: Arc<dyn sisyphus_tools_llm::EmbeddingClient>,
    ) -> Self {
        let pm = Arc::new(sisyphus_roadmap::proposal_manager::ProposalManager::new(state_store.clone()));
        let rm = Arc::new(RoadmapManager::new(state_store.clone(), sql_store, pm.clone()));
        
        let test_executor = Arc::new(sisyphus_tools_executor::executor::test_cmds::TestExecutor::new());
        
        let intelligence_manager = Arc::new(IntelligenceManager::new(
            Arc::new(SisyphusIntel::new(
                intel.simulator.clone(),
                Arc::new(sisyphus_intel::simulation::LlmLogicalSimulator::new(llm_client.clone())),
                intel.reflector.clone(),
                intel.evolver.clone(),
            )),
            synthesizer,
            llm_client.clone(),
        ));

        let knowledge_manager = Arc::new(sisyphus_knowledge::AssetManager::new(
            std::path::PathBuf::from(".sisyphus/knowledge"),
        ));

        let intervention_manager = Arc::new(crate::intervention_manager::InterventionManager::new());
        let internalization_manager = Arc::new(crate::internalization_manager::InternalizationManager::new(
            intelligence_manager.llm_client.clone(),
            knowledge_manager.clone(),
        ));
        let visual_manager = Arc::new(sisyphus_visual::VisualPerceptionManager::new(
            Arc::new(sisyphus_visual::SisyphusVisualProvider::new(intelligence_manager.llm_client.clone())),
            intelligence_manager.llm_client.clone(),
        ));

        let execution_manager = Arc::new(ExecutionManager::new(
            state_store,
            sop_store,
            tool_executor,
            test_executor.clone(),
            intervention_manager.clone(),
            internalization_manager.clone(),
            Some(visual_manager.clone()),
        ));

        let channel = Arc::new(AgentChannel::new());
        let benchmark = Arc::new(parking_lot::Mutex::new(ContinuousBenchmark::new(
            ".omo/benchmarks",
        )));
        
        let strategic_manager = Arc::new(strategic_manager::StrategicManager::new(
            intelligence_manager.clone(),
            execution_manager.clone(),
            sisyphus_types::config::CognitiveLoopConfig::default(),
            channel.clone(),
            benchmark.clone(),
            meta_learner.clone(),
            experience_bank.clone(),
            knowledge_manager.clone(),
            rm.clone(),
            intervention_manager.clone(),
            internalization_manager.clone(),
            Some(visual_manager.clone()),
        ));
        
        let swarm_coordinator = Arc::new(SwarmCoordinator::new(
            strategic_manager.clone(),
            intelligence_manager.clone(),
            channel.clone(),
        ));
        
        let logical_simulator = Arc::new(sisyphus_intel::simulation::LlmLogicalSimulator::new(llm_client.clone()));
        
        // Spawn default swarm agents for the Audit Loop
        swarm_coordinator.spawn_agent(AgentRole::Architect, "architect-0".into());
        swarm_coordinator.spawn_agent(AgentRole::Auditor, "auditor-0".into());

        let micro_reflector = Arc::new(sisyphus_intel::micro_reflector::LlmMicroReflector::new(fast_llm_client));


        Self {
            execution_manager,
            intelligence_manager,
            strategic_manager,
            swarm_coordinator,
            logical_simulator,
            consensus,
            intervention_manager,
            channel,
            benchmark,
            meta_learner,
            roadmap_manager: rm,
            proposal_manager: pm,
            experience_bank,
            micro_reflector: Some(micro_reflector),
            test_executor,
            strategic_reflector: None,
            milestone_generator: None,
            resource_optimizer: sisyphus_roadmap::resource_optimizer::ResourceOptimizer::new(),
        }
    }


    /// Override the cognitive loop configuration (max_iterations, convergence_threshold).

    pub fn with_cognitive_loop_config(self, config: sisyphus_types::config::CognitiveLoopConfig) -> Self {

        *self.strategic_manager.config.write() = config;

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

        self.execution_manager.tool_router.register(prefix, executor);

        Ok(())

    }



    pub async fn cache_plan(&self, plan_id: &str, plan: &sisyphus_types::CognitivePlan) -> Result<(), CoreError> {

        let json = serde_json::to_value(plan).map_err(|e| CoreError::Internal(e.to_string()))?;

        self.execution_manager.state_store.set_state_json("sys", &format!("plan_cache:{}", plan_id), json).await

            .map_err(|e| CoreError::Internal(e.to_string()))

    }



    pub async fn get_cached_plan(&self, plan_id: &str) -> Result<Option<sisyphus_types::CognitivePlan>, CoreError> {

        let val = self.execution_manager.state_store.get_state_json("sys", &format!("plan_cache:{}", plan_id)).await

            .map_err(|e| CoreError::Internal(e.to_string()))?;

        

        if let Some(v) = val {

            let plan = serde_json::from_value(v).map_err(|e| CoreError::Internal(e.to_string()))?;

            Ok(Some(plan))

        } else {

            Ok(None)

        }

    }



    pub async fn handle_feedback(

        &self,

        feedback: sisyphus_types::ExecutionFeedback,

        project_id: &str,

    ) -> Result<sisyphus_types::EvolutionResponse, CoreError> {

        tracing::info!(

            plan_id = %feedback.plan_id,

            step_id = %feedback.step_id,

            status = ?feedback.status,

            "Handling execution feedback"

        );



        let mut trace = String::new();

        if let Ok(history) = self.execution_manager.state_store.get_state_json(project_id, &format!("history:{}", feedback.plan_id)).await {

            if let Some(val) = history {

                trace = val.to_string();

            }

        }



        let sop = self.load_sop(&feedback.plan_id, project_id).await.map_err(|e| CoreError::Internal(e.to_string()))?;

        let prediction = self.intelligence_manager.predict_outcome(sop, &ExecutionContext::new(), project_id, &feedback.plan_id).await

            .map_err(|e| CoreError::Internal(e))?;



        let reflection = self.intelligence_manager.reflect_on_trace(&trace, &prediction, &ExecutionContext::new()).await

            .map_err(|e| CoreError::Internal(format!("Reflection failed: {e}")))?;



        let should_pivot = feedback.status == sisyphus_types::FeedbackStatus::Failed 

            || reflection.deviation_score > 0.7;



        let mut revised_plan = None;

        if should_pivot {

            tracing::warn!(plan_id = %feedback.plan_id, "Strategic pivot triggered by feedback");

            let augmented_goal = format!(

                "Original Goal: {}\nFailure in step {}: {}\nSuggested Refinement: {}",

                feedback.plan_id, 

                feedback.step_id, 

                feedback.error.as_deref().unwrap_or("Unknown error"),

                reflection.suggested_improvements.first().cloned().unwrap_or_default()

            );



            let ctx = sisyphus_types::ExecutionContext::new();

            let synthesis = self.intelligence_manager.synthesize_goal(

                &augmented_goal, 

                &ctx, 

                project_id,
                sisyphus_types::WorkMode::Standard,

                None

            ).await.map_err(|e| CoreError::Internal(e))?;



            let config = sisyphus_types::SOPGraphConfig::from_graph(&synthesis.graph);

            revised_plan = Some(config.into_cognitive_plan(feedback.plan_id.clone(), augmented_goal.clone()));

        }



        Ok(sisyphus_types::EvolutionResponse {

            should_pivot,

            revised_plan,

            reflection_report: reflection.suggested_improvements.first().cloned().unwrap_or_default(),

        })

    }



    pub async fn capture_success(

        &self,

        plan_id: &str,

        project_id: &str,

        final_plan: sisyphus_types::CognitivePlan,

    ) -> Result<(), CoreError> {

        tracing::info!(plan_id = %plan_id, "Capturing successful cognitive plan as SOP");



        let mut graph = SOPGraph::new();

        let mut id_map = HashMap::new();



        for step in &final_plan.steps {

            let tool_id = match &step.action {

                sisyphus_types::CognitiveAction::ToolCall { tool_id, .. } => tool_id.clone(),

                sisyphus_types::CognitiveAction::FileEdit { path, .. } => {

                    format!("file_edit:{}", path)

                },

                sisyphus_types::CognitiveAction::ShellExecute { .. } => "shell_exec_tool".into(),

            };

            let idx = graph.add_node(Arc::new(WasmSOPNode {

                name: step.name.clone(),

                tool_id,

                fuel_limit: 1_000_000,

                executor: self.execution_manager.tool_router.clone(),

            }));

            id_map.insert(step.id.clone(), idx);

        }



        for edge in &final_plan.edges {

            if let (Some(&src), Some(&tgt)) = (id_map.get(&edge.from), id_map.get(&edge.to)) {

                graph.add_edge(src, tgt);

            }

        }



        let sop_id = format!("exp_{}", plan_id);

        self.save_sop(&sop_id, project_id, &graph).await

    }



    pub async fn save_sop(&self, sop_id: &str, project_id: &str, graph: &SOPGraph) -> Result<(), CoreError> {

        let config = sisyphus_types::SOPGraphConfig::from_graph(graph);

        let value = serde_json::to_value(&config).map_err(|e| CoreError::Internal(e.to_string()))?;



        if self.consensus.is_leader().await {

            if let Err(e) = self.consensus.propose(sop_id, value.clone()).await {

                tracing::warn!(sop_id = %sop_id, error = %e, "Consensus proposal failed, falling back to local save");

            }

        }



        self.execution_manager.save_sop(sop_id, project_id, &graph).await

    }



    pub async fn load_sop(&self, sop_id: &str, project_id: &str) -> Result<Arc<SOPGraph>, CoreError> {

        self.execution_manager.load_sop(sop_id, project_id).await

    }



    pub fn broadcast_event(&self, sop_id: &str, event: ExecutionEvent) {

        crate::cognitive_dashboard::CognitiveDashboard::render(&event);

        self.execution_manager.broadcast_event(sop_id, event);

    }



    pub async fn execute_goals(&self, goals: &[&str], project_id: &str) -> Result<CompositeOutput, CoreError> {

        let ctx = ExecutionContext::new();

        let composite =

            sisyphus_intel::synthesize_composite(goals, &*self.intelligence_manager.synthesizer, &ctx, project_id).await

                .map_err(|e| CoreError::Internal(e.to_string()))?;

        self.execute_composite(&composite, None, project_id).await

    }



    pub async fn execute_goal(
        &self,
        goal: &str,
        sop_id: &str,
        project_id: &str,
        event_tx: Option<mpsc::UnboundedSender<ExecutionEvent>>,
        context_json: Option<&str>,
    ) -> Result<(NodeOutput, String), CoreError> {

        let ctx = ExecutionContext::new();

        if let Some(ctx_str) = context_json {

            ctx.notepad.write().unwrap_or_else(|e| e.into_inner()).write_section("context".into(), ctx_str.into());

        }

        

        println!("DEBUG: [execute_goal] Analyzing intent for goal: {}", goal);

        let mode = IntentGate::analyze(goal, &self.intelligence_manager.llm_client).await;

        println!("DEBUG: [execute_goal] Intent analyzed. Mode: {:?}", mode);

        

        println!("DEBUG: [execute_goal] Searching for successful experiences...");

        // BEGIN_EXP_RETRIEVAL

        let mut insights: Option<String> = None;

        // END_EXP_RETRIEVAL

        

        

        let context_hash = format!("{:x}", md5::compute(goal));

        let query = sisyphus_types::experience::ExperienceQuery {

            query_text: goal.to_string(),

            context_hash,

            action_id: None,

            project_id: None,
            visual_embedding: None,

            limit: 5,

        };



        if let Ok(exps) = self.experience_bank.query_similar(query).await {

            let best_exp = exps.into_iter()

                .filter(|e| e.outcome_success)

                .min_by(|a, b| a.deviation_score.partial_cmp(&b.deviation_score).unwrap_or(std::cmp::Ordering::Equal));



            if let Some(exp) = best_exp {

                if let Ok(graph) = self.load_sop(&exp.action_id, project_id).await {

                    insights = Some(format!(

                        "Found highly relevant successful experience (SOP: {}). Nodes: {}. Deviation: {:.4}", 

                        exp.action_id, graph.graph.node_indices().count(), exp.deviation_score

                    ));

                    println!("DEBUG: [execute_goal] Found and injecting most relevant experience: {}", exp.action_id);

                }

            }

        }



        println!("DEBUG: [execute_goal] Synthesizing SOP...");

        let synthesis = self.intelligence_manager.synthesize_goal(
            goal, 
            &ctx, 
            project_id,
            mode,
            insights.as_deref()
        ).await
            .map_err(|e| CoreError::Internal(e.to_string()))?;

        println!("DEBUG: [execute_goal] Synthesis complete. Plan: {}", synthesis.plan_description);

        

        // --- Swarm Audit & Logical Simulation Loop ---
        println!("DEBUG: [execute_goal] Entering refinement loop (Swarm Audit + Logical Simulation)...");
        let mut current_config = sisyphus_types::SOPGraphConfig::from_graph(&synthesis.graph);
        let mut simulation_attempts = 0;
        let max_simulation_attempts = 3;

        loop {
            // 1. Swarm Audit for high-level refinement
            let evolved_config = self.swarm_coordinator.coordinate_evolution(goal, &current_config).await
                .map_err(|e| CoreError::Internal(format!("Swarm audit failed: {}", e)))?;
            
            // 2. Logical Simulation for deep flaw detection
            let sim_graph = sisyphus_types::SOPGraph::from_config(
                &evolved_config,
                self.execution_manager.tool_router.clone(),
                self.test_executor.clone()
            ).map_err(|e| CoreError::Internal(format!("Failed to rebuild graph for simulation: {}", e)))?;

            let sim_result = self.logical_simulator.simulate_logic(
                sim_graph,
                &ctx,
                sop_id,
                Some(goal)
            ).await.map_err(|e| CoreError::Internal(format!("Logical simulation failed: {}", e)))?;

            if sim_result.is_valid {
                println!("DEBUG: [execute_goal] SOP passed logical simulation. Proceeding to execution.");
                current_config = evolved_config;
                break;
            }

            simulation_attempts += 1;
            if simulation_attempts >= max_simulation_attempts {
                println!("DEBUG: [execute_goal] Max simulation attempts reached. Proceeding with best available (may contain warnings).");
                current_config = evolved_config;
                break;
            }

            let critical_flaws: Vec<_> = sim_result.flaws.iter()
                .filter(|f| matches!(f.severity, sisyphus_intel::simulation::FlawSeverity::Critical))
                .collect();

            println!("DEBUG: [execute_//T_goal] Simulation detected {} critical flaws. Triggering re-evolution...", critical_flaws.len());
            
            // Inject flaw descriptions into the next evolution cycle via a temporary goal augmentation
            // In a real system, we'd pass this directly to the coordinator. 
            // For now, we'll wrap the current config and let the swarm try to fix it.
            current_config = evolved_config;
        }
        
        let graph = sisyphus_types::SOPGraph::from_config(
            &current_config,
            self.execution_manager.tool_router.clone(),
            self.test_executor.clone()
        ).map_err(|e| CoreError::Internal(format!("Final graph build failed: {}", e)))?;
        println!("DEBUG: [execute_goal] Refinement loop complete. SOP ready for execution.");
        // --- End Swarm Audit Loop ---

        let plan_desc = synthesis.plan_description;

    

        println!("DEBUG: [execute_goal] Entering cognitive loop...");

        let (res, report_obj, _) = self

            .execute_with_cognition(graph, sop_id, Some(project_id), None, event_tx, None)

            .await

            .map_err(|e| CoreError::Internal(e.to_string()))?;

        println!("DEBUG: [execute_goal] Cognitive loop completed.");

        

        let report_str = format!("Plan: {} | Report: {} iterations, converged={}", plan_desc, report_obj.iterations, report_obj.converged);

        let mermaid_graph = report_obj.to_mermaid();

        

        Ok((res, format!("{}\n\n### Execution Trace\n```mermaid\n{}```", report_str, mermaid_graph)))

    }



    pub async fn execute_goal_streaming(
        &self,
        goal: &str,
        sop_id: &str,
        project_id: &str,
        event_tx: Option<mpsc::UnboundedSender<ExecutionEvent>>,
        stream_tx: Option<mpsc::UnboundedSender<sisyphus_types::CognitiveEvent>>,
    ) -> Result<(NodeOutput, String), CoreError> {

        let ctx = ExecutionContext::new();

        let mode = IntentGate::analyze(goal, &self.intelligence_manager.llm_client).await;

        let synthesis = self.intelligence_manager.synthesize_goal(goal, &ctx, project_id, mode, None).await

            .map_err(|e| CoreError::Internal(e.to_string()))?;

        

        let (res, report_obj, _) = self

            .execute_with_cognition(synthesis.graph, sop_id, Some(project_id), None, event_tx, stream_tx)

            .await

            .map_err(|e| CoreError::Internal(e.to_string()))?;

        

        let report_str = format!("Plan: {} | Report: {} iterations, converged={}", synthesis.plan_description, report_obj.iterations, report_obj.converged);

        let mermaid_graph = report_obj.to_mermaid();

        Ok((res, format!("{}\n\n### Execution Trace\n```mermaid\n{}```", report_str, mermaid_graph)))

    }



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

        self.execute_goal(&augmented_goal, sop_id, project_id, event_tx, None).await

    }



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



        // Record structured experience for Monte Carlo biasing
        let mut experience = sisyphus_types::experience::ExperienceTuple {
            context_text: goal.to_string(),
            context_hash: format!("{:x}", md5::compute(goal)),
            action_id: sop_id.to_string(),
            project_id: None,
            outcome_success: success,
            deviation_score: deviation.unwrap_or(0.0),
            cost: 0, // Actual cost tracking TBD
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            embedding: None,
            visual_embedding: None,
            insight: None,
        };

        // Capture visual experience if successful
        if success {
            if let Some(ref v_manager) = self.strategic_manager.visual_manager {
                // In a real system, we would capture the current screenshot.
                // For now, we simulate the visual state.
                let dummy_image = vec![0u8; 1024];
                if let Ok(vis_emb) = v_manager.get_embedding(&dummy_image).await {
                    experience.visual_embedding = Some(vis_emb);
                }
            }
        }

        let _ = self.experience_bank.record(experience).await;



        if !success {

            let health = self.roadmap_manager.apply_blocking(project_id).await.ok();

            return result.map(|(o, r)| (o, r, health));

        }



        let health = self.roadmap_manager.evaluate_health(project_id).await.ok();

        

        // Automatically trigger strategic reflection to analyze roadmap trajectory and propose pivots.

        self.strategic_reflect(project_id).await;



        result.map(|(o, r)| (o, r, health))

    }



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



                if let Some(proposal) = reflector.propose_pivot(&reflection, project_id) {

                    let prop_id = proposal.proposal_id.clone();

                    if let Err(e) = self.proposal_manager.submit_proposal(&proposal.project_id, proposal.clone()).await {

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



    pub async fn init_mcp_executors(&self, config: &sisyphus_types::config::McpConfig) -> Result<(), String> {

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



    pub async fn approve_and_apply_proposal(

        &self,

        project_id: &str,
        proposal_id: &str,

        reviewer: &str,

    ) -> Result<(), CoreError> {

        let proposal = self.proposal_manager.get_proposal(project_id, proposal_id).await

            .map_err(|e| CoreError::Internal(e.to_string()))?

            .ok_or_else(|| CoreError::Internal(format!("Proposal {} not found", proposal_id)))?;



        self.proposal_manager.approve_proposal(project_id, proposal_id, reviewer).await

            .map_err(|e| CoreError::Internal(e.to_string()))?;



        self.roadmap_manager.apply_proposal_action(&proposal.project_id, &proposal.action)

            .await

            .map_err(|e| CoreError::Internal(e.to_string()))?;



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



    /// Autonomously execute the project roadmap by picking the next available goal.

    pub async fn run_project_roadmap(

        &self,

        project_id: &str,

        event_tx: Option<mpsc::UnboundedSender<ExecutionEvent>>,

    ) -> Result<(), CoreError> {

        tracing::info!(project_id, "Starting autonomous roadmap execution");



        loop {

            // 1. Proactive Health Check: Simulate remaining roadmap to detect potential risks before execution.

            if let Some(_reflector) = self.strategic_reflector.as_ref() {

                if let Some(reflection) = self.strategic_reflect(project_id).await {

                    if reflection.confidence > 0.7 && !reflection.risk_factors.is_empty() {

                        tracing::warn!(

                            project_id, 

                            risks = ?reflection.risk_factors, 

                            "Proactive reflection detected risks. Consider reviewing roadmap."

                        );

                        // We don't break the loop, but the generated proposals will be available for approval.

                    }

                }

            }



            // 2. Find the next pending goal

            let next_goal_ref = self.roadmap_manager.get_next_goal(project_id).await

                .map_err(|e| CoreError::Internal(format!("Failed to get next goal: {}", e)))?;





            let goal_ref = match next_goal_ref {

                Some(g) => g,

                None => {

                    tracing::info!(project_id, "No more pending goals in roadmap. Project complete.");

                    break;

                }

            };



            // 2. Resolve context (Milestone, Phase) for the goal

            let project = self.roadmap_manager.get_project_or_err(project_id).await

                .map_err(|e| CoreError::Internal(format!("Project not found: {}", e)))?;



            let mut milestone_id = None;

            let mut phase_id = None;



            for ms in &project.roadmap.milestones {

                for ph in &ms.phases {

                    if ph.goals.iter().any(|g| g.goal_id == goal_ref.goal_id) {

                        milestone_id = Some(ms.id.clone());

                        phase_id = Some(ph.id.clone());

                        break;

                    }

                }

                if milestone_id.is_some() { break; }

            }



            let (m_id, p_id) = match (milestone_id, phase_id) {

                (Some(m), Some(p)) => (m, p),

                _ => return Err(CoreError::Internal(format!("Goal {} not found in any phase/milestone", goal_ref.goal_id))),

            };



            // 3. Execute the goal

            tracing::info!(project_id, goal_id = %goal_ref.goal_id, "Executing next goal from roadmap");

            

            let result = self.execute_and_record(

                &goal_ref.description, 

                &format!("sop-{}", goal_ref.goal_id), 

                project_id, 

                &m_id, 

                &p_id, 

                &goal_ref.goal_id, 

                event_tx.clone()

            ).await;



            match result {

                Ok(_) => {

                    tracing::info!(project_id, goal_id = %goal_ref.goal_id, "Successfully completed goal");

                }

                Err(CoreError::StrategicPivot(reason)) => {

                    tracing::warn!(project_id, goal_id = %goal_ref.goal_id, reason, "Strategic pivot triggered. Re-evaluating roadmap...");

                }

                Err(e) => {

                    tracing::error!(project_id, goal_id = %goal_ref.goal_id, error = %e, "Goal execution failed");

                    return Err(e);

                }

            }

        }



        Ok(())

    }



    /// Initialize a new project from a high-level vision, automatically generating

    /// a structured roadmap of milestones, phases, and goals.

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



    /// Initialize a new project from a high-level vision, automatically generating

    /// a structured roadmap of milestones, phases, and goals.

    pub async fn initialize_project_from_vision(

        &self,

        project_id: &str,

        vision: &str,

    ) -> Result<sisyphus_types::Project, CoreError> {

        tracing::info!(project_id, vision = %vision, "Initializing project from vision");



        // 1. Create the project metadata

        let project_name = project_id.replace('_', " ").to_uppercase(); // Simple fallback name

        let project = self.roadmap_manager.create_project(

            project_id, 

            &project_name, 

            vision

        ).await.map_err(|e| CoreError::Internal(format!("Failed to create project: {}", e)))?;



        // 2. Generate the roadmap milestones

        if let Some(ref generator) = self.milestone_generator {

            tracing::info!(project_id, "Generating autonomous roadmap milestones...");

            generator.generate_milestones(

                &self.roadmap_manager, 

                project_id, 

                vision

            ).await.map_err(|e| CoreError::Internal(format!("Milestone generation failed: {}", e)))?;

        } else {

            return Err(CoreError::Internal("Milestone generator not configured".into()));

        }



        tracing::info!(project_id, "Project successfully initialized from vision");

        Ok(project)

    }













    pub async fn execute_composite(
        &self,
        composite: &CompositeGraph,
        event_tx: Option<mpsc::UnboundedSender<ExecutionEvent>>,
        project_id: &str,
    ) -> Result<CompositeOutput, CoreError> {

        let order = if composite.execution_order.is_empty() {

            DependencyResolver::resolve(composite)?

        } else {

            composite.execution_order.clone()

        };



        let order_len = order.len();
        let project_id_owned = project_id.to_string();

        let mut results: HashMap<String, (NodeOutput, String)> = HashMap::with_capacity(order_len);

        let accumulated: Arc<DashMap<String, NodeOutput>> =

            Arc::new(DashMap::with_capacity(order_len));

        #[allow(clippy::type_complexity)]

        let mut join_set: JoinSet<(

            String,

            Result<(NodeOutput, crate::strategic_manager::CognitiveReport, Option<Arc<SOPGraph>>), CoreError>,

        )> = JoinSet::new();



        let in_degree: Arc<DashMap<String, usize>> =

            Arc::new(DashMap::with_capacity(order_len));

        for sop_id in &order {

            in_degree.entry(sop_id.clone()).or_insert(0);

        }

        for dep in &composite.dependencies {

            *in_degree.entry(dep.target_sop_id.clone()).or_insert(0) += 1;

        }



        let mut graph_cache: HashMap<String, (Arc<SOPGraph>, NodeIndex)> =

            HashMap::with_capacity(order_len);

        for sop_id in &order {

            let config = composite

                .graphs

                .get(sop_id)

                .ok_or_else(|| CoreError::Internal(format!("Graph {} not found in composite", sop_id)))?;

            let graph = SOPGraph::from_config(config, self.execution_manager.tool_router.clone(), self.test_executor.clone())

                .map_err(|e| CoreError::Internal(e.to_string()))?;

            let start_node = graph

                .graph

                .node_indices()

                .next()

                .ok_or_else(|| CoreError::Internal(format!("Graph {} has no nodes", sop_id)))?;

            graph_cache.insert(sop_id.clone(), (graph, start_node));

        }



        let composite = Arc::new(composite.clone());

        

        let self_arc = Arc::new(SisyphusKernel {

            execution_manager: Arc::clone(&self.execution_manager),

            intelligence_manager: Arc::clone(&self.intelligence_manager),

            strategic_manager: Arc::clone(&self.strategic_manager),
            swarm_coordinator: Arc::clone(&self.swarm_coordinator),
            logical_simulator: Arc::clone(&self.logical_simulator),
            consensus: Arc::clone(&self.consensus),
                    intervention_manager: Arc::clone(&self.intervention_manager),

            channel: Arc::new(AgentChannel::new()),

            benchmark: Arc::clone(&self.benchmark),

            meta_learner: Arc::clone(&self.meta_learner),

            roadmap_manager: Arc::clone(&self.roadmap_manager),

            proposal_manager: Arc::clone(&self.proposal_manager),

            experience_bank: Arc::clone(&self.experience_bank),

            micro_reflector: self.micro_reflector.clone(),

            test_executor: self.test_executor.clone(),

            strategic_reflector: self.strategic_reflector.clone(),

            milestone_generator: self.milestone_generator.clone(),

            resource_optimizer: sisyphus_roadmap::resource_optimizer::ResourceOptimizer::new(),

        });



        for sop_id in &order {

            if in_degree.get(sop_id).map(|d| *d).unwrap_or(0) == 0 {

                if let Some((graph, _start_node)) = graph_cache.remove(sop_id) {

                    let kernel = Arc::clone(&self_arc);

                    let acc = Arc::clone(&accumulated);

                    let composite_ref = Arc::clone(&composite);

                    let tx = event_tx.clone();

                    let sid = sop_id.clone();

                    let pid = project_id_owned.clone();
                    join_set.spawn(async move {

                        let dep_ctx = build_dep_ctx_for(&sid, &composite_ref, &*acc);

                        let result = kernel

                            .execute_with_cognition(graph, &sid, Some(&pid), dep_ctx, tx, None)

                            .await;

                        (sid, result)

                    });

                }

            }

        }



        while let Some(res) = join_set.join_next().await {

            let (completed_id, outcome) = res.map_err(|e| CoreError::Internal(e.to_string()))?;

            let (node_output, report_obj, _evolved) = outcome?;

            let report = format!("Converged: {}, Iterations: {}", report_obj.converged, report_obj.iterations);



            accumulated.insert(completed_id.clone(), node_output.clone());

            results.insert(completed_id.clone(), (node_output, report));



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

                if let Some((graph, _start_node)) = graph_cache.remove(&sid) {

                    let kernel = Arc::clone(&self_arc);

                    let acc = Arc::clone(&accumulated);

                    let composite_ref = Arc::clone(&composite);

                    let tx = event_tx.clone();

                    let pid = project_id_owned.clone();
                    join_set.spawn(async move {

                        let dep_ctx = build_dep_ctx_for(&sid, &composite_ref, &*acc);

                        let result = kernel

                            .execute_with_cognition(graph, &sid, Some(&pid), dep_ctx, tx, None)

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

