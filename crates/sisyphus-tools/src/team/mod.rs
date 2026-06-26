//! Team orchestration — decompose complex goals into parallel agent task DAGs.
//!
//! ## Architecture
//!
//! - [`TeamOrchestrator`] accepts a [`TeamGoal`] and optionally decomposes it into
//!   a [`TeamPlan`] (a DAG of [`TaskNode`]s) using an LLM.
//! - Tasks are executed respecting dependency order, with per-agent concurrency limits.
//! - Results are synthesized into a [`TeamReport`].
//! - [`TeamError`] covers all failure modes (LLM errors, invalid plans, agent validation).

pub(crate) mod parse;
pub mod error;

mod orchestrator;

pub use error::TeamError;
pub use orchestrator::TeamOrchestrator;

use serde_json::Value;

// ── Types ──────────────────────────────────────────────────

/// A high-level goal for a team of agents.
#[derive(Debug, Clone)]
pub struct TeamGoal {
    /// Unique identifier for this goal.
    pub id: String,
    /// Natural-language description of what to accomplish.
    pub objective: String,
    /// Optional contextual information.
    pub context: Option<String>,
}

/// A single task within a team plan.
#[derive(Debug, Clone)]
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
}

/// A complete team execution plan.
#[derive(Debug, Clone)]
pub struct TeamPlan {
    /// The goal this plan was built from.
    pub goal_id: String,
    /// Ordered set of tasks (topological sort order).
    pub tasks: Vec<TaskNode>,
}

/// Result of a single completed task.
#[derive(Debug, Clone)]
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

/// Progress event emitted during team execution.
#[derive(Debug, Clone)]
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
    /// Final team report ready.
    Complete(TeamReport),
    /// Irrecoverable error.
    Error { message: String },
}

/// Final report from a team execution.
#[derive(Debug, Clone)]
pub struct TeamReport {
    pub goal_id: String,
    pub success: bool,
    pub task_results: Vec<TaskResult>,
    pub synthesis: Option<String>,
    pub total_duration_ms: u128,
}
