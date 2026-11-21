use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;
use std::pin::pin;
use tokio::select;

#[derive(Deserialize)]
struct TaskDef {
    id: String,
    name: String,
    command: String,
    #[serde(default)]
    dir: Option<String>,
}

/// Run tasks.
/// 执行任务文件里的命令。
///
/// 每条命令都先过**统一权限体系**（与 `exec` 同一口径）：危险命令默认需要人工
/// 确认，非交互环境直接拒绝。此前这里完全绕过权限闸。
pub async fn run_tasks(
    file: PathBuf,
    max_concurrent: usize,
    perms: &crate::permission::CliPermission,
    auto_approve: bool,
) -> Result<()> {
    let content = std::fs::read_to_string(&file)
        .with_context(|| format!("read task file: {}", file.display()))?;
    let tasks: Vec<TaskDef> = serde_json::from_str(&content)
        .context("parse task JSON — expected array of {id, name, command}")?;

    if tasks.is_empty() {
        println!("No tasks defined.");
        return Ok(());
    }

    // 每条命令单独判定并写审计（deny → 直接拒绝，不执行任何命令）。
    for t in &tasks {
        crate::permission::require_tool_with(perms, "exec", auto_approve)
            .with_context(|| format!("任务 {} 的命令被权限策略拒绝", t.id))?;
    }

    let units: Vec<crate::tools::WorkUnit> = tasks
        .into_iter()
        .map(|t| {
            crate::tools::work_unit(&t.id, &t.name, move || {
                let cmd = t.command.clone();
                let dir = t.dir.clone();
                async move {
                    let dir_ref = dir.as_deref().map(std::path::Path::new);
                    let output = crate::tools::bash::execute(&cmd, dir_ref, 60000).await?;
                    Ok(serde_json::json!({
                        "stdout": output.stdout,
                        "stderr": output.stderr,
                        "exit_code": output.exit_code,
                        "duration_ms": output.duration.as_millis(),
                    }))
                }
            })
        })
        .collect();

    let orchestrator = crate::tools::WorkOrchestrator::new(max_concurrent);
    let (mut rx, handle) = orchestrator.run_all_with_events(units)?;

    use crate::tools::DelegateEvent;
    let mut completed = 0usize;
    let total;
    let mut handle = pin!(handle);

    // 与句柄并发排空事件，防止通道死锁
    loop {
        select! {
            result = &mut handle => {
                total = result.context("task execution failed")?.len();
                break;
            }
            event = rx.recv() => {
                match event {
                    Some(DelegateEvent::Started { id, name }) => {
                        println!("[{id}] {name} ... running");
                    }
                    Some(DelegateEvent::Completed(report)) => {
                        completed += 1;
                        let status = if report.status == crate::tools::WorkStatus::Completed {
                            format!("OK ({}ms)", report.duration_ms)
                        } else {
                            format!("FAIL ({})", report.error.as_deref().unwrap_or("unknown"))
                        };
                        println!("[{}] {} ... {}", report.id, report.name, status);
                    }
                    Some(DelegateEvent::Progress { id, message }) => {
                        println!("[{id}] {message}");
                    }
                    None => {
                        total = completed;
                        break;
                    }
                }
            }
        }
    }

    // 排空剩余事件（select! 选中前通道可能已关闭）
    while let Ok(event) = rx.try_recv() {
        if let DelegateEvent::Completed(report) = event {
            completed += 1;
            let status = if report.status == crate::tools::WorkStatus::Completed {
                format!("OK ({}ms)", report.duration_ms)
            } else {
                format!("FAIL ({})", report.error.as_deref().unwrap_or("unknown"))
            };
            println!("[{}] {} ... {}", report.id, report.name, status);
        }
    }

    println!("\nDone: {}/{} tasks completed", completed, total);
    Ok(())
}

/// 使用配置文件运行代理团队
pub async fn run_team_with_config(
    goal: String,
    context: Option<String>,
    agents: Option<String>,
    max_concurrent: usize,
    team_config: sisyphus_tools_agent::team_config::TeamConfig,
) -> Result<()> {
    let _ = max_concurrent;
    use crate::tools::agent_registry::AgentRegistry;
    use crate::tools::executor::BuiltinToolExecutor;
    use crate::tools::llm::DynamicLlmClient;
    use crate::tools::metis::{DummyLlmClient, LlmClient};
    use crate::tools::team::{TeamGoal, TeamOrchestrator};
    use sisyphus_types::Config;
    use std::collections::HashSet;
    use std::sync::{Arc, RwLock};
    use uuid::Uuid;

    println!("🎯 团队目标: {goal}");
    if let Some(ref ctx) = context {
        println!("📝 上下文: {ctx}");
    }
    println!("⚙️  执行策略: {}", team_config.strategy);

    // 用选中的代理构建注册表
    let config = Arc::new(RwLock::new(Config::default()));
    let llm_client = Arc::new(DynamicLlmClient::new(config));
    let registry = Arc::new(AgentRegistry::new(llm_client.clone()));

    // 从配置文件注册代理
    if team_config.agents.is_empty() {
        // 如果配置文件没有指定代理，使用内置代理
        registry.register_builtin();
    } else {
        // 注册配置文件中启用的代理
        for agent_config in &team_config.agents {
            if agent_config.enabled {
                registry.register(&agent_config.id, agent_config.to_agent_info());
            }
        }
    }

    // 若指定则过滤代理 (CLI agents 参数优先)
    if let Some(agent_list) = agents {
        let selected: HashSet<String> =
            agent_list.split(',').map(|s| s.trim().to_string()).collect();

        // 仅保留选中的代理
        let all_agents = registry.list();
        for (id, _) in all_agents {
            if !selected.contains(&id) {
                registry.unregister(&id);
            }
        }
    }

    // 构建执行器（M1.2：将实际工具文件写入通过回调注入 TeamContext，作为外部/工具变更事件源）
    let team_context = Arc::new(crate::tools::team::TeamContext::new());
    // 可配置工作区影响策略：observe / record_affected / fail_affected_pending
    // 默认 record_affected：便于 CLI 路径审计与后续定向处理；不改变旧默认 Observe 的库级语义。
    let impact_policy = match std::env::var("SISYPHUS_TEAM_IMPACT_POLICY")
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "observe" => crate::tools::team::WorkspaceImpactPolicy::Observe,
        "fail_affected_pending" | "fail-affected-pending" | "fail" => {
            crate::tools::team::WorkspaceImpactPolicy::FailAffectedPending
        },
        _ => crate::tools::team::WorkspaceImpactPolicy::RecordAffected,
    };
    team_context.set_workspace_impact_policy(impact_policy);

    let mutation_ctx = team_context.clone();
    let file_mutation_sink = Some(Arc::new(move |path: String| {
        mutation_ctx.record_workspace_change(
            crate::tools::team::WorkspaceChange::new(
                path,
                crate::tools::team::WorkspaceChangeKind::Modified,
            )
            .with_source("tool:file_mutation_sink"),
        );
    }) as Arc<dyn Fn(String) + Send + Sync>);

    let dummy_llm: Arc<dyn LlmClient> = Arc::new(DummyLlmClient);
    let feedback = Arc::new(crate::tools::FeedbackStore::default_dir());
    let executor = Arc::new(BuiltinToolExecutor {
        registry: crate::tools::HookRegistry::new(),
        llm_client: dummy_llm,
        feedback_store: feedback,
        file_mutation_sink,
    });

    // 用规划 LLM 创建编排器（共享上下文，便于工具回调与调度层统一观测）
    let orchestrator =
        TeamOrchestrator::with_context(registry, executor, Some(llm_client), team_context.clone());

    // 运行团队目标（显式分解+执行，便于 S5 检查点与 M2.1 定向重试）
    let team_goal = TeamGoal {
        id: Uuid::new_v4().to_string(),
        objective: goal,
        context,
    };

    println!("🚀 启动团队执行...");
    let plan = orchestrator
        .decompose_goal(&team_goal)
        .await
        .map_err(|e| anyhow::anyhow!("Team goal decomposition failed: {}", e))?;
    let checkpoint_out = std::env::var("SISYPHUS_TEAM_CHECKPOINT_OUT")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let checkpoint_path = checkpoint_out.as_ref().map(std::path::Path::new);
    let interrupt_after_batches = std::env::var("SISYPHUS_TEAM_INTERRUPT_AFTER_BATCHES")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&n| n > 0);
    if let Some(n) = interrupt_after_batches {
        println!("⏸️  确定性中断注入已启用: {} 批次后中断（S5）", n);
    }
    let impact_retry_rounds = std::env::var("SISYPHUS_TEAM_IMPACT_RETRY_ROUNDS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(0);
    if impact_retry_rounds > 0 {
        println!(
            "🔁 工作区影响自动定向重跑已启用: 最多 {} 轮（M1.2）",
            impact_retry_rounds
        );
    }
    let mut report = if checkpoint_path.is_some() || interrupt_after_batches.is_some() {
        if let Some(path) = checkpoint_path {
            println!("💾 周期性检查点保存已启用: {}", path.display());
        }
        orchestrator
            .execute_plan_with_resume_and_checkpoint(
                plan.clone(),
                None,
                team_context.clone(),
                None,
                checkpoint_path,
                interrupt_after_batches,
            )
            .await
            .map_err(|e| anyhow::anyhow!("Team execution failed: {}", e))?
    } else if impact_retry_rounds > 0 {
        orchestrator
            .execute_plan_with_impact_retry(
                plan.clone(),
                None,
                team_context.clone(),
                impact_retry_rounds,
            )
            .await
            .map_err(|e| anyhow::anyhow!("Team execution failed: {}", e))?
    } else {
        orchestrator
            .execute_plan_with_context(plan.clone(), team_context.clone())
            .await
            .map_err(|e| anyhow::anyhow!("Team execution failed: {}", e))?
    };

    fn collect_verify_paths(
        team_context: &crate::tools::team::TeamContext,
        report: &crate::tools::team::TeamReport,
    ) -> Vec<std::path::PathBuf> {
        let mut verify_paths: Vec<std::path::PathBuf> = Vec::new();
        for change in team_context.workspace_changes() {
            let pb = std::path::PathBuf::from(&change.path);
            if (pb.is_absolute() || change.path.contains('/') || change.path.contains('\\'))
                && !verify_paths.iter().any(|x| x == &pb)
            {
                verify_paths.push(pb);
            }
        }
        for rec in &report.workspace_impact_records {
            for p in &rec.changed_paths {
                let pb = std::path::PathBuf::from(p);
                if (pb.is_absolute() || p.contains('/') || p.contains('\\'))
                    && !verify_paths.iter().any(|x| x == &pb)
                {
                    verify_paths.push(pb);
                }
            }
        }
        verify_paths
    }

    fn print_team_report(report: &crate::tools::team::TeamReport) {
        println!("\n=== 团队执行报告 ===");
        println!("目标 ID: {}", report.goal_id);
        println!(
            "整体状态: {}",
            if report.success {
                "✅ 成功"
            } else {
                "❌ 失败"
            }
        );
        println!("任务数: {}", report.task_results.len());
        println!(
            "成功: {}",
            report.task_results.iter().filter(|r| r.success).count()
        );
        println!(
            "失败: {}",
            report.task_results.iter().filter(|r| !r.success).count()
        );
        println!("总耗时: {}ms", report.total_duration_ms);
        if report.conflict_stats.detected > 0 || report.conflict_stats.conflict_batches > 0 {
            println!(
                "并行冲突: 检测 {} 次 (write-write {}, read-write {}), 冲突批次 {}, 串行化波次 {}",
                report.conflict_stats.detected,
                report.conflict_stats.write_write,
                report.conflict_stats.read_write,
                report.conflict_stats.conflict_batches,
                report.conflict_stats.serialized_waves
            );
        }
        if !report.workspace_impact_records.is_empty() {
            println!(
                "工作区影响记录: {} 条",
                report.workspace_impact_records.len()
            );
            for rec in &report.workspace_impact_records {
                println!(
                    "  - 批次 {}: 变更 {} 项 -> 受影响任务 {:?}",
                    rec.batch_index,
                    rec.changed_paths.len(),
                    rec.affected_task_ids
                );
            }
        }
    }

    print_team_report(&report);

    // S5：执行结束后再保存一次最终检查点快照（若启用周期性保存则覆盖为最终状态）
    if let Some(path) = checkpoint_out.as_deref() {
        let ckpt = crate::tools::team::TeamCheckpoint::from_report(
            plan.clone(),
            team_context.as_ref(),
            &report,
        );
        if let Err(e) = ckpt.save_to_file(path) {
            println!("⚠️  保存团队检查点失败 ({}): {}", path, e);
        } else {
            println!("💾 团队检查点已保存(最终): {}", path);
        }
    }

    // M2.1：编排后验证 + 可选 verify-fix 钩子 + 可选定向重试（支持多轮）
    let fix_cmd = std::env::var("SISYPHUS_TEAM_VERIFY_FIX_CMD")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let verify_retry = matches!(
        std::env::var("SISYPHUS_TEAM_VERIFY_RETRY")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "1" | "true" | "yes" | "on"
    );
    let fix_rounds = std::env::var("SISYPHUS_TEAM_VERIFY_FIX_ROUNDS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(1);

    let mut verify_failed = false;
    let verify_paths = collect_verify_paths(&team_context, &report);
    if !verify_paths.is_empty() {
        if let Some(outcome) = crate::post_edit::verify_after_edit(&verify_paths)? {
            println!("\n--- 编排后验证 ---");
            println!("{}", outcome.summary_line());
            verify_failed = outcome.status.is_failure();
            if verify_failed {
                println!("验证未通过：将结合受影响任务与变更路径进行定向排查/修复。");
            }
        }
    }

    for round in 1..=fix_rounds {
        if !verify_failed {
            break;
        }
        let mut did_action = false;

        if let Some(cmd) = &fix_cmd {
            println!("\n--- verify-fix 修复尝试 (第 {} 轮) ---", round);
            println!("执行修复命令: {}", cmd);
            let fix_status = std::process::Command::new("sh").arg("-lc").arg(cmd).status();
            match fix_status {
                Ok(st) => println!("修复命令退出码: {:?}", st.code()),
                Err(e) => println!("修复命令执行失败: {e}"),
            }
            did_action = true;
            let verify_paths_fix = collect_verify_paths(&team_context, &report);
            if !verify_paths_fix.is_empty() {
                if let Some(outcome2) = crate::post_edit::verify_after_edit(&verify_paths_fix)? {
                    println!("\n--- 编排后验证（修复后，第 {} 轮）---", round);
                    println!("{}", outcome2.summary_line());
                    verify_failed = outcome2.status.is_failure();
                }
            } else {
                verify_failed = false;
            }
        }

        if verify_failed && verify_retry {
            let impact_failed_ids: Vec<String> = report
                .task_results
                .iter()
                .filter(|r| r.is_workspace_impact_failure())
                .map(|r| r.task_id.clone())
                .collect();
            if !impact_failed_ids.is_empty() {
                println!(
                    "\n--- 定向重试（workspace impact 失败任务，第 {} 轮）---\n目标任务: {:?}",
                    round, impact_failed_ids
                );
                let resume = crate::tools::team::TeamResumeState {
                    completed_results: report.task_results.clone(),
                    conflict_stats: report.conflict_stats.clone(),
                    processed_change_count: report.processed_change_count,
                }
                .without_workspace_impact_failures();
                report = orchestrator
                    .execute_plan_with_resume(
                        plan.clone(),
                        None,
                        team_context.clone(),
                        Some(resume),
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("Targeted retry execution failed: {}", e))?;
                print_team_report(&report);
                did_action = true;

                let verify_paths_retry = collect_verify_paths(&team_context, &report);
                if !verify_paths_retry.is_empty() {
                    if let Some(outcome) = crate::post_edit::verify_after_edit(&verify_paths_retry)?
                    {
                        println!("\n--- 编排后验证（重试后，第 {} 轮）---", round);
                        println!("{}", outcome.summary_line());
                        verify_failed = outcome.status.is_failure();
                    }
                } else {
                    verify_failed = false;
                }
            }
        }

        if !did_action {
            break;
        }
    }

    if let Some(synthesis) = &report.synthesis {
        println!("\n--- 综合总结 ---");
        println!("{synthesis}");
    }

    println!("\n--- 任务详情 ---");
    for task in &report.task_results {
        let status = if task.success { "✅" } else { "❌" };
        println!(
            "{status} [{}] {} ({}ms)",
            task.agent_id, task.task_id, task.duration_ms
        );
        if let Some(err) = &task.error {
            println!("    错误: {err}");
        }
        if let Some(output) = &task.output {
            let preview = output.to_string();
            let preview = if preview.len() > 200 {
                format!("{}...", &preview[..200])
            } else {
                preview
            };
            println!("    输出: {preview}");
        }
    }

    Ok(())
}

/// Run inline.
/// 内联执行命令列表。
///
/// 每条命令都先过**统一权限体系**（与 `exec` 同一口径）。
pub async fn run_inline(
    commands: &[String],
    max_concurrent: usize,
    perms: &crate::permission::CliPermission,
    auto_approve: bool,
) -> Result<()> {
    // 每条命令单独判定并写审计（deny → 直接拒绝，不执行任何命令）。
    for cmd in commands {
        crate::permission::require_tool_with(perms, "exec", auto_approve)
            .with_context(|| format!("命令被权限策略拒绝: {cmd}"))?;
    }

    let units: Vec<crate::tools::WorkUnit> = commands
        .iter()
        .enumerate()
        .map(|(i, cmd)| {
            let id = format!("task-{i}");
            let cmd = cmd.clone();
            let cmd_display = cmd[..cmd.len().min(40)].to_string();
            crate::tools::work_unit(&id, &cmd_display, move || {
                let cmd = cmd.clone();
                async move {
                    let output = crate::tools::bash::execute(&cmd, None, 60000).await?;
                    Ok(serde_json::json!({
                        "stdout": output.stdout,
                        "stderr": output.stderr,
                        "exit_code": output.exit_code,
                        "duration_ms": output.duration.as_millis(),
                    }))
                }
            })
        })
        .collect();

    let orchestrator = crate::tools::WorkOrchestrator::new(max_concurrent);
    let reports = orchestrator.run_all(units).await?;

    for r in &reports {
        let tag = if r.status == crate::tools::WorkStatus::Completed {
            "OK"
        } else {
            "FAIL"
        };
        println!("[{}] {} {} ({}ms)", r.id, tag, r.name, r.duration_ms);
    }
    println!(
        "\nDone: {}/{}",
        reports.iter().filter(|r| r.status == crate::tools::WorkStatus::Completed).count(),
        reports.len()
    );
    Ok(())
}

/// 运行代理团队完成复杂目标
pub async fn run_team(
    goal: String,
    context: Option<String>,
    agents: Option<String>,
    max_concurrent: usize,
) -> Result<()> {
    let team_config = sisyphus_tools_agent::team_config::TeamConfig::default();
    run_team_with_config(goal, context, agents, max_concurrent, team_config).await
}
