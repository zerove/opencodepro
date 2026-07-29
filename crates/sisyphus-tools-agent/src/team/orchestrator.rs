use super::error::TeamError;
use super::{TaskNode, TeamEvent, TeamGoal, TeamPlan, TeamReport};
use crate::agent_registry::AgentRegistry;
use sisyphus_tools_llm::metis::LlmClient;
use sisyphus_types::ToolExecutor;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Orchestrates a team of agents to accomplish a complex goal.
///
/// ## Flow
/// 1. Accept a `TeamGoal` and optional `TeamPlan`
/// 2. Decompose goal into task DAG (if no plan provided)
/// 3. Execute tasks respecting dependency order, using `WorkOrchestrator`
/// 4. Synthesize results into a `TeamReport`
#[derive(Clone)]
pub struct TeamOrchestrator {
    /// Agent registry for lookups and concurrency limits.
    pub(super) registry: Arc<AgentRegistry>,
    pub(super) executor: Arc<dyn ToolExecutor>,
    pub(super) planner_llm: Option<Arc<dyn LlmClient>>,
}

impl TeamOrchestrator {
    /// Create a new team orchestrator.
    ///
    /// `planner_llm` is used for LLM-based task decomposition and result
    /// synthesis. Pass `None` to use manual planning only.
    pub fn new(
        registry: Arc<AgentRegistry>,
        executor: Arc<dyn ToolExecutor>,
        planner_llm: Option<Arc<dyn LlmClient>>,
    ) -> Self {
        Self {
            registry,
            executor,
            planner_llm,
        }
    }

    /// Execute a team plan and produce a report.
    ///
    /// Tasks are validated against the agent registry before execution.
    /// Concurrency is limited per-agent based on `AgentInfo.max_concurrency`.
    /// Tasks with no pending dependencies run in parallel.
    pub async fn execute_plan(&self, plan: TeamPlan) -> Result<TeamReport, TeamError> {
        self.execute_plan_internal(plan, None).await
    }

    /// Decompose a goal and execute the resulting plan in one step.
    ///
    /// This is the main entry point for end-to-end team execution:
    /// 1. LLM decomposes the goal into a task DAG
    /// 2. Plan is validated and executed with agent-aware routing
    /// 3. Results are synthesized into a `TeamReport`
    pub async fn run_goal(&self, goal: TeamGoal) -> Result<TeamReport, TeamError> {
        let plan = self.decompose_goal(&goal).await?;
        self.execute_plan(plan).await
    }

    /// Decompose a goal and execute it, streaming progress events.
    ///
    /// Same as [`run_goal`](Self::run_goal) but sends progress events through the returned
    /// `mpsc::Receiver`.  The receiver yields [`TeamEvent`] variants:
    /// `PlanReady`, `TaskStarted`, `TaskCompleted`, `Synthesizing`,
    /// `Complete`, or `Error`.
    ///
    /// Requires `Arc<Self>` because execution is spawned on a background task.
    /// The caller must receive events from the returned stream until `Complete`
    /// is received.
    pub async fn run_goal_streamed(
        self: Arc<Self>,
        goal: TeamGoal,
    ) -> Result<mpsc::Receiver<TeamEvent>, TeamError> {
        let (tx, rx) = mpsc::channel(256);
        let plan = self.decompose_goal(&goal).await?;
        let _ = tx
            .send(TeamEvent::PlanReady {
                task_count: plan.tasks.len(),
            })
            .await;

        tokio::spawn(async move {
            let report = self.execute_plan_internal(plan, Some(tx.clone())).await;
            match report {
                Ok(report) => {
                    let _ = tx.send(TeamEvent::Complete(report)).await;
                }
                Err(e) => {
                    let _ = tx
                        .send(TeamEvent::Error {
                            message: e.to_string(),
                        })
                        .await;
                }
            }
        });

        Ok(rx)
    }

    /// Create a simple linear plan from a list of tasks.
    ///
    /// Each task runs after all previous tasks complete.
    pub fn create_sequential_plan(goal_id: &str, tasks: Vec<TaskNode>) -> TeamPlan {
        TeamPlan {
            goal_id: goal_id.to_string(),
            tasks,
        }
    }

    /// Run a team goal with a pre-defined set of tasks (ephemeral convenience wrapper).
    pub async fn run(&self, goal: TeamGoal, tasks: Vec<TaskNode>) -> Result<TeamReport, TeamError> {
        let plan = Self::create_sequential_plan(&goal.id, tasks);
        self.execute_plan(plan).await
    }
}

#[cfg(test)]
#[path = "orchestrator_tests.rs"]
mod tests;
