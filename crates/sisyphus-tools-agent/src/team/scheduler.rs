//! DAG execution engine for team plans.
//!
//! This module implements [`execute_plan_internal`], the core scheduling loop that
//! runs tasks respecting dependency order with per-agent concurrency limits,
//! deadlock detection, and optional event streaming.

use super::orchestrator::TeamOrchestrator;
use super::{TaskNode, TaskResult, TeamContext, TeamError, TeamEvent, TeamPlan, TeamReport};
use crate::delegate::{WorkOrchestrator, WorkUnit};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;

impl TeamOrchestrator {
    /// Execute a plan with optional event streaming.
    ///
    /// Same as [`execute_plan`] but also sends progress events through `event_tx`
    /// during execution (TaskStarted, TaskCompleted, Synthesizing, Complete).
    /// Returns the final report after all events have been sent.
    pub(crate) async fn execute_plan_internal(
        &self,
        plan: TeamPlan,
        event_tx: Option<mpsc::Sender<TeamEvent>>,
        context: Arc<TeamContext>,
    ) -> Result<TeamReport, TeamError> {
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
                        let task =
                            plan.tasks
                                .iter()
                                .find(|t| t.id == *task_id)
                                .ok_or_else(|| {
                                    TeamError::Internal("task id should exist in plan".to_string())
                                })?;
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
                        let task =
                            plan.tasks
                                .iter()
                                .find(|t| t.id == *task_id)
                                .ok_or_else(|| {
                                    TeamError::Internal("task id should exist in plan".to_string())
                                })?;
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
                    let context = context.clone();
                    let task_id = task.id.clone();
                    let read_keys = task.read_keys.clone();
                    let write_keys = task.write_keys.clone();
                    let agent_id = task.agent_id.clone();

                    // Inject context values into args for keys this task reads
                    let mut args = task.args.clone();
                    if !read_keys.is_empty() {
                        let mut ctx_values = serde_json::Map::new();
                        for key in &read_keys {
                            if let Some(value) = context.read_tracked(key, &task_id) {
                                ctx_values.insert(key.clone(), value);
                            }
                        }
                        if !ctx_values.is_empty() {
                            args["_context"] = Value::Object(ctx_values);
                        }
                    }

                    let args_bytes = serde_json::to_vec(&args).unwrap_or_default();

                    WorkUnit {
                        id: task_id.clone(),
                        name: format!("{}:{}", agent_id, tool_id),
                        handler: Box::new(move || {
                            let ex = executor.clone();
                            let tid = tool_id.clone();
                            let args = args_bytes.clone();
                            let ctx = context.clone();
                            let wid = write_keys.clone();
                            let tid_for_write = task_id.clone();
                            Box::pin(async move {
                                let result = ex.execute(&tid, args, 0).await;
                                match result {
                                    Ok(bytes) => {
                                        let value: Value =
                                            serde_json::from_slice(&bytes).unwrap_or(Value::Null);

                                        // Capture writes from task output
                                        // Convention: output["_writes"] = {"key": value, ...}
                                        if let Some(writes) =
                                            value.get("_writes").and_then(|v| v.as_object())
                                        {
                                            for (key, val) in writes {
                                                if wid.contains(key) || wid.is_empty() {
                                                    ctx.write(
                                                        key.clone(),
                                                        val.clone(),
                                                        &tid_for_write,
                                                    );
                                                }
                                            }
                                        }

                                        Ok(value)
                                    }
                                    Err(e) => {
                                        Err(sisyphus_tools_core::ToolError::Generic(e.to_string()))
                                    }
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
            iterations: 1, // Single iteration for basic execute_plan
        };

        if let Some(tx) = &event_tx {
            let _ = tx.try_send(TeamEvent::Complete(report.clone()));
        }

        Ok(report)
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
            Err(format!("Agent '{agent_id}' is not registered"))
        }
    }
}
