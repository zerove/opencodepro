use super::error::TeamError;
use super::{
    Reflection, TaskNode, TeamContext, TeamEvent, TeamGoal, TeamPlan, TeamReport, TeamWorkflow,
};
use crate::agent_registry::AgentRegistry;
use sisyphus_tools_llm::metis::LlmClient;
use sisyphus_types::ToolExecutor;
use std::collections::HashSet;
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
    /// Shared context for inter-task communication (created per execution).
    pub(super) context: Option<Arc<TeamContext>>,
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
            context: None,
        }
    }

    /// Create a new team orchestrator with a pre-initialized context.
    pub fn with_context(
        registry: Arc<AgentRegistry>,
        executor: Arc<dyn ToolExecutor>,
        planner_llm: Option<Arc<dyn LlmClient>>,
        context: Arc<TeamContext>,
    ) -> Self {
        Self {
            registry,
            executor,
            planner_llm,
            context: Some(context),
        }
    }

    /// Execute a team plan and produce a report.
    ///
    /// Tasks are validated against the agent registry before execution.
    /// Concurrency is limited per-agent based on `AgentInfo.max_concurrency`.
    /// Tasks with no pending dependencies run in parallel.
    pub async fn execute_plan(&self, plan: TeamPlan) -> Result<TeamReport, TeamError> {
        let ctx = self.context.clone().unwrap_or_else(|| Arc::new(TeamContext::new()));
        self.execute_plan_internal(plan, None, ctx).await
    }

    /// Execute a team plan with a specific context.
    pub async fn execute_plan_with_context(
        &self,
        plan: TeamPlan,
        context: Arc<TeamContext>,
    ) -> Result<TeamReport, TeamError> {
        self.execute_plan_internal(plan, None, context).await
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
            let report = self
                .execute_plan_internal(plan, Some(tx.clone()), Arc::new(TeamContext::new()))
                .await;
            match report {
                Ok(report) => {
                    let _ = tx.send(TeamEvent::Complete(report)).await;
                },
                Err(e) => {
                    let _ = tx
                        .send(TeamEvent::Error {
                            message: e.to_string(),
                        })
                        .await;
                },
            }
        });

        Ok(rx)
    }

    /// Run a goal with iterative reflection and replanning.
    ///
    /// This implements the full cognitive loop: predict → execute → reflect → evolve.
    /// After each iteration, the LLM reflects on progress and suggests remedial tasks
    /// until the goal is complete or max_iterations is reached.
    pub async fn run_goal_iterative(
        &self,
        goal: TeamGoal,
        max_iterations: usize,
    ) -> Result<TeamReport, TeamError> {
        let context = self.context.clone().unwrap_or_else(|| Arc::new(TeamContext::new()));
        let mut all_results = Vec::new();
        let mut plan = self.decompose_goal(&goal).await?;
        let mut total_iterations = 0;
        let start = std::time::Instant::now();

        for iteration in 0..max_iterations {
            total_iterations = iteration + 1;

            // Execute current plan
            let report = self.execute_plan_internal(plan.clone(), None, context.clone()).await?;
            all_results.extend(report.task_results.clone());

            // Reflect on results
            if let Some(llm) = &self.planner_llm {
                let _ = self.emit_event(&context, TeamEvent::Reflecting { iteration }).await;
                let reflection = self.reflect(llm.as_ref(), &goal, &report, &context).await?;

                if reflection.is_complete {
                    // Goal achieved - synthesize final report
                    let synthesis = self.synthesize(llm.as_ref(), &plan, &all_results).await;
                    return Ok(TeamReport {
                        goal_id: goal.id,
                        success: true,
                        task_results: all_results,
                        synthesis,
                        total_duration_ms: start.elapsed().as_millis(),
                        iterations: total_iterations,
                    });
                }

                // Replan based on reflection
                let _ = self
                    .emit_event(
                        &context,
                        TeamEvent::Replanning {
                            iteration,
                            reason: reflection.summary.clone(),
                        },
                    )
                    .await;
                plan = self.replan(&goal, &all_results, &reflection, context.clone()).await?;
            } else {
                // No LLM for reflection - single pass
                let synthesis = self.synthesize_fallback(&plan, &all_results);
                return Ok(TeamReport {
                    goal_id: goal.id,
                    success: report.success,
                    task_results: all_results,
                    synthesis,
                    total_duration_ms: start.elapsed().as_millis(),
                    iterations: total_iterations,
                });
            }
        }

        // Max iterations reached
        let synthesis = if let Some(llm) = &self.planner_llm {
            self.synthesize(llm.as_ref(), &plan, &all_results).await
        } else {
            self.synthesize_fallback(&plan, &all_results)
        };

        Ok(TeamReport {
            goal_id: goal.id,
            success: false,
            task_results: all_results,
            synthesis,
            total_duration_ms: start.elapsed().as_millis(),
            iterations: total_iterations,
        })
    }

    /// Run a goal with a predefined workflow pattern.
    pub async fn run_goal_workflow(
        &self,
        goal: TeamGoal,
        workflow: TeamWorkflow,
    ) -> Result<TeamReport, TeamError> {
        match workflow {
            TeamWorkflow::SinglePass => self.run_goal(goal).await,
            TeamWorkflow::ReviewIterate { max_rounds } => {
                self.run_review_iterate(goal, max_rounds as usize).await
            },
            TeamWorkflow::ResearchIterate { max_rounds } => {
                self.run_research_iterate(goal, max_rounds as usize).await
            },
            TeamWorkflow::Adaptive { max_iterations } => {
                self.run_goal_iterative(goal, max_iterations as usize).await
            },
        }
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

    /// Emit an event to the context's execution log (for streaming).
    async fn emit_event(&self, context: &Arc<TeamContext>, event: TeamEvent) {
        // For now, we just log; streaming is handled by execute_plan_internal
        // In the future, this could integrate with the event_tx channel
        context.record_task_result(crate::team::TaskResult {
            task_id: format!("event_{}", uuid::Uuid::new_v4()),
            agent_id: "orchestrator".into(),
            success: true,
            output: Some(serde_json::to_value(event).unwrap_or_default()),
            error: None,
            duration_ms: 0,
        });
    }

    /// Reflect on execution results using the planner LLM.
    async fn reflect(
        &self,
        llm: &dyn LlmClient,
        goal: &TeamGoal,
        report: &TeamReport,
        context: &Arc<TeamContext>,
    ) -> Result<Reflection, TeamError> {
        let context_snapshot = context.snapshot();
        let execution_log = context.execution_log();

        let prompt = format!(
            r#"You are a reflection engine for a multi-agent cognitive system.

ORIGINAL GOAL: {}
GOAL CONTEXT: {}

EXECUTION REPORT:
- Iteration: {}
- Tasks completed: {}
- Tasks successful: {}
- Tasks failed: {}
- Total duration: {} ms

SHARED CONTEXT SNAPSHOT:
{}

EXECUTION LOG (last 5 tasks):
{}

Analyze the execution and respond with a JSON object:
{{
  "is_complete": boolean,
  "confidence": float (0.0-1.0),
  "gaps": [string, ...],
  "deviations": [string, ...],
  "suggested_tasks": [
    {{"id": "task_id", "agent_id": "agent", "tool_id": "tool", "args": {{}}, "depends_on": [], "read_keys": [], "write_keys": []}},
    ...
  ],
  "summary": "Natural language summary of reflection"
}}

Rules:
- is_complete = true ONLY if the original goal is fully achieved
- confidence reflects how certain you are about is_complete
- gaps = what's missing to achieve the goal
- deviations = how execution drifted from the goal
- suggested_tasks = concrete remedial tasks (empty if is_complete)
- summary = concise reflection for logging"#,
            goal.objective,
            goal.context.as_deref().unwrap_or("None"),
            report.iterations,
            report.task_results.len(),
            report.task_results.iter().filter(|r| r.success).count(),
            report.task_results.iter().filter(|r| !r.success).count(),
            report.total_duration_ms,
            serde_json::to_string_pretty(&context_snapshot).unwrap_or_default(),
            serde_json::to_string_pretty(&execution_log.iter().rev().take(5).collect::<Vec<_>>())
                .unwrap_or_default(),
        );

        let response =
            llm.generate(&prompt).await.map_err(|e| TeamError::LlmFailed(e.to_string()))?;

        // Parse JSON response
        let reflection: Reflection = serde_json::from_str(&response)
            .map_err(|e| TeamError::Internal(format!("Failed to parse reflection: {}", e)))?;

        Ok(reflection)
    }

    /// Replan based on reflection results.
    async fn replan(
        &self,
        goal: &TeamGoal,
        _all_results: &[crate::team::TaskResult],
        reflection: &Reflection,
        _context: Arc<TeamContext>,
    ) -> Result<TeamPlan, TeamError> {
        if reflection.suggested_tasks.is_empty() {
            return Err(TeamError::Internal(
                "No suggested tasks in reflection".into(),
            ));
        }

        // Build new plan from suggested tasks
        let mut tasks = reflection.suggested_tasks.clone();

        // Ensure task IDs are unique
        for (i, task) in tasks.iter_mut().enumerate() {
            if task.id.is_empty() {
                task.id = format!("replan_{}_{}", reflection.summary.len(), i);
            }
        }

        // Collect IDs to avoid borrow checker conflicts
        let ids: HashSet<String> = tasks.iter().map(|t| t.id.clone()).collect();
        for task in tasks.iter_mut() {
            task.depends_on.retain(|dep| ids.contains(dep));
        }

        Ok(TeamPlan {
            goal_id: goal.id.clone(),
            tasks,
        })
    }

    /// Fallback synthesis without LLM.
    fn synthesize_fallback(
        &self,
        plan: &TeamPlan,
        results: &[crate::team::TaskResult],
    ) -> Option<String> {
        let success = results.iter().filter(|r| r.success).count();
        let failed = results.len() - success;
        Some(format!(
            "Completed {} tasks ({} successful, {} failed) for plan '{}'",
            results.len(),
            success,
            failed,
            plan.goal_id
        ))
    }

    /// Review-Iterate workflow: metis analyzes → momus reviews → metis revises...
    async fn run_review_iterate(
        &self,
        goal: TeamGoal,
        max_rounds: usize,
    ) -> Result<TeamReport, TeamError> {
        let context = self.context.clone().unwrap_or_else(|| Arc::new(TeamContext::new()));
        let mut all_results = Vec::new();
        let start = std::time::Instant::now();

        for round in 0..max_rounds {
            // Phase 1: metis analyzes
            let analysis_plan = TeamPlan {
                goal_id: format!("{}_analysis_{}", goal.id, round),
                tasks: vec![crate::team::TaskNode {
                    id: format!("analysis_{}", round),
                    agent_id: "metis".into(),
                    tool_id: "metis/analyse".to_string(),
                    args: serde_json::json!({
                        "objective": goal.objective,
                        "context": goal.context,
                        "previous_results": context.snapshot(),
                    }),
                    depends_on: vec![],
                    read_keys: vec!["spec".into(), "requirements".into()],
                    write_keys: vec!["analysis".into(), "spec".into()],
                }],
            };

            let analysis_report =
                self.execute_plan_internal(analysis_plan, None, context.clone()).await?;
            all_results.extend(analysis_report.task_results);

            // Phase 2: momus reviews
            let review_plan = TeamPlan {
                goal_id: format!("{}_review_{}", goal.id, round),
                tasks: vec![crate::team::TaskNode {
                    id: format!("review_{}", round),
                    agent_id: "momus".into(),
                    tool_id: "momus/review_code".to_string(),
                    args: serde_json::json!({
                        "target": "analysis",
                        "criteria": ["correctness", "completeness", "clarity"],
                    }),
                    depends_on: vec![format!("analysis_{}", round)],
                    read_keys: vec!["analysis".into()],
                    write_keys: vec!["review".into(), "issues".into()],
                }],
            };

            let review_report =
                self.execute_plan_internal(review_plan.clone(), None, context.clone()).await?;
            all_results.extend(review_report.task_results.clone());

            // Check if review passes (no critical issues)
            let has_critical = review_report.task_results.iter().any(|r| {
                r.output
                    .as_ref()
                    .and_then(|o| o.get("critical_issues").and_then(|v| v.as_array()))
                    .map(|arr| !arr.is_empty())
                    .unwrap_or(false)
            });

            if !has_critical {
                // Review passed, synthesize final
                let synthesis = if let Some(llm) = &self.planner_llm {
                    self.synthesize(llm.as_ref(), &review_plan, &all_results).await
                } else {
                    self.synthesize_fallback(&review_plan, &all_results)
                };
                return Ok(TeamReport {
                    goal_id: goal.id,
                    success: true,
                    task_results: all_results,
                    synthesis,
                    total_duration_ms: start.elapsed().as_millis(),
                    iterations: round + 1,
                });
            }

            // Phase 3: metis revises based on review
            let revise_plan = TeamPlan {
                goal_id: format!("{}_revise_{}", goal.id, round),
                tasks: vec![crate::team::TaskNode {
                    id: format!("revise_{}", round),
                    agent_id: "metis".to_string(),
                    tool_id: "metis/analyse".to_string(),
                    args: serde_json::json!({
                        "action": "revise",
                        "previous_analysis": context.read("analysis"),
                        "review_feedback": context.read("review"),
                    }),
                    depends_on: vec![format!("review_{}", round)],
                    read_keys: vec![
                        "analysis".to_string(),
                        "review".to_string(),
                        "issues".to_string(),
                    ],
                    write_keys: vec!["analysis".to_string(), "spec".to_string()],
                }],
            };

            let revise_report =
                self.execute_plan_internal(revise_plan, None, context.clone()).await?;
            all_results.extend(revise_report.task_results);
        }

        // Max rounds reached
        let synthesis = if let Some(llm) = &self.planner_llm {
            self.synthesize(
                llm.as_ref(),
                &TeamPlan {
                    goal_id: goal.id.clone(),
                    tasks: vec![],
                },
                &all_results,
            )
            .await
        } else {
            self.synthesize_fallback(
                &TeamPlan {
                    goal_id: goal.id.clone(),
                    tasks: vec![],
                },
                &all_results,
            )
        };

        Ok(TeamReport {
            goal_id: goal.id,
            success: false,
            task_results: all_results,
            synthesis,
            total_duration_ms: start.elapsed().as_millis(),
            iterations: max_rounds,
        })
    }

    /// Research-Iterate workflow: librarian researches → metis synthesizes → librarian deepens...
    async fn run_research_iterate(
        &self,
        goal: TeamGoal,
        max_rounds: usize,
    ) -> Result<TeamReport, TeamError> {
        let context = self.context.clone().unwrap_or_else(|| Arc::new(TeamContext::new()));
        let mut all_results = Vec::new();
        let start = std::time::Instant::now();

        for round in 0..max_rounds {
            // Phase 1: librarian researches
            let research_plan = TeamPlan {
                goal_id: format!("{}_research_{}", goal.id, round),
                tasks: vec![crate::team::TaskNode {
                    id: format!("research_{}", round),
                    agent_id: "librarian".into(),
                    tool_id: "librarian/research".to_string(),
                    args: serde_json::json!({
                        "query": goal.objective,
                        "focus": if round == 0 { "overview" } else { "deep_dive" },
                        "previous_findings": context.read("findings"),
                    }),
                    depends_on: vec![],
                    read_keys: vec!["findings".into()],
                    write_keys: vec!["findings".into(), "sources".into()],
                }],
            };

            let research_report =
                self.execute_plan_internal(research_plan, None, context.clone()).await?;
            all_results.extend(research_report.task_results);

            // Phase 2: metis synthesizes
            let synthesize_plan = TeamPlan {
                goal_id: format!("{}_synthesize_{}", goal.id, round),
                tasks: vec![crate::team::TaskNode {
                    id: format!("synthesize_{}", round),
                    agent_id: "metis".into(),
                    tool_id: "metis/analyse".to_string(),
                    args: serde_json::json!({
                        "action": "synthesize",
                        "research_findings": context.read("findings"),
                        "sources": context.read("sources"),
                    }),
                    depends_on: vec![format!("research_{}", round)],
                    read_keys: vec!["findings".into(), "sources".into()],
                    write_keys: vec!["synthesis".into(), "spec".into()],
                }],
            };

            let synth_report =
                self.execute_plan_internal(synthesize_plan.clone(), None, context.clone()).await?;
            all_results.extend(synth_report.task_results);

            // Check if synthesis is sufficient (has concrete spec)
            let has_spec = context.has("spec");
            if has_spec && round >= 1 {
                let synthesis = if let Some(llm) = &self.planner_llm {
                    self.synthesize(llm.as_ref(), &synthesize_plan, &all_results).await
                } else {
                    self.synthesize_fallback(&synthesize_plan, &all_results)
                };
                return Ok(TeamReport {
                    goal_id: goal.id,
                    success: true,
                    task_results: all_results,
                    synthesis,
                    total_duration_ms: start.elapsed().as_millis(),
                    iterations: round + 1,
                });
            }
        }

        let synthesis = if let Some(llm) = &self.planner_llm {
            self.synthesize(
                llm.as_ref(),
                &TeamPlan {
                    goal_id: goal.id.clone(),
                    tasks: vec![],
                },
                &all_results,
            )
            .await
        } else {
            self.synthesize_fallback(
                &TeamPlan {
                    goal_id: goal.id.clone(),
                    tasks: vec![],
                },
                &all_results,
            )
        };

        Ok(TeamReport {
            goal_id: goal.id,
            success: false,
            task_results: all_results,
            synthesis,
            total_duration_ms: start.elapsed().as_millis(),
            iterations: max_rounds,
        })
    }
}

#[cfg(test)]
#[path = "orchestrator_tests.rs"]
mod tests;
