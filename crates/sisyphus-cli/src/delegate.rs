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
        let selected: HashSet<String> = agent_list
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();

        // 仅保留选中的代理
        let all_agents = registry.list();
        for (id, _) in all_agents {
            if !selected.contains(&id) {
                registry.unregister(&id);
            }
        }
    }

    // 构建执行器
    let dummy_llm: Arc<dyn LlmClient> = Arc::new(DummyLlmClient);
    let feedback = Arc::new(crate::tools::FeedbackStore::default_dir());
    let executor = Arc::new(BuiltinToolExecutor {
        registry: crate::tools::HookRegistry::new(),
        llm_client: dummy_llm,
        feedback_store: feedback,
        bridge_client: None,
    });

    // 用规划 LLM 创建编排器
    let orchestrator = TeamOrchestrator::new(registry, executor, Some(llm_client));

    // 运行团队目标
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

    // 打印结果
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

/// Run inline.
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
