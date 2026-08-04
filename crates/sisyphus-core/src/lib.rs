//! Sisyphus Core — 认知引擎核心模块。
//!
//! 提供 DAG 执行、认知循环 (v1/v2)、SOP 编排与内核实现。

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

impl From<sisyphus_types::TypesError> for CoreError {
    fn from(err: sisyphus_types::TypesError) -> Self {
        CoreError::Internal(err.to_string())
    }
}

// 注意：`From<String> for CoreError` 已被有意移除。
// 请显式使用 `CoreError::Internal(msg)`，让错误转换过程可见。

pub struct SisyphusKernel {
    pub intelligence_manager: Arc<IntelligenceManager>,

    pub execution_manager: Arc<ExecutionManager>,

    pub strategic_manager: Arc<strategic_manager::StrategicManager>,
    pub swarm_coordinator: Arc<SwarmCoordinator>,
    pub logical_simulator: Arc<dyn sisyphus_intel::simulation::LogicalSimulator>,
    pub consensus: Arc<dyn Consensus>,
    pub intervention_manager: Arc<crate::intervention_manager::InterventionManager>,

    /// 代理间消息总线，用于执行过程中的发布/订阅通信。
    pub channel: Arc<AgentChannel>,

    /// 性能基准跟踪器，记录认知迭代的时序数据。
    pub benchmark: Arc<parking_lot::Mutex<ContinuousBenchmark>>,

    /// 元学习器，记录执行结果以优化策略。
    pub meta_learner: Arc<parking_lot::Mutex<MetaLearner>>,

    /// 战略规划管理器，维护 项目 → 里程碑 → 阶段 → 目标 的层级结构。
    pub roadmap_manager: Arc<RoadmapManager>,

    /// 提案管理器，处理人在回路（human-in-the-loop）的战略转向。
    pub proposal_manager: Arc<sisyphus_roadmap::proposal_manager::ProposalManager>,

    pub experience_bank: Arc<dyn sisyphus_types::experience::ExperienceBank>,

    pub micro_reflector: Option<Arc<dyn sisyphus_types::MicroReflector>>,

    pub test_executor: Arc<dyn sisyphus_types::TestExecutor>,

    /// 阶段 3 — 战略反思引擎（LLM 增强的路线图分析）。
    pub strategic_reflector:
        Option<Arc<sisyphus_roadmap::strategic_reflection::StrategicReflector>>,

    /// 阶段 3 — 自主里程碑生成器（LLM 驱动的路线图创建）。
    pub milestone_generator: Option<Arc<sisyphus_roadmap::milestone_generator::MilestoneGenerator>>,

    /// 阶段 3 — 资源/成本预测引擎（仅分析）。
    pub resource_optimizer: sisyphus_roadmap::resource_optimizer::ResourceOptimizer,
}

/// `SisyphusKernel` 的构建器，用命名 setter 替代 13 个位置参数。
///
/// 所有字段都是必填的，调用 `build()` 之前必须完成设置。
/// 这是一个便捷 API——底层的 `SisyphusKernel::new()` 仍然
/// 需要全部 13 个依赖。
///
/// # 示例
/// ```ignore
/// let kernel = KernelBuilder::new(meta_learner, consensus)
///     .state_store(my_store)
///     .llm_client(my_llm)
///     .tool_executor(my_exec)
///     // ... other fields ...
///     .build()?; // 缺少必需字段时返回 CoreError::Internal
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
    /// 用两个始终必需的依赖创建新的构建器。
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

    /// 消费构建器并生成 `SisyphusKernel`。
    ///
    /// # 错误
    /// 当任何必填字段未被设置（缺少 setter 调用）时，
    /// 返回 `CoreError::Internal`，而不是 panic。
    #[allow(clippy::too_many_arguments)]
    pub fn build(self) -> Result<SisyphusKernel, CoreError> {
        let required = |name: &str| -> Result<SisyphusKernel, CoreError> {
            Err(CoreError::Internal(format!(
                "KernelBuilder: '{name}' was not set. Call .{name}() before .build()."
            )))
        };
        let state_store = match self.state_store {
            Some(v) => v,
            None => return required("state_store"),
        };
        let sop_store = match self.sop_store {
            Some(v) => v,
            None => return required("sop_store"),
        };
        let vector_store = match self.vector_store {
            Some(v) => v,
            None => return required("vector_store"),
        };
        let experience_bank = match self.experience_bank {
            Some(v) => v,
            None => return required("experience_bank"),
        };
        let tool_executor = match self.tool_executor {
            Some(v) => v,
            None => return required("tool_executor"),
        };
        let intel = match self.intel {
            Some(v) => v,
            None => return required("intel"),
        };
        let llm_client = match self.llm_client {
            Some(v) => v,
            None => return required("llm_client"),
        };
        let fast_llm_client = match self.fast_llm_client {
            Some(v) => v,
            None => return required("fast_llm_client"),
        };
        let synthesizer = match self.synthesizer {
            Some(v) => v,
            None => return required("synthesizer"),
        };
        let sql_store = match self.sql_store {
            Some(v) => v,
            None => return required("sql_store"),
        };
        let embedding_client = match self.embedding_client {
            Some(v) => v,
            None => return required("embedding_client"),
        };
        Ok(SisyphusKernel::new(
            state_store,
            sop_store,
            vector_store,
            experience_bank,
            tool_executor,
            intel,
            llm_client,
            fast_llm_client,
            synthesizer,
            self.meta_learner,
            self.consensus,
            sql_store,
            embedding_client,
        ))
    }
}

mod kernel;
#[allow(unused_imports)]
pub use kernel::*;

#[cfg(test)]
mod tests;
