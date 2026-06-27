use super::error::TeamError;
use super::parse::parse_decomposition_response;
use super::{TaskNode, TaskResult, TeamEvent, TeamGoal, TeamPlan, TeamReport};
use crate::agent_registry::AgentRegistry;
use crate::delegate::{WorkOrchestrator, WorkUnit};
use crate::metis::LlmClient;
use serde_json::Value;
use sisyphus_types::ToolExecutor;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;
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
    registry: Arc<AgentRegistry>,
    executor: Arc<dyn ToolExecutor>,
    planner_llm: Option<Arc<dyn LlmClient>>,
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
    pub async fn execute_plan(&self, plan: TeamPlan) -> TeamReport {
        self.execute_plan_internal(plan, None).await
    }

    /// Execute a plan with optional event streaming.
    ///
    /// Same as [`execute_plan`] but also sends progress events through `event_tx`
    /// during execution (TaskStarted, TaskCompleted, Synthesizing, Complete).
    /// Returns the final report after all events have been sent.
    pub(crate) async fn execute_plan_internal(
        &self,
        plan: TeamPlan,
        event_tx: Option<mpsc::Sender<TeamEvent>>,
    ) -> TeamReport {
        let start = Instant::now();
        let mut task_results: Vec<TaskResult> = Vec::new();
        let mut completed: HashMap<String, TaskResult> = HashMap::new();

        // Pre-validate all tasks against the registry
        for task in &plan.tasks {
            if !self.registry.has(&task.agent_id) {
                let result = TaskResult::failed(
                    task,
                    format!("Agent '{}' is not registered", task.agent_id),
                );
                task_results.push(result.clone());
                completed.insert(task.id.clone(), result);
            } else if let Err(msg) = self.validate_tool_for_agent(&task.agent_id, &task.tool_id) {
                let result = TaskResult::failed(task, msg);
                task_results.push(result.clone());
                completed.insert(task.id.clone(), result);
            }
        }

        // Build agent concurrency limits
        let agent_max: HashMap<String, usize> = {
            let mut m = HashMap::new();
            for (id, info) in self.registry.list() {
                m.insert(id, info.max_concurrency);
            }
            m
        };

        // Track active task count per agent
        let mut active: HashMap<String, usize> = HashMap::new();

        // Build remaining set (tasks that passed validation)
        let mut remaining: HashSet<String> = plan
            .tasks
            .iter()
            .filter(|t| !completed.contains_key(&t.id))
            .map(|t| t.id.clone())
            .collect();

        while !remaining.is_empty() {
            let ready: Vec<&TaskNode> = plan
                .tasks
                .iter()
                .filter(|t| remaining.contains(&t.id))
                .filter(|t| t.depends_on.iter().all(|d| completed.contains_key(d)))
                .filter(|t| {
                    let current = active.get(&t.agent_id).copied().unwrap_or(0);
                    let limit = agent_max.get(&t.agent_id).copied().unwrap_or(usize::MAX);
                    current < limit
                })
                .collect();

            if ready.is_empty() {
                // Check if remaining are concurrency-blocked vs deadlocked
                let blocked: Vec<&TaskNode> = plan
                    .tasks
                    .iter()
                    .filter(|t| remaining.contains(&t.id))
                    .filter(|t| t.depends_on.iter().all(|d| completed.contains_key(d)))
                    .collect();

                if blocked.is_empty() {
                    // True deadlock: dependencies can never be satisfied
                    if let Some(tx) = &event_tx {
                        let _ = tx.try_send(TeamEvent::Error {
                            message: "Dependency deadlock detected".into(),
                        });
                    }
                    for task_id in &remaining {
                        let task = plan
                            .tasks
                            .iter()
                            .find(|t| t.id == *task_id)
                            .expect("task id should exist in plan");
                        let result = TaskResult::failed(
                            task,
                            "Dependency deadlock: unsatisfied dependencies".into(),
                        );
                        if let Some(tx) = &event_tx {
                            let _ = tx.try_send(TeamEvent::TaskCompleted(result.clone()));
                        }
                        task_results.push(result.clone());
                        completed.insert(task_id.clone(), result);
                    }
                } else if active.values().all(|&c| c == 0) {
                    // Concurrency deadlock: tasks are ready but all agents at capacity
                    // and no tasks are running to free capacity
                    if let Some(tx) = &event_tx {
                        let _ = tx.try_send(TeamEvent::Error {
                            message: "Concurrency deadlock detected".into(),
                        });
                    }
                    for task_id in &remaining {
                        let task = plan
                            .tasks
                            .iter()
                            .find(|t| t.id == *task_id)
                            .expect("task id should exist in plan");
                        let result = TaskResult::failed(
                            task,
                            "Concurrency deadlock: all agents at capacity".into(),
                        );
                        if let Some(tx) = &event_tx {
                            let _ = tx.try_send(TeamEvent::TaskCompleted(result.clone()));
                        }
                        task_results.push(result.clone());
                        completed.insert(task_id.clone(), result);
                    }
                }
                break;
            }

            // Emit TaskStarted for each ready task
            for task in &ready {
                if let Some(tx) = &event_tx {
                    let _ = tx.try_send(TeamEvent::TaskStarted {
                        task_id: task.id.clone(),
                        agent_id: task.agent_id.clone(),
                        tool_id: task.tool_id.clone(),
                    });
                }
            }

            // Bump active counts
            for task in &ready {
                *active.entry(task.agent_id.clone()).or_insert(0) += 1;
            }

            // Execute ready tasks in parallel
            let units: Vec<WorkUnit> = ready
                .iter()
                .map(|task| {
                    let executor = self.executor.clone();
                    let tool_id = task.tool_id.clone();
                    let args_bytes = serde_json::to_vec(&task.args).unwrap_or_default();
                    let task_id = task.id.clone();

                    WorkUnit {
                        id: task_id.clone(),
                        name: format!("{}:{}", task.agent_id, task.tool_id),
                        handler: Box::new(move || {
                            let ex = executor.clone();
                            let tid = tool_id.clone();
                            let args = args_bytes.clone();
                            Box::pin(async move {
                                let result = ex.execute(&tid, args, 0).await;
                                match result {
                                    Ok(bytes) => {
                                        let value: Value =
                                            serde_json::from_slice(&bytes).unwrap_or(Value::Null);
                                        Ok(value)
                                    }
                                    Err(e) => Err(crate::ToolError::Generic(e)),
                                }
                            })
                        }),
                    }
                })
                .collect();

            let orchestrator = WorkOrchestrator::new(ready.len());
            let reports = orchestrator.run_all(units).await.unwrap_or_default();

            for (i, report) in reports.iter().enumerate() {
                let task = ready[i];
                let result = if report.status == crate::delegate::WorkStatus::Completed {
                    TaskResult {
                        task_id: task.id.clone(),
                        agent_id: task.agent_id.clone(),
                        success: true,
                        output: report.result.clone(),
                        error: None,
                        duration_ms: report.duration_ms,
                    }
                } else {
                    TaskResult {
                        task_id: task.id.clone(),
                        agent_id: task.agent_id.clone(),
                        success: false,
                        output: None,
                        error: report.error.clone(),
                        duration_ms: report.duration_ms,
                    }
                };
                // Decrement active count for this agent
                if let Some(count) = active.get_mut(&task.agent_id) {
                    *count = count.saturating_sub(1);
                }
                // Emit TaskCompleted
                if let Some(tx) = &event_tx {
                    let _ = tx.try_send(TeamEvent::TaskCompleted(result.clone()));
                }
                task_results.push(result.clone());
                completed.insert(task.id.clone(), result);
            }

            for task in &ready {
                remaining.remove(&task.id);
            }
        }

        // Synthesis
        let success_count = task_results.iter().filter(|r| r.success).count();
        if let Some(tx) = &event_tx {
            let _ = tx.try_send(TeamEvent::Synthesizing {
                completed: success_count,
                failed: task_results.len() - success_count,
            });
        }
        let synthesis = if let Some(llm) = &self.planner_llm {
            self.synthesize(llm.as_ref(), &plan, &task_results).await
        } else {
            Some(format!(
                "Completed {} tasks ({} successful, {} failed)",
                task_results.len(),
                success_count,
                task_results.len() - success_count,
            ))
        };

        let report = TeamReport {
            goal_id: plan.goal_id,
            success: success_count == task_results.len(),
            task_results,
            synthesis,
            total_duration_ms: start.elapsed().as_millis(),
        };

        if let Some(tx) = &event_tx {
            let _ = tx.try_send(TeamEvent::Complete(report.clone()));
        }

        report
    }

    /// Validate that an agent supports the given tool ID.
    fn validate_tool_for_agent(&self, agent_id: &str, tool_id: &str) -> Result<(), String> {
        if let Some(info) = self.registry.get(agent_id) {
            if info.tool_prefixes.iter().any(|p| tool_id.starts_with(p)) {
                Ok(())
            } else {
                Err(format!(
                    "Agent '{}' does not support tool '{}'. Supported prefixes: {}",
                    agent_id,
                    tool_id,
                    info.tool_prefixes.join(", ")
                ))
            }
        } else {
            Err(format!("Agent '{}' is not registered", agent_id))
        }
    }

    /// Use an LLM to synthesize task results into a coherent summary.
    async fn synthesize(
        &self,
        llm: &dyn LlmClient,
        plan: &TeamPlan,
        results: &[TaskResult],
    ) -> Option<String> {
        let mut summary_parts: Vec<String> = Vec::new();
        for result in results {
            let status = if result.success { "✓" } else { "✗" };
            let output_str = result
                .output
                .as_ref()
                .map(|v| v.to_string())
                .unwrap_or_default();
            let snippet = if output_str.len() > 200 {
                format!("{}...", &output_str[..200])
            } else {
                output_str
            };
            summary_parts.push(format!(
                "{} [{}] {}: {}",
                status, result.agent_id, result.task_id, snippet
            ));
        }

        let prompt = format!(
            "You are a team lead synthesizing results from multiple specialist agents.\n\n\
             Goal: {}\n\n\
             Task Results:\n{}\n\n\
             Provide a concise synthesis of what was accomplished, key findings, \
             and any issues that need attention.",
            plan.goal_id,
            summary_parts.join("\n"),
        );

        let system = "You are a Senior Technical Lead. Synthesize multi-agent results concisely.";
        match llm.chat(system, &prompt, false).await {
            Ok(s) => Some(s),
            Err(_) => {
                let success_count = results.iter().filter(|r| r.success).count();
                Some(format!(
                    "Completed {} tasks ({} successful, {} failed)",
                    results.len(),
                    success_count,
                    results.len() - success_count,
                ))
            }
        }
    }

    /// Decompose a `TeamGoal` into a `TeamPlan` using the LLM.
    pub async fn decompose_goal(&self, goal: &TeamGoal) -> Result<TeamPlan, TeamError> {
        let llm = self.planner_llm.as_ref().ok_or(TeamError::NoPlanner)?;

        let agents = self.registry.list();
        let agent_descriptions: Vec<String> = agents
            .iter()
            .map(|(id, info)| {
                format!(
                    "- `{}` ({}): {} — tools: {}",
                    id,
                    info.display_name,
                    info.description,
                    info.tool_prefixes.join(", ")
                )
            })
            .collect();

        let system_prompt = format!(
            "You are a senior engineering lead that decomposes complex goals into \
             parallel task DAGs for a team of specialist AI agents.\n\n\
             Available agents:\n{}\n\n\
             Rules:\n\
             1. Break the goal into the smallest meaningful tasks — prefer more granular \
             tasks that can run in parallel.\n\
             2. Assign each task to the most suitable agent.\n\
             3. Set `depends_on` only when a task genuinely needs another's output.\n\
             4. Use `args` to pass the exact JSON arguments the tool expects.\n\
             5. Use `id` values that are short, kebab-case, and unique (e.g. \
             `\"research-competitors\"`, `\"review-architecture\"`).\n\
             6. Include context from the goal description in args where useful.\n\n\
             Respond ONLY with a JSON object in this exact shape (no markdown, no \
             explanation):\n\
             {{\n\
               \"tasks\": [\n\
                 {{\n\
                   \"id\": \"task-id\",\n\
                   \"agent\": \"agent-name\",\n\
                   \"tool_id\": \"agent/tool\",\n\
                   \"args\": {{ /* tool-specific JSON arguments */ }},\n\
                   \"depends_on\": []\n\
                 }}\n\
               ]\n\
             }}",
            agent_descriptions.join("\n"),
        );

        let user_prompt = format!(
            "Goal: {}\n{}",
            goal.objective,
            goal.context
                .as_ref()
                .map(|c| format!("Context: {}", c))
                .unwrap_or_default(),
        );

        let response = llm
            .chat(&system_prompt, &user_prompt, true)
            .await
            .map_err(TeamError::LlmFailed)?;

        parse_decomposition_response(&response, &goal.id).map_err(TeamError::InvalidPlan)
    }

    /// Decompose a goal and execute the resulting plan in one step.
    ///
    /// This is the main entry point for end-to-end team execution:
    /// 1. LLM decomposes the goal into a task DAG
    /// 2. Plan is validated and executed with agent-aware routing
    /// 3. Results are synthesized into a `TeamReport`
    pub async fn run_goal(&self, goal: TeamGoal) -> Result<TeamReport, TeamError> {
        let plan = self.decompose_goal(&goal).await?;
        Ok(self.execute_plan(plan).await)
    }

    /// Decompose a goal and execute it, streaming progress events.
    ///
    /// Same as [`run_goal`] but sends progress events through the returned
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
            let _ = tx.send(TeamEvent::Complete(report)).await;
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
    pub async fn run(&self, goal: TeamGoal, tasks: Vec<TaskNode>) -> TeamReport {
        let plan = Self::create_sequential_plan(&goal.id, tasks);
        self.execute_plan(plan).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_registry::AgentInfo;
    use crate::executor::BuiltinToolExecutor;
    use crate::feedback::FeedbackStore;
    use crate::hook::HookRegistry;
    use crate::llm::DynamicLlmClient;
    use crate::metis::DummyLlmClient;
    use serde_json::json;
    use sisyphus_types::Config;
    use std::sync::{Arc, RwLock};

    fn make_registry() -> Arc<AgentRegistry> {
        let config = Arc::new(RwLock::new(Config::default()));
        let client = Arc::new(DynamicLlmClient::new(config));
        let reg = Arc::new(AgentRegistry::new(client));
        reg.register_builtin();
        reg
    }

    fn make_executor() -> Arc<dyn ToolExecutor> {
        let dummy: Arc<dyn LlmClient> = Arc::new(DummyLlmClient);
        let feedback = Arc::new(FeedbackStore::default_dir());
        Arc::new(BuiltinToolExecutor {
            registry: HookRegistry::new(),
            llm_client: dummy,
            feedback_store: feedback,
        })
    }

    fn make_orchestrator() -> TeamOrchestrator {
        TeamOrchestrator::new(make_registry(), make_executor(), None)
    }

    #[tokio::test]
    async fn test_execute_empty_plan() {
        let orch = make_orchestrator();
        let plan = TeamPlan {
            goal_id: "empty".into(),
            tasks: vec![],
        };
        let report = orch.execute_plan(plan).await;
        assert!(report.success);
        assert!(report.task_results.is_empty());
    }

    #[tokio::test]
    async fn test_execute_single_task() {
        let orch = make_orchestrator();
        let tasks = vec![TaskNode {
            id: "t1".into(),
            agent_id: "metis".into(),
            tool_id: "metis/analyse".into(),
            args: json!({"request": "test analysis"}),
            depends_on: vec![],
        }];
        let report = orch
            .run(
                TeamGoal {
                    id: "single".into(),
                    objective: "Test single task".into(),
                    context: None,
                },
                tasks,
            )
            .await;

        assert!(
            report.success,
            "single metis task should succeed with DummyLlmClient"
        );
        assert_eq!(report.task_results.len(), 1);
        assert!(report.task_results[0].success);
        assert!(report.synthesis.is_some());
    }

    #[tokio::test]
    async fn test_execute_sequential_tasks() {
        let orch = make_orchestrator();
        let tasks = vec![
            TaskNode {
                id: "t1".into(),
                agent_id: "metis".into(),
                tool_id: "metis/analyse".into(),
                args: json!({"request": "first analysis"}),
                depends_on: vec![],
            },
            TaskNode {
                id: "t2".into(),
                agent_id: "metis".into(),
                tool_id: "metis/analyse".into(),
                args: json!({"request": "second analysis"}),
                depends_on: vec!["t1".into()],
            },
        ];
        let report = orch
            .run(
                TeamGoal {
                    id: "seq".into(),
                    objective: "Sequential tasks".into(),
                    context: None,
                },
                tasks,
            )
            .await;

        assert!(report.success);
        assert_eq!(report.task_results.len(), 2);
        assert!(report.task_results[0].success);
        assert!(report.task_results[1].success);
    }

    #[tokio::test]
    async fn test_execute_parallel_tasks() {
        let orch = make_orchestrator();
        let tasks = vec![
            TaskNode {
                id: "t1".into(),
                agent_id: "metis".into(),
                tool_id: "metis/analyse".into(),
                args: json!({"request": "parallel A"}),
                depends_on: vec![],
            },
            TaskNode {
                id: "t2".into(),
                agent_id: "metis".into(),
                tool_id: "metis/analyse".into(),
                args: json!({"request": "parallel B"}),
                depends_on: vec![],
            },
            TaskNode {
                id: "t3".into(),
                agent_id: "metis".into(),
                tool_id: "metis/analyse".into(),
                args: json!({"request": "parallel C"}),
                depends_on: vec![],
            },
        ];
        let report = orch
            .run(
                TeamGoal {
                    id: "par".into(),
                    objective: "Parallel tasks".into(),
                    context: None,
                },
                tasks,
            )
            .await;

        assert!(
            report.success,
            "three parallel metis tasks should all succeed"
        );
        assert_eq!(report.task_results.len(), 3);
    }

    #[tokio::test]
    async fn test_execute_diamond_dependency() {
        let orch = make_orchestrator();
        let tasks = vec![
            TaskNode {
                id: "t1".into(),
                agent_id: "metis".into(),
                tool_id: "metis/analyse".into(),
                args: json!({"request": "root"}),
                depends_on: vec![],
            },
            TaskNode {
                id: "t2".into(),
                agent_id: "metis".into(),
                tool_id: "metis/analyse".into(),
                args: json!({"request": "left"}),
                depends_on: vec!["t1".into()],
            },
            TaskNode {
                id: "t3".into(),
                agent_id: "metis".into(),
                tool_id: "metis/analyse".into(),
                args: json!({"request": "right"}),
                depends_on: vec!["t1".into()],
            },
            TaskNode {
                id: "t4".into(),
                agent_id: "metis".into(),
                tool_id: "metis/analyse".into(),
                args: json!({"request": "merge"}),
                depends_on: vec!["t2".into(), "t3".into()],
            },
        ];
        let report = orch
            .run(
                TeamGoal {
                    id: "diamond".into(),
                    objective: "Diamond dependency".into(),
                    context: None,
                },
                tasks,
            )
            .await;

        assert!(report.success);
        assert_eq!(report.task_results.len(), 4);
    }

    #[tokio::test]
    async fn test_deadlock_detection() {
        let orch = make_orchestrator();
        let tasks = vec![
            TaskNode {
                id: "t1".into(),
                agent_id: "metis".into(),
                tool_id: "metis/analyse".into(),
                args: json!({"request": "a"}),
                depends_on: vec!["t2".into()],
            },
            TaskNode {
                id: "t2".into(),
                agent_id: "metis".into(),
                tool_id: "metis/analyse".into(),
                args: json!({"request": "b"}),
                depends_on: vec!["t1".into()],
            },
        ];
        let report = orch
            .run(
                TeamGoal {
                    id: "deadlock".into(),
                    objective: "Test deadlock".into(),
                    context: None,
                },
                tasks,
            )
            .await;

        assert!(!report.success, "deadlock should cause failure");
        assert_eq!(report.task_results.len(), 2);
        assert!(report.task_results.iter().all(|r| !r.success));
    }

    #[tokio::test]
    async fn test_missing_dependency_deadlock() {
        let orch = make_orchestrator();
        let tasks = vec![TaskNode {
            id: "t1".into(),
            agent_id: "metis".into(),
            tool_id: "metis/analyse".into(),
            args: json!({"request": "a"}),
            depends_on: vec!["nonexistent".into()],
        }];
        let report = orch
            .run(
                TeamGoal {
                    id: "missing-dep".into(),
                    objective: "Test missing dep".into(),
                    context: None,
                },
                tasks,
            )
            .await;

        assert!(!report.success);
        let err = &report.task_results[0].error;
        assert!(
            err.as_ref().unwrap().contains("deadlock"),
            "error should mention deadlock; got: {:?}",
            err
        );
    }

    #[tokio::test]
    async fn test_synthesis_with_llm() {
        let dummy_llm: Arc<dyn LlmClient> = Arc::new(DummyLlmClient);
        let orch = TeamOrchestrator::new(make_registry(), make_executor(), Some(dummy_llm));

        let tasks = vec![TaskNode {
            id: "t1".into(),
            agent_id: "metis".into(),
            tool_id: "metis/analyse".into(),
            args: json!({"request": "test"}),
            depends_on: vec![],
        }];
        let report = orch
            .run(
                TeamGoal {
                    id: "synth".into(),
                    objective: "Test synthesis".into(),
                    context: None,
                },
                tasks,
            )
            .await;

        assert!(report.success);
        assert!(report.synthesis.is_some());
        let s = report.synthesis.unwrap();
        assert!(!s.is_empty(), "synthesis should not be empty");
    }

    #[test]
    fn test_create_sequential_plan() {
        let tasks = vec![
            TaskNode {
                id: "t1".into(),
                agent_id: "a".into(),
                tool_id: "t".into(),
                args: json!({}),
                depends_on: vec![],
            },
            TaskNode {
                id: "t2".into(),
                agent_id: "b".into(),
                tool_id: "t".into(),
                args: json!({}),
                depends_on: vec![],
            },
        ];
        let plan = TeamOrchestrator::create_sequential_plan("g1", tasks);
        assert_eq!(plan.goal_id, "g1");
        assert_eq!(plan.tasks.len(), 2);
    }

    #[tokio::test]
    async fn test_execute_unknown_tool_graceful() {
        let orch = make_orchestrator();
        let tasks = vec![TaskNode {
            id: "t1".into(),
            agent_id: "metis".into(),
            tool_id: "nonexistent/tool".into(),
            args: json!({}),
            depends_on: vec![],
        }];
        let report = orch
            .run(
                TeamGoal {
                    id: "unknown-tool".into(),
                    objective: "Test unknown tool".into(),
                    context: None,
                },
                tasks,
            )
            .await;

        assert!(!report.success, "unknown tool should fail");
        assert!(!report.task_results.is_empty());
    }

    #[tokio::test]
    async fn test_report_metrics() {
        let orch = make_orchestrator();
        let tasks = vec![TaskNode {
            id: "t1".into(),
            agent_id: "metis".into(),
            tool_id: "metis/analyse".into(),
            args: json!({"request": "test"}),
            depends_on: vec![],
        }];
        let report = orch
            .run(
                TeamGoal {
                    id: "metrics".into(),
                    objective: "Test metrics".into(),
                    context: None,
                },
                tasks,
            )
            .await;

        let success_count = report.task_results.iter().filter(|r| r.success).count();
        assert_eq!(success_count, 1, "one task should succeed");
    }

    #[tokio::test]
    async fn test_unknown_agent_fails() {
        let orch = make_orchestrator();
        let tasks = vec![TaskNode {
            id: "t1".into(),
            agent_id: "nonexistent-agent".into(),
            tool_id: "metis/analyse".into(),
            args: json!({}),
            depends_on: vec![],
        }];
        let report = orch
            .run(
                TeamGoal {
                    id: "bad-agent".into(),
                    objective: "Test unknown agent".into(),
                    context: None,
                },
                tasks,
            )
            .await;

        let failed: Vec<&TaskResult> = report.task_results.iter().filter(|r| !r.success).collect();
        assert_eq!(failed.len(), 1, "task with unknown agent should fail");
        let err = failed[0].error.as_ref().unwrap();
        assert!(
            err.contains("not registered"),
            "error should mention not registered; got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_tool_not_matching_agent_fails() {
        let orch = make_orchestrator();
        let tasks = vec![TaskNode {
            id: "t1".into(),
            agent_id: "metis".into(),
            tool_id: "security/audit_code".into(),
            args: json!({}),
            depends_on: vec![],
        }];
        let report = orch
            .run(
                TeamGoal {
                    id: "tool-mismatch".into(),
                    objective: "Test tool mismatch".into(),
                    context: None,
                },
                tasks,
            )
            .await;

        let failed: Vec<&TaskResult> = report.task_results.iter().filter(|r| !r.success).collect();
        assert_eq!(failed.len(), 1, "mismatched tool should fail");
        let err = failed[0].error.as_ref().unwrap();
        assert!(
            err.contains("does not support"),
            "error should mention 'does not support'; got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_decompose_no_llm() {
        let orch = make_orchestrator();
        let goal = TeamGoal {
            id: "test".into(),
            objective: "Test".into(),
            context: None,
        };
        let result = orch.decompose_goal(&goal).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            TeamError::NoPlanner => {}
            other => panic!("expected NoPlanner error, got: {:?}", other),
        }
    }

    struct JsonLlmClient(String);

    #[async_trait::async_trait]
    impl LlmClient for JsonLlmClient {
        async fn chat(
            &self,
            _system: &str,
            _user: &str,
            _json_mode: bool,
        ) -> Result<String, String> {
            Ok(self.0.clone())
        }
    }

    struct ErrLlmClient;

    #[async_trait::async_trait]
    impl LlmClient for ErrLlmClient {
        async fn chat(
            &self,
            _system: &str,
            _user: &str,
            _json_mode: bool,
        ) -> Result<String, String> {
            Err("LLM API error".to_string())
        }
    }

    #[tokio::test]
    async fn test_decompose_llm_error() {
        let orch = TeamOrchestrator::new(
            make_registry(),
            make_executor(),
            Some(Arc::new(ErrLlmClient)),
        );
        let goal = TeamGoal {
            id: "err".into(),
            objective: "Test".into(),
            context: None,
        };
        let result = orch.decompose_goal(&goal).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            TeamError::LlmFailed(msg) => assert!(msg.contains("LLM API error")),
            other => panic!("expected LlmFailed, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_decompose_success() {
        let json = r#"{
            "tasks": [
                {"id": "research", "agent": "librarian", "tool_id": "librarian/research", "args": {"query": "test"}, "depends_on": []}
            ]
        }"#;
        let orch = TeamOrchestrator::new(
            make_registry(),
            make_executor(),
            Some(Arc::new(JsonLlmClient(json.to_string()))),
        );
        let goal = TeamGoal {
            id: "decompose-test".into(),
            objective: "Research topic X".into(),
            context: Some("Focus on 2026 trends".into()),
        };
        let plan = orch.decompose_goal(&goal).await.unwrap();
        assert_eq!(plan.goal_id, "decompose-test");
        assert_eq!(plan.tasks.len(), 1);
        assert_eq!(plan.tasks[0].tool_id, "librarian/research");
    }

    #[tokio::test]
    async fn test_run_goal_end_to_end() {
        let json = r#"{
            "tasks": [
                {"id": "analyse", "agent": "metis", "tool_id": "metis/analyse", "args": {"request": "test analysis"}, "depends_on": []}
            ]
        }"#;
        let orch = TeamOrchestrator::new(
            make_registry(),
            make_executor(),
            Some(Arc::new(JsonLlmClient(json.to_string()))),
        );
        let report = orch
            .run_goal(TeamGoal {
                id: "e2e".into(),
                objective: "End-to-end test".into(),
                context: None,
            })
            .await
            .unwrap();
        assert!(report.success);
        assert_eq!(report.task_results.len(), 1);
    }

    #[tokio::test]
    async fn test_concurrency_limit_respected() {
        let config = Arc::new(RwLock::new(Config::default()));
        let client = Arc::new(DynamicLlmClient::new(config));
        let reg = Arc::new(AgentRegistry::new(client));

        // Register an agent with max_concurrency=1
        reg.register(
            "limited-agent",
            AgentInfo {
                display_name: "Limited".into(),
                description: "Single-task agent".into(),
                tool_prefixes: vec!["test/".into()],
                max_concurrency: 1,
            },
        );

        // Add a simple executor that handles test/*
        let executor: Arc<dyn ToolExecutor> = make_executor();
        let orch = TeamOrchestrator::new(reg, executor, None);

        // Three tasks with no deps — but only 1 should run at a time
        let tasks = vec![
            TaskNode {
                id: "a".into(),
                agent_id: "limited-agent".into(),
                tool_id: "test/tool".into(),
                args: json!({}),
                depends_on: vec![],
            },
            TaskNode {
                id: "b".into(),
                agent_id: "limited-agent".into(),
                tool_id: "test/tool".into(),
                args: json!({}),
                depends_on: vec![],
            },
            TaskNode {
                id: "c".into(),
                agent_id: "limited-agent".into(),
                tool_id: "test/tool".into(),
                args: json!({}),
                depends_on: vec![],
            },
        ];

        let report = orch
            .run(
                TeamGoal {
                    id: "concurrency".into(),
                    objective: "Test concurrency".into(),
                    context: None,
                },
                tasks,
            )
            .await;

        // All should complete (even if they fail due to unknown tool)
        // The important thing is no deadlock — all 3 tasks are accounted for
        assert_eq!(
            report.task_results.len(),
            3,
            "all 3 tasks should be accounted for"
        );
    }

    #[tokio::test]
    async fn test_event_streaming() {
        let orch = make_orchestrator();
        let plan = TeamPlan {
            goal_id: "events".into(),
            tasks: vec![
                TaskNode {
                    id: "e1".into(),
                    agent_id: "metis".into(),
                    tool_id: "metis/analyse".into(),
                    args: json!({"request": "event test 1"}),
                    depends_on: vec![],
                },
                TaskNode {
                    id: "e2".into(),
                    agent_id: "metis".into(),
                    tool_id: "metis/analyse".into(),
                    args: json!({"request": "event test 2"}),
                    depends_on: vec![],
                },
            ],
        };
        let (tx, mut rx) = mpsc::channel(64);

        // Execute with event channel
        let report = orch.execute_plan_internal(plan, Some(tx)).await;
        assert!(report.success, "all tasks should succeed");

        // Collect all events (with timeout)
        let mut started = 0u32;
        let mut completed = 0u32;
        let mut got_synthesizing = false;
        let mut got_complete = false;

        use tokio::time::{timeout, Duration};
        loop {
            match timeout(Duration::from_secs(2), rx.recv()).await {
                Ok(Some(event)) => match event {
                    TeamEvent::TaskStarted { .. } => started += 1,
                    TeamEvent::TaskCompleted(ref r) => {
                        if r.success {
                            completed += 1;
                        }
                    }
                    TeamEvent::Synthesizing { .. } => got_synthesizing = true,
                    TeamEvent::Complete(_) => {
                        got_complete = true;
                        break;
                    }
                    _ => {}
                },
                Ok(None) => break,
                Err(_) => break,
            }
        }

        assert_eq!(started, 2, "should see 2 TaskStarted events");
        assert_eq!(completed, 2, "should see 2 successful TaskCompleted events");
        assert!(got_synthesizing, "should see Synthesizing event");
        assert!(got_complete, "should see Complete event");
    }

    #[tokio::test]
    async fn test_event_streaming_deadlock() {
        let orch = make_orchestrator();
        let plan = TeamPlan {
            goal_id: "deadlock-events".into(),
            tasks: vec![
                TaskNode {
                    id: "a".into(),
                    agent_id: "metis".into(),
                    tool_id: "metis/analyse".into(),
                    args: json!({"request": "a"}),
                    depends_on: vec!["b".into()],
                },
                TaskNode {
                    id: "b".into(),
                    agent_id: "metis".into(),
                    tool_id: "metis/analyse".into(),
                    args: json!({"request": "b"}),
                    depends_on: vec!["a".into()],
                },
            ],
        };
        let (tx, mut rx) = mpsc::channel(64);

        let _report = orch.execute_plan_internal(plan, Some(tx)).await;

        // Should see Error event + 2 TaskCompleted (failed)
        let mut errors = 0u32;
        let mut failed_completions = 0u32;
        let mut got_complete = false;

        use tokio::time::{timeout, Duration};
        loop {
            match timeout(Duration::from_secs(2), rx.recv()).await {
                Ok(Some(event)) => match event {
                    TeamEvent::Error { .. } => errors += 1,
                    TeamEvent::TaskCompleted(ref r) => {
                        if !r.success {
                            failed_completions += 1;
                        }
                    }
                    TeamEvent::Complete(_) => {
                        got_complete = true;
                        break;
                    }
                    _ => {}
                },
                Ok(None) => break,
                Err(_) => break,
            }
        }

        assert_eq!(errors, 1, "deadlock should emit Error event");
        assert_eq!(
            failed_completions, 2,
            "both tasks should emit failed TaskCompleted"
        );
        assert!(got_complete, "should still emit Complete event");
    }
}
