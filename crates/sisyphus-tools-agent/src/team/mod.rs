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

mod context;
mod decomposer;
mod orchestrator;
mod scheduler;

pub use context::TeamContext;
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
