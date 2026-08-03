use dashmap::DashMap;

use petgraph::graph::NodeIndex;

use serde::{Deserialize, Serialize};

use std::collections::HashMap;

use std::sync::Arc;

use thiserror::Error;

use sisyphus_intel::SisyphusIntel;

use sisyphus_mem::{MfsStore, SopSqlStore};

use sisyphus_roadmap::RoadmapManager;

use sisyphus_tools_build::benchmark::ContinuousBenchmark;

use sisyphus_tools_llm_tools::meta_learner::MetaLearner;

use sisyphus_tools_system::agent_channel::AgentChannel;

use sisyphus_types::AgentMessage;

use crate::intent_gate::IntentGate;

pub use sisyphus_types::{
    CompositeGraph, DataMapping, ExecutionContext, ExecutionEvent, ExecutorRouter, GraphDependency,
    NodeOutput, SOPGraph, SOPGraphConfig, StateStore, ToolExecutor, WasmSOPNode,
};

pub mod dag;

pub use dag::*;

pub mod executor;

pub use executor::*;

pub mod supervisor;

pub use supervisor::*;

pub mod cognitive_loop;

pub mod cognitive_loop_v2;
pub use cognitive_loop_v2::*;

pub use strategic_manager::{CognitiveIteration, CognitiveReport};

pub mod execution_manager;

pub mod intervention_manager;
pub mod strategic_manager;
pub use intervention_manager::*;
pub mod internalization_manager;
pub use internalization_manager::*;
pub mod self_programming_manager;
pub use self_programming_manager::*;

pub mod swarm_coordinator;
pub use swarm_coordinator::*;

pub mod cognitive_dashboard;
pub mod enhanced_dashboard;

pub use execution_manager::*;

pub mod intelligence_manager;

pub use intelligence_manager::*;

pub mod intent_gate;

pub mod budget;

pub mod chaos_testing;
pub use chaos_testing::*;

pub mod performance;

pub mod dag_renderer;
pub use dag_renderer::*;

#[async_trait::async_trait]

pub trait Consensus: Send + Sync {
    async fn is_leader(&self) -> bool;

    async fn get_leader(&self) -> String;

    async fn propose(
        &self,
        key: &str,
        value: serde_json::Value,
    ) -> Result<(), sisyphus_types::TypesError>;
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

// NOTE: `From<String> for CoreError` was intentionally removed.
// Use `CoreError::Internal(msg)` explicitly to make error conversion visible.

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

/// Builder for `SisyphusKernel` with named setters instead of 13 positional args.
///
/// All fields are required and must be set before calling `build()`.
/// This is a convenience API — the underlying `SisyphusKernel::new()` still
/// requires all 13 dependencies.
///
/// # Example
/// ```ignore
/// let kernel = KernelBuilder::new(meta_learner, consensus)
///     .state_store(my_store)
///     .llm_client(my_llm)
///     .tool_executor(my_exec)
///     // ... other fields ...
///     .build();
/// ```
pub struct KernelBuilder {
    state_store: Option<Arc<MfsStore>>,
    sop_store: Option<Arc<SopSqlStore>>,
    vector_store: Option<Arc<sisyphus_mem::VectorMemoryStore>>,
    experience_bank: Option<Arc<dyn sisyphus_types::experience::ExperienceBank>>,
    tool_executor: Option<Arc<dyn ToolExecutor>>,
    intel: Option<Arc<SisyphusIntel>>,
    llm_client: Option<Arc<dyn sisyphus_tools_llm::LlmClient>>,
    fast_llm_client: Option<Arc<dyn sisyphus_tools_llm::LlmClient>>,
    synthesizer: Option<Arc<dyn sisyphus_intel::FlowSynthesizer>>,
    meta_learner: Arc<parking_lot::Mutex<MetaLearner>>,
    consensus: Arc<dyn Consensus>,
    sql_store: Option<Arc<sisyphus_mem::RoadmapSqlStore>>,
    embedding_client: Option<Arc<dyn sisyphus_tools_llm::EmbeddingClient>>,
}

macro_rules! builder_setter {
    ($field:ident, $ty:ty) => {
        pub fn $field(mut self, v: $ty) -> Self {
            self.$field = Some(v);
            self
        }
    };
}

impl KernelBuilder {
    /// Create a new builder with the two always-required dependencies.
    pub fn new(
        meta_learner: Arc<parking_lot::Mutex<MetaLearner>>,
        consensus: Arc<dyn Consensus>,
    ) -> Self {
        Self {
            state_store: None,
            sop_store: None,
            vector_store: None,
            experience_bank: None,
            tool_executor: None,
            intel: None,
            llm_client: None,
            fast_llm_client: None,
            synthesizer: None,
            meta_learner,
            consensus,
            sql_store: None,
            embedding_client: None,
        }
    }

    builder_setter!(state_store, Arc<MfsStore>);
    builder_setter!(sop_store, Arc<SopSqlStore>);
    builder_setter!(vector_store, Arc<sisyphus_mem::VectorMemoryStore>);
    builder_setter!(
        experience_bank,
        Arc<dyn sisyphus_types::experience::ExperienceBank>
    );
    builder_setter!(tool_executor, Arc<dyn ToolExecutor>);
    builder_setter!(intel, Arc<SisyphusIntel>);
    builder_setter!(llm_client, Arc<dyn sisyphus_tools_llm::LlmClient>);
    builder_setter!(fast_llm_client, Arc<dyn sisyphus_tools_llm::LlmClient>);
    builder_setter!(synthesizer, Arc<dyn sisyphus_intel::FlowSynthesizer>);
    builder_setter!(sql_store, Arc<sisyphus_mem::RoadmapSqlStore>);
    builder_setter!(
        embedding_client,
        Arc<dyn sisyphus_tools_llm::EmbeddingClient>
    );

    /// Consume the builder and produce a `SisyphusKernel`.
    ///
    /// # Panics
    /// Panics if any required field was not set (missing setter call).
    #[allow(clippy::too_many_arguments)]
    pub fn build(self) -> SisyphusKernel {
        let required = |name: &str| -> ! {
            panic!(
                "KernelBuilder: '{}' was not set. Call .{}() before .build().",
                name, name
            )
        };
        SisyphusKernel::new(
            self.state_store.unwrap_or_else(|| required("state_store")),
            self.sop_store.unwrap_or_else(|| required("sop_store")),
            self.vector_store
                .unwrap_or_else(|| required("vector_store")),
            self.experience_bank
                .unwrap_or_else(|| required("experience_bank")),
            self.tool_executor
                .unwrap_or_else(|| required("tool_executor")),
            self.intel.unwrap_or_else(|| required("intel")),
            self.llm_client.unwrap_or_else(|| required("llm_client")),
            self.fast_llm_client
                .unwrap_or_else(|| required("fast_llm_client")),
            self.synthesizer.unwrap_or_else(|| required("synthesizer")),
            self.meta_learner,
            self.consensus,
            self.sql_store.unwrap_or_else(|| required("sql_store")),
            self.embedding_client
                .unwrap_or_else(|| required("embedding_client")),
        )
    }
}

mod kernel;
#[allow(unused_imports)]
pub use kernel::*;

#[cfg(test)]
mod tests;
