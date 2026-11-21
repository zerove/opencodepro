//! DAG execution engine for team plans.
//!
//! This module implements [`execute_plan_internal`], the core scheduling loop that
//! runs tasks respecting dependency order with per-agent concurrency limits,
//! deadlock detection, and optional event streaming.

use super::checkpoint::TeamResumeState;
use super::conflict::{
    detect_conflicts, partition_by_write_conflicts, write_keys_of, ConflictKind,
};
use super::orchestrator::TeamOrchestrator;
use super::{
    ConflictStats, DelegationPolicy, TaskNode, TaskResult, TeamContext, TeamError, TeamEvent,
    TeamPlan, TeamReport, WorkspaceChange, WorkspaceChangeKind,
};
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
        self.execute_plan_with_resume(plan, event_tx, context, None).await
    }

    /// Execute a plan from a checkpoint/resume state (M0.3/S5 最小恢复路径)。
    pub async fn execute_plan_with_resume(
        &self,
        plan: TeamPlan,
        event_tx: Option<mpsc::Sender<TeamEvent>>,
        context: Arc<TeamContext>,
        resume: Option<TeamResumeState>,
    ) -> Result<TeamReport, TeamError> {
        self.execute_plan_with_resume_and_checkpoint(plan, event_tx, context, resume, None, None)
            .await
    }

    /// 执行计划，并对因工作区影响被标记失败的任务自动执行定向重跑（M1.2）。
    ///
    /// - 首轮正常执行（不注入恢复态）。
    /// - 若报告存在 `is_workspace_impact_failure()` 的失败任务，且仍有剩余轮次，
    ///   则从上一轮结果构造恢复态（排除影响类失败），仅重新调度这些任务；
    ///   已成功或非影响类失败的任务不会重复执行。
    /// - 每轮重试前发出 [`TeamEvent::ImpactRetry`]；轮次上限保证终止。
    /// - 返回最终合并报告，`iterations` = 1 + 实际使用的重试轮数。
    pub async fn execute_plan_with_impact_retry(
        &self,
        plan: TeamPlan,
        event_tx: Option<mpsc::Sender<TeamEvent>>,
        context: Arc<TeamContext>,
        max_rounds: usize,
    ) -> Result<TeamReport, TeamError> {
        let mut report = self
            .execute_plan_with_resume(plan.clone(), event_tx.clone(), context.clone(), None)
            .await?;
        let mut rounds = 0usize;

        while rounds < max_rounds {
            let retry_ids: Vec<String> = report
                .task_results
                .iter()
                .filter(|r| r.is_workspace_impact_failure())
                .map(|r| r.task_id.clone())
                .collect();
            if retry_ids.is_empty() {
                break;
            }

            rounds += 1;
            tracing::info!(
                round = rounds,
                tasks = ?retry_ids,
                "workspace-impact auto-retry: re-executing affected tasks"
            );
            if let Some(tx) = &event_tx {
                let _ = tx.try_send(TeamEvent::ImpactRetry {
                    round: rounds,
                    task_ids: retry_ids,
                });
            }

            let resume = TeamResumeState {
                completed_results: report.task_results.clone(),
                conflict_stats: report.conflict_stats.clone(),
                processed_change_count: report.processed_change_count,
            }
            .without_workspace_impact_failures();

            report = self
                .execute_plan_with_resume(
                    plan.clone(),
                    event_tx.clone(),
                    context.clone(),
                    Some(resume),
                )
                .await?;
            report.iterations = rounds + 1;
        }

        if rounds > 0 {
            let note =
                format!("\n[impact-retry] re-ran workspace-impact tasks in {rounds} round(s)");
            report.synthesis = Some(match report.synthesis.take() {
                Some(mut s) => {
                    s.push_str(&note);
                    s
                },
                None => note.trim().to_string(),
            });
        }

        Ok(report)
    }

    /// Execute a plan from a checkpoint/resume state with optional periodic checkpoint persistence (S5).
    ///
    /// When `checkpoint_path` is provided, a checkpoint snapshot is saved after each scheduling batch
    /// (best-effort; save failures are logged and do not abort execution).
    /// When `interrupt_after_batches` is set, execution stops after that many batches if tasks remain,
    /// saving a checkpoint (if configured) and returning a partial interrupted report (deterministic
    /// interrupt injection for S5 testing/recovery).
    pub async fn execute_plan_with_resume_and_checkpoint(
        &self,
        plan: TeamPlan,
        event_tx: Option<mpsc::Sender<TeamEvent>>,
        context: Arc<TeamContext>,
        resume: Option<TeamResumeState>,
        checkpoint_path: Option<&std::path::Path>,
        interrupt_after_batches: Option<usize>,
    ) -> Result<TeamReport, TeamError> {
        let start = Instant::now();
        let mut task_results: Vec<TaskResult> = Vec::new();
        let mut completed: HashMap<String, TaskResult> = HashMap::new();
        let mut conflict_stats = ConflictStats::default();
        let mut processed_change_count: usize = context.workspace_changes().len();

        if let Some(state) = resume {
            conflict_stats = state.conflict_stats;
            processed_change_count =
                state.processed_change_count.min(context.workspace_changes().len());
            for result in state.completed_results {
                if plan.tasks.iter().any(|t| t.id == result.task_id) {
                    task_results.push(result.clone());
                    completed.insert(result.task_id.clone(), result);
                }
            }
        }

        // Pre-validate all tasks against the registry (skip tasks already completed/resumed)
        for task in &plan.tasks {
            if completed.contains_key(&task.id) {
                continue;
            }
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
        let mut batch_index: usize = 0;

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
                            plan.tasks.iter().find(|t| t.id == *task_id).ok_or_else(|| {
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
                            plan.tasks.iter().find(|t| t.id == *task_id).ok_or_else(|| {
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

            // T3.4 / M1.3：并行冲突检测 —— 同批任务写同一资源时告警，并拆波次串行化
            let conflicts = detect_conflicts(&ready);
            if !conflicts.is_empty() {
                conflict_stats.conflict_batches += 1;
                conflict_stats.detected += conflicts.len();
                for conflict in &conflicts {
                    match conflict.kind {
                        ConflictKind::WriteWrite => conflict_stats.write_write += 1,
                        ConflictKind::ReadWrite => conflict_stats.read_write += 1,
                    }
                    tracing::warn!(
                        kind = conflict.kind.as_str(),
                        key = %conflict.key,
                        tasks = ?conflict.task_ids,
                        "parallel task conflict detected; conflicting tasks will be serialized"
                    );
                    if let Some(tx) = &event_tx {
                        let _ = tx.try_send(TeamEvent::ConflictDetected {
                            kind: conflict.kind.as_str().to_string(),
                            key: conflict.key.clone(),
                            task_ids: conflict.task_ids.clone(),
                        });
                    }
                }
            }
            // 写冲突的任务分到不同波次：波次之间顺序执行，波次内部并行
            let waves = partition_by_write_conflicts(&ready);
            if !conflicts.is_empty() {
                conflict_stats.serialized_waves += waves.len();
            }

            for wave in waves {
                // Execute ready tasks in parallel
                let units: Vec<WorkUnit> = wave
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

                        // Resolve the delegation policy (`args["_delegation"]` convention):
                        // retries, per-attempt timeout, and fallback agents.
                        // Fallback agents are pre-validated here so the handler only
                        // deals with retry + timeout + candidate sequencing.
                        let policy = DelegationPolicy::from_args(&args);
                        let mut candidates: Vec<String> =
                            Vec::with_capacity(1 + policy.fallback_agents.len());
                        candidates.push(agent_id.clone());
                        for fallback in &policy.fallback_agents {
                            if fallback != &agent_id
                                && self.validate_tool_for_agent(fallback, &tool_id).is_ok()
                            {
                                candidates.push(fallback.clone());
                            }
                        }
                        let retries = policy.retries;
                        let timeout_ms = policy.timeout_ms;

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
                                    // Delegation loop: primary agent first, then fallbacks,
                                    // each with up to `retries` retries after the first attempt.
                                    let mut last_error: Option<String> = None;
                                    for candidate in &candidates {
                                        for _ in 0..=retries {
                                            let fut = ex.execute(&tid, args.clone(), 0);
                                            let result = match timeout_ms {
                                                Some(ms) if ms > 0 => {
                                                    match tokio::time::timeout(
                                                        std::time::Duration::from_millis(ms),
                                                        fut,
                                                    )
                                                    .await
                                                    {
                                                        Ok(res) => res,
                                                        Err(_) => Err(
                                                            sisyphus_types::TypesError::InternalError(
                                                                format!("task timed out after {ms}ms"),
                                                            ),
                                                        ),
                                                    }
                                                },
                                                _ => fut.await,
                                            };
                                            match result {
                                                Ok(bytes) => {
                                                    let mut value: Value =
                                                        serde_json::from_slice(&bytes)
                                                            .unwrap_or(Value::Null);

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

                                                    // Annotate which agent actually produced
                                                    // the result (primary or fallback).
                                                    if let Value::Object(map) = &mut value {
                                                        map.insert(
                                                            "_delegated_agent".into(),
                                                            Value::String(candidate.clone()),
                                                        );
                                                    }

                                                    return Ok(value);
                                                },
                                                Err(e) => last_error = Some(e.to_string()),
                                            }
                                        }
                                    }
                                    Err(sisyphus_tools_core::ToolError::Generic(
                                        last_error.unwrap_or_else(|| {
                                            "delegation failed: no candidates".into()
                                        }),
                                    ))
                                })
                            }),
                        }
                    })
                    .collect();

                let orchestrator = WorkOrchestrator::new(wave.len());
                let reports = orchestrator.run_all(units).await.unwrap_or_default();

                for (i, report) in reports.iter().enumerate() {
                    let task = wave[i];
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
                    // M1.2（最小闭环）：成功任务完成后记录工作区变更（按推断/声明写键）
                    if result.success {
                        for key in write_keys_of(task) {
                            context.record_workspace_change(
                                WorkspaceChange::new(key, WorkspaceChangeKind::Modified)
                                    .with_source(task.id.clone()),
                            );
                        }
                    }
                    // Emit TaskCompleted
                    if let Some(tx) = &event_tx {
                        let _ = tx.try_send(TeamEvent::TaskCompleted(result.clone()));
                    }
                    task_results.push(result.clone());
                    completed.insert(task.id.clone(), result);
                }
            }

            // M1.2（最小闭环）：批次执行完成后，消费新增工作区变更（含外部/回调记录）并评估对未完成任务的影响
            let all_changes = context.workspace_changes();
            if all_changes.len() > processed_change_count {
                let new_changes = all_changes[processed_change_count..].to_vec();
                processed_change_count = all_changes.len();

                let mut changed_paths: Vec<String> = Vec::new();
                for c in &new_changes {
                    if !changed_paths.iter().any(|p| p == &c.path) {
                        changed_paths.push(c.path.clone());
                    }
                }

                if !changed_paths.is_empty() {
                    let pending: Vec<&TaskNode> = plan
                        .tasks
                        .iter()
                        .filter(|t| remaining.contains(&t.id) && !completed.contains_key(&t.id))
                        .collect();
                    if !pending.is_empty() {
                        let affected_task_ids =
                            context.affected_tasks_by_changes(&pending, &new_changes);
                        if !affected_task_ids.is_empty() {
                            tracing::info!(
                                batch_index,
                                changed = ?changed_paths,
                                affected = ?affected_task_ids,
                                "workspace changes may affect pending tasks; reevaluation suggested"
                            );
                            let policy = context.workspace_impact_policy();
                            if matches!(
                                policy,
                                super::context::WorkspaceImpactPolicy::RecordAffected
                                    | super::context::WorkspaceImpactPolicy::FailAffectedPending
                            ) {
                                context.record_workspace_impact(
                                    batch_index,
                                    changed_paths.clone(),
                                    affected_task_ids.clone(),
                                );
                            }
                            if let Some(tx) = &event_tx {
                                let _ = tx.try_send(TeamEvent::WorkspaceImpact {
                                    batch_index,
                                    changed_paths: changed_paths.clone(),
                                    affected_task_ids: affected_task_ids.clone(),
                                });
                            }
                            if policy == super::context::WorkspaceImpactPolicy::FailAffectedPending
                            {
                                for task_id in &affected_task_ids {
                                    if !remaining.contains(task_id)
                                        || completed.contains_key(task_id)
                                    {
                                        continue;
                                    }
                                    if let Some(task) = plan.tasks.iter().find(|t| &t.id == task_id)
                                    {
                                        let result = TaskResult::failed(
                                            task,
                                            format!(
                                                "workspace impact: pending task affected by changes {:?} in batch {batch_index}",
                                                changed_paths
                                            ),
                                        );
                                        if let Some(tx) = &event_tx {
                                            let _ = tx
                                                .try_send(TeamEvent::TaskCompleted(result.clone()));
                                        }
                                        task_results.push(result.clone());
                                        completed.insert(task.id.clone(), result);
                                        remaining.remove(&task.id);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            for task in &ready {
                remaining.remove(&task.id);
            }
            batch_index += 1;

            // S5：周期性检查点持久化（每批次后 best-effort 保存，便于中断恢复）
            if let Some(path) = checkpoint_path {
                let ckpt = super::checkpoint::TeamCheckpoint::from_state(
                    plan.goal_id.clone(),
                    plan.clone(),
                    task_results.clone(),
                    conflict_stats.clone(),
                    context.as_ref(),
                    processed_change_count,
                );
                if let Err(e) = ckpt.save_to_file(path) {
                    tracing::warn!(batch_index, path = %path.display(), error = %e, "failed to save periodic team checkpoint");
                }
            }

            // S5：确定性中断注入（用于测试/可控恢复切片）
            if let Some(limit) = interrupt_after_batches {
                if batch_index >= limit && !remaining.is_empty() {
                    tracing::info!(
                        batch_index,
                        remaining = remaining.len(),
                        "team execution interrupted after batch limit"
                    );
                    let interrupted_report = TeamReport {
                        goal_id: plan.goal_id.clone(),
                        success: false,
                        task_results: task_results.clone(),
                        synthesis: Some(format!(
                            "Interrupted after {} batch(es): {} task(s) remaining",
                            batch_index,
                            remaining.len()
                        )),
                        total_duration_ms: start.elapsed().as_millis(),
                        iterations: 1,
                        conflict_stats: conflict_stats.clone(),
                        workspace_impact_records: context.workspace_impact_records(),
                        processed_change_count,
                    };
                    if let Some(tx) = &event_tx {
                        let _ = tx.try_send(TeamEvent::Complete(interrupted_report.clone()));
                    }
                    return Ok(interrupted_report);
                }
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
            conflict_stats,
            workspace_impact_records: context.workspace_impact_records(),
            processed_change_count,
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
