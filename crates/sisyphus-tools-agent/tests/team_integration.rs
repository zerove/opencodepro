//! Team 编排真实 LLM 集成测试
//!
//! 运行条件：
//! - 设置环境变量 LLM_API_KEY、LLM_ENDPOINT、LLM_MODEL
//! - 或在 config.toml 中配置 [llm] 节点
//! - 否则测试会被跳过（而非失败）

use sisyphus_tools_agent::agent_registry::AgentRegistry;
use sisyphus_tools_agent::team::{TeamGoal, TeamOrchestrator, TeamWorkflow};
use sisyphus_tools_core::feedback::FeedbackStore;
use sisyphus_tools_core::hook::HookRegistry;
use sisyphus_tools_executor::executor::BuiltinToolExecutor;
use sisyphus_tools_llm::metis::LlmClient;
use sisyphus_tools_llm::DynamicLlmClient;
use sisyphus_types::Config;
use std::env;
use std::sync::Arc;

/// 获取真实 LLM 客户端，未配置时返回 None
fn get_real_llm_client() -> Option<Arc<dyn LlmClient>> {
    let api_key = env::var("LLM_API_KEY")
        .ok()
        .or_else(|| env::var("SISYPHUS_LLM_API_KEY").ok())
        .filter(|s| !s.is_empty() && s != "mock");

    let endpoint = env::var("LLM_ENDPOINT")
        .ok()
        .or_else(|| env::var("SISYPHUS_LLM_ENDPOINT").ok())
        .unwrap_or_else(|| "http://localhost:8000/v1/chat/completions".into());

    let model = env::var("LLM_MODEL")
        .ok()
        .or_else(|| env::var("SISYPHUS_LLM_MODEL").ok())
        .unwrap_or_else(|| "gpt-4o".into());

    api_key.map(|key| {
        let cfg = Config {
            llm: sisyphus_types::config::LlmConfig {
                api_key: key,
                endpoint,
                model,
                embedding: sisyphus_types::EmbeddingConfig::default(),
                budget_mode: sisyphus_types::BudgetMode::Warning,
                fast_fail_cooling_secs: 30,
                request_timeout_secs: 120,
                connect_timeout_secs: 10,
                agents: Default::default(),
                categories: Default::default(),
                providers: vec![],
            },
            ..Default::default()
        };
        Arc::new(DynamicLlmClient::new(Arc::new(std::sync::RwLock::new(cfg)))) as Arc<dyn LlmClient>
    })
}

/// 创建集成测试用的 orchestrator
async fn make_integration_orchestrator() -> Option<TeamOrchestrator> {
    let llm = get_real_llm_client()?;
    let reg = Arc::new(AgentRegistry::new(llm.clone()));
    reg.register_builtin();

    let feedback = Arc::new(FeedbackStore::default_dir());
    let hooks = HookRegistry::new();
    let executor = Arc::new(BuiltinToolExecutor {
        registry: hooks,
        llm_client: llm.clone(),
        feedback_store: feedback,
        bridge_client: None,
    });

    Some(TeamOrchestrator::new(reg, executor, Some(llm)))
}

/// 测试辅助：运行目标并打印报告摘要
async fn run_and_report(
    orch: &TeamOrchestrator,
    goal: TeamGoal,
    workflow: TeamWorkflow,
    label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n========== {} ==========", label);
    println!("Goal: {} - {}", goal.id, goal.objective);
    println!("Workflow: {:?}", workflow);

    let report = orch.run_goal_workflow(goal, workflow).await?;

    println!("Success: {}", report.success);
    println!("Iterations: {}", report.iterations);
    println!("Tasks executed: {}", report.task_results.len());
    println!("Total duration: {} ms", report.total_duration_ms);

    for (i, result) in report.task_results.iter().enumerate() {
        println!(
            "  Task {}: {} ({}) - {} - {} ms",
            i + 1,
            result.task_id,
            result.agent_id,
            if result.success { "OK" } else { "FAILED" },
            result.duration_ms
        );
        if let Some(err) = &result.error {
            println!("    Error: {}", err);
        }
    }

    if let Some(synthesis) = &report.synthesis {
        println!("Synthesis:\n{}", synthesis);
    }

    if !report.success {
        return Err(format!("{} failed", label).into());
    }

    Ok(())
}

#[tokio::test]
#[ignore = "requires LLM_API_KEY, LLM_ENDPOINT, LLM_MODEL env vars"]
async fn test_team_single_pass_real() {
    let Some(orch) = make_integration_orchestrator().await else {
        eprintln!("Skipping: LLM not configured");
        return;
    };

    let goal = TeamGoal {
        id: "integration-single-pass".into(),
        objective: "分析 Rust 项目中常见的内存安全模式，并给出 3 个具体代码示例".into(),
        context: Some("目标受众：中级 Rust 开发者".into()),
    };

    run_and_report(&orch, goal, TeamWorkflow::SinglePass, "SinglePass 真实测试").await.unwrap();
}

#[tokio::test]
#[ignore = "requires LLM_API_KEY, LLM_ENDPOINT, LLM_MODEL env vars"]
async fn test_team_review_iterate_real() {
    let Some(orch) = make_integration_orchestrator().await else {
        eprintln!("Skipping: LLM not configured");
        return;
    };

    let goal = TeamGoal {
        id: "integration-review-iterate".into(),
        objective: "设计一个简易的任务调度器接口，要求：支持依赖图、优先级、超时控制、并发限制"
            .into(),
        context: Some("用于单机异步任务编排，Rust async 生态".into()),
    };

    run_and_report(
        &orch,
        goal,
        TeamWorkflow::ReviewIterate { max_rounds: 2 },
        "ReviewIterate 真实测试",
    )
    .await
    .unwrap();
}

#[tokio::test]
#[ignore = "requires LLM_API_KEY, LLM_ENDPOINT, LLM_MODEL env vars"]
async fn test_team_research_iterate_real() {
    let Some(orch) = make_integration_orchestrator().await else {
        eprintln!("Skipping: LLM not configured");
        return;
    };

    let goal = TeamGoal {
        id: "integration-research-iterate".into(),
        objective:
            "调研 Rust 异步运行时对比：tokio vs async-std vs smol 在延迟、吞吐、内存占用上的差异"
                .into(),
        context: Some("需要具体基准数据和选型建议".into()),
    };

    run_and_report(
        &orch,
        goal,
        TeamWorkflow::ResearchIterate { max_rounds: 2 },
        "ResearchIterate 真实测试",
    )
    .await
    .unwrap();
}

#[tokio::test]
#[ignore = "requires LLM_API_KEY, LLM_ENDPOINT, LLM_MODEL env vars"]
async fn test_team_adaptive_real() {
    let Some(orch) = make_integration_orchestrator().await else {
        eprintln!("Skipping: LLM not configured");
        return;
    };

    let goal = TeamGoal {
        id: "integration-adaptive".into(),
        objective: "制定一个 CI/CD 流水线改进计划：从当前的基础检查扩展到包含安全扫描、性能基准、依赖审计、自动发布".into(),
        context: Some("GitHub Actions 环境，Rust 项目".into()),
    };

    run_and_report(
        &orch,
        goal,
        TeamWorkflow::Adaptive { max_iterations: 3 },
        "Adaptive 真实测试",
    )
    .await
    .unwrap();
}

#[tokio::test]
#[ignore = "requires LLM_API_KEY, LLM_ENDPOINT, LLM_MODEL env vars"]
async fn test_team_streamed_events_real() {
    let Some(orch) = make_integration_orchestrator().await else {
        eprintln!("Skipping: LLM not configured");
        return;
    };

    use sisyphus_tools_agent::team::TeamEvent;

    let goal = TeamGoal {
        id: "integration-streamed".into(),
        objective: "用一句话解释什么是所有权（ownership）在 Rust 中的含义".into(),
        context: None,
    };

    let orch_arc = Arc::new(orch);
    let mut rx = orch_arc.run_goal_streamed(goal).await.unwrap();

    let mut event_count = 0;
    let mut complete = false;
    while let Some(event) = rx.recv().await {
        event_count += 1;
        println!("Event {}: {:?}", event_count, event);
        if matches!(event, TeamEvent::Complete(_)) {
            complete = true;
            break;
        }
    }

    assert!(complete, "should receive Complete event");
    assert!(
        event_count >= 3,
        "should see Planning, TaskStarted, TaskCompleted, Complete at minimum"
    );
    println!("Total events received: {}", event_count);
}

/// 并发执行多个目标的压力测试
#[tokio::test]
#[ignore = "requires LLM_API_KEY, LLM_ENDPOINT, LLM_MODEL env vars"]
async fn test_team_concurrent_goals_real() {
    let Some(orch) = make_integration_orchestrator().await else {
        eprintln!("Skipping: LLM not configured");
        return;
    };

    let orch = Arc::new(orch);
    let mut handles = vec![];

    for i in 0..3 {
        let orch = orch.clone();
        let handle = tokio::spawn(async move {
            let goal = TeamGoal {
                id: format!("concurrent-{}", i),
                objective: format!(
                    "简述 Rust 中 {} 的核心概念和用法",
                    ["Pin", "Future", "Send"][i]
                ),
                context: None,
            };
            orch.run_goal_workflow(goal, TeamWorkflow::SinglePass).await
        });
        handles.push(handle);
    }

    let mut success_count = 0;
    for handle in handles {
        if let Ok(Ok(report)) = handle.await {
            if report.success {
                success_count += 1;
            }
        }
    }

    println!("Concurrent goals: {}/3 succeeded", success_count);
    assert!(
        success_count >= 2,
        "at least 2 of 3 concurrent goals should succeed"
    );
}
