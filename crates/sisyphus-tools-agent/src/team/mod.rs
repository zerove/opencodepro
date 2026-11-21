//! Team orchestration — decompose complex goals into parallel agent task DAGs.
//!
//! ## Architecture
//!
//! - [`TeamOrchestrator`] accepts a [`TeamGoal`] and optionally decomposes it into
//!   a [`TeamPlan`] (a DAG of [`TaskNode`]s) using an LLM.
//! - Tasks are executed respecting dependency order, with per-agent concurrency limits.
//! - Results are synthesized into a [`TeamReport`].
//! - [`TeamError`] covers all failure modes (LLM errors, invalid plans, agent validation).

pub mod error;
pub(crate) mod parse;

pub mod checkpoint;
pub mod conflict;
mod context;
mod decomposer;
mod orchestrator;
mod scheduler;

pub use checkpoint::{TeamCheckpoint, TeamContextSnapshot, TeamResumeState};
pub use conflict::{
    detect_conflicts, partition_by_write_conflicts, read_keys_of, write_keys_of, Conflict,
    ConflictKind,
};
pub use context::{
    TeamContext, WorkspaceChange, WorkspaceChangeKind, WorkspaceImpactPolicy, WorkspaceImpactRecord,
};
pub use error::TeamError;
pub use orchestrator::TeamOrchestrator;

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── Types ──────────────────────────────────────────────────

/// A high-level goal for a team of agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamGoal {
    /// Unique identifier for this goal.
    pub id: String,
    /// Natural-language description of what to accomplish.
    pub objective: String,
    /// Optional contextual information.
    pub context: Option<String>,
}

/// A single task within a team plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskNode {
    /// Unique task ID within the plan.
    pub id: String,
    /// Which agent should execute this task.
    pub agent_id: String,
    /// Tool ID to call (e.g. `"librarian/research"`, `"metis/analyse"`).
    pub tool_id: String,
    /// JSON arguments for the tool.
    pub args: Value,
    /// IDs of tasks that must complete before this one can start.
    pub depends_on: Vec<String>,
    /// Keys this task will read from the shared context.
    pub read_keys: Vec<String>,
    /// Keys this task will write to the shared context.
    pub write_keys: Vec<String>,
}

/// A complete team execution plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamPlan {
    /// The goal this plan was built from.
    pub goal_id: String,
    /// Ordered set of tasks (topological sort order).
    pub tasks: Vec<TaskNode>,
}

/// Result of a single completed task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub task_id: String,
    pub agent_id: String,
    pub success: bool,
    pub output: Option<Value>,
    pub error: Option<String>,
    pub duration_ms: u128,
}

impl TaskResult {
    pub(crate) fn failed(task: &TaskNode, error: String) -> Self {
        TaskResult {
            task_id: task.id.clone(),
            agent_id: task.agent_id.clone(),
            success: false,
            output: None,
            error: Some(error),
            duration_ms: 0,
        }
    }

    /// 该结果是否因工作区影响守卫（`FailAffectedPending`）被定向标记为失败。
    ///
    /// 统一识别逻辑，避免调度层/恢复态/CLI 各自匹配错误字符串导致语义漂移。
    pub fn is_workspace_impact_failure(&self) -> bool {
        !self.success && self.error.as_deref().unwrap_or("").contains("workspace impact")
    }
}

/// Delegation policy for a task with retry, timeout, and fallback semantics.
///
/// This is passed per-task via the `args["_delegation"]` convention (same style
/// as `_write_keys`), so existing [`TaskNode`] constructions remain unchanged.
///
/// ## Semantics
/// 1. Execute with the task's primary `agent_id`.
/// 2. On failure, retry up to `retries` additional times (same agent).
/// 3. If still failing, try each `fallback_agents` in order, honoring `retries`
///    for each candidate.
/// 4. If `timeout_ms` is set, a single attempt exceeding it counts as a failure.
///
/// The successful result is annotated with `_delegated_agent` so callers can
/// observe which agent actually produced the output (primary or fallback).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DelegationPolicy {
    /// Additional attempts after the first failure (per candidate agent).
    #[serde(default)]
    pub retries: u32,
    /// Per-attempt timeout in milliseconds. `None` = no timeout.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// Candidate agents tried in order after the primary agent exhausts retries.
    #[serde(default)]
    pub fallback_agents: Vec<String>,
}

impl DelegationPolicy {
    /// Parse a policy from task args using the `_delegation` convention.
    ///
    /// Returns the default (no retries, no timeout, no fallbacks) when the
    /// key is absent or malformed.
    pub fn from_args(args: &Value) -> Self {
        args.get("_delegation")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default()
    }
}

/// Progress event emitted during team execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TeamEvent {
    /// The team started planning for a goal.
    Planning { goal_id: String, objective: String },
    /// A plan was produced.
    PlanReady { task_count: usize },
    /// A task started execution.
    TaskStarted {
        task_id: String,
        agent_id: String,
        tool_id: String,
    },
    /// A task completed (success or failure).
    TaskCompleted(TaskResult),
    /// 检测到并行冲突（T3.4）：同批任务写同一资源，或读写竞争。
    ///
    /// 冲突任务会被拆到不同执行波次（串行化），避免静默覆盖。
    ConflictDetected {
        /// 冲突类型：`write_write` / `read_write`。
        kind: String,
        /// 冲突的资源键（文件路径等）。
        key: String,
        /// 参与冲突的任务 id。
        task_ids: Vec<String>,
    },
    /// M1.2（最小闭环）：批次执行后，已记录工作区变更对剩余任务的影响评估结果。
    ///
    /// 默认仅观测与重评估信号；调用方可选用
    /// [`TeamOrchestrator::execute_plan_with_impact_retry`] 开启对影响类失败任务的自动定向重跑。
    WorkspaceImpact {
        /// 本次评估关联的执行批次序号（从 0 开始）。
        batch_index: usize,
        /// 触发评估的变更资源键（去重）。
        changed_paths: Vec<String>,
        /// 受影响的剩余任务 id（按发现顺序去重）。
        affected_task_ids: Vec<String>,
    },
    /// M1.2（自动定向重跑）：即将对因工作区影响被标记失败的任务执行一轮重试。
    ImpactRetry {
        /// 重试轮次（从 1 开始）。
        round: usize,
        /// 本轮将重新调度的任务 id。
        task_ids: Vec<String>,
    },
    /// All tasks complete, synthesis starting.
    Synthesizing { completed: usize, failed: usize },
    /// Reflection phase started.
    Reflecting { iteration: usize },
    /// Reflection complete, replanning if needed.
    Replanning { iteration: usize, reason: String },
    /// Final team report ready.
    Complete(TeamReport),
    /// Irrecoverable error.
    Error { message: String },
}

/// Reflection result from evaluating a team execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reflection {
    /// Whether the goal is considered complete.
    pub is_complete: bool,
    /// Confidence level (0.0 to 1.0).
    pub confidence: f32,
    /// Identified gaps or missing pieces.
    pub gaps: Vec<String>,
    /// Deviations from the original objective.
    pub deviations: Vec<String>,
    /// Suggested remedial tasks.
    pub suggested_tasks: Vec<TaskNode>,
    /// Natural language summary of the reflection.
    pub summary: String,
}

/// 并行冲突与串行化统计（M1.3 可观测性）。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConflictStats {
    /// 检测到的冲突总数（write-write + read-write）。
    #[serde(default)]
    pub detected: usize,
    /// 写写冲突次数。
    #[serde(default)]
    pub write_write: usize,
    /// 读写冲突次数。
    #[serde(default)]
    pub read_write: usize,
    /// 因写冲突拆分出的总执行波次数（仅统计发生拆分的批次，`> ready_count` 时意味着串行化）。
    #[serde(default)]
    pub serialized_waves: usize,
    /// 发生冲突并被串行化处理的批次数。
    #[serde(default)]
    pub conflict_batches: usize,
}

impl ConflictStats {
    /// 合并另一份统计（用于多轮/多阶段执行聚合）。
    pub fn merge(&mut self, other: &ConflictStats) {
        self.detected += other.detected;
        self.write_write += other.write_write;
        self.read_write += other.read_write;
        self.serialized_waves += other.serialized_waves;
        self.conflict_batches += other.conflict_batches;
    }
}

/// Final report from a team execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamReport {
    pub goal_id: String,
    pub success: bool,
    pub task_results: Vec<TaskResult>,
    pub synthesis: Option<String>,
    pub total_duration_ms: u128,
    /// Number of iterations executed (for iterative workflows).
    pub iterations: usize,
    /// 并行冲突与串行化统计（可观测性；旧数据缺失时为默认值）。
    #[serde(default)]
    pub conflict_stats: ConflictStats,
    /// 工作区变更影响评估的可审计记录（M1.2；旧数据缺失时为空）。
    #[serde(default)]
    pub workspace_impact_records: Vec<WorkspaceImpactRecord>,
    /// 已消费的工作区变更数量（用于 S5 检查点/恢复续跑；旧数据缺失时为 0）。
    #[serde(default)]
    pub processed_change_count: usize,
}

/// Predefined team workflow patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamWorkflow {
    /// Single pass: plan once, execute once.
    SinglePass,
    /// Review-iterate: metis analyzes, momus reviews, metis revises... up to max_rounds.
    ReviewIterate { max_rounds: u32 },
    /// Research-iterate: librarian researches, metis synthesizes, librarian deepens... up to max_rounds.
    ResearchIterate { max_rounds: u32 },
    /// Fully adaptive: reflect after each iteration, replan based on gaps.
    Adaptive { max_iterations: u32 },
}
