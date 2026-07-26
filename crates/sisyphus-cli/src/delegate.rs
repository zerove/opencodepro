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

pub async fn run_tasks(file: PathBuf, max_concurrent: usize) -> Result<()> {
    let content = std::fs::read_to_string(&file)
        .with_context(|| format!("read task file: {}", file.display()))?;
    let tasks: Vec<TaskDef> = serde_json::from_str(&content)
        .context("parse task JSON — expected array of {id, name, command}")?;

    if tasks.is_empty() {
        println!("No tasks defined.");
        return Ok(());
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

    // Drain events concurrently with handle to prevent channel deadlock
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

    // Drain any remaining events (channel may have closed before select! picked them)
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

pub async fn run_inline(commands: &[String], max_concurrent: usize) -> Result<()> {
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
        reports
            .iter()
            .filter(|r| r.status == crate::tools::WorkStatus::Completed)
            .count(),
        reports.len()
    );
    Ok(())
}

/// Run a team of agents to accomplish a complex goal
pub async fn run_team(
    goal: String,
    context: Option<String>,
    agents: Option<String>,
    max_concurrent: usize,
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

    // Build registry with selected agents
    let config = Arc::new(RwLock::new(Config::default()));
    let llm_client = Arc::new(DynamicLlmClient::new(config));
    let registry = Arc::new(AgentRegistry::new(llm_client.clone()));

    // Register built-in agents
    registry.register_builtin();

    // Filter agents if specified
    if let Some(agent_list) = agents {
        let selected: HashSet<String> = agent_list
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();

        // Keep only selected agents
        let all_agents = registry.list();
        for (id, _) in all_agents {
            if !selected.contains(&id) {
                registry.unregister(&id);
            }
        }
    }

    // Build executor
    let dummy_llm: Arc<dyn LlmClient> = Arc::new(DummyLlmClient);
    let feedback = Arc::new(crate::tools::FeedbackStore::default_dir());
    let executor = Arc::new(BuiltinToolExecutor {
        registry: crate::tools::HookRegistry::new(),
        llm_client: dummy_llm,
        feedback_store: feedback,
        bridge_client: None,
    });

    // Create orchestrator with planner LLM
    let orchestrator = TeamOrchestrator::new(registry, executor, Some(llm_client));

    // Run the team goal
    let team_goal = TeamGoal {
        id: Uuid::new_v4().to_string(),
        objective: goal,
        context,
    };

    println!("🚀 启动团队执行...");
    let report = orchestrator
        .run_goal(team_goal)
        .await
        .map_err(|e| anyhow::anyhow!("Team execution failed: {}", e))?;

    // Print results
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
