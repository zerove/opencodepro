use crate::bridge::BridgeClient;
use anyhow::Result;

/// Run benchmarks.
pub async fn run_benchmarks(client: &mut BridgeClient) -> Result<()> {
    let benchmarks = client.list_benchmarks().await?;
    if benchmarks.is_empty() {
        println!("No benchmarks found.");
        println!("提示: 先通过 bridge 执行任务/benchmark 后再查看；可用 `sisyphus status` 查看桥接状态。");
        return Ok(());
    }

    println!(
        "{:<20} {:>10} {:>10} {:>10}",
        "Benchmark", "Duration", "Timestamp", "Success"
    );
    println!("{}", "-".repeat(55));
    for b in benchmarks {
        println!(
            "{:<20} {:>9.2}s {:>10} {:>10}",
            b.name,
            b.duration_secs,
            b.timestamp,
            if b.success { "✅" } else { "❌" }
        );
    }
    Ok(())
}

/// Run status.
pub async fn run_status(client: &mut BridgeClient) -> Result<()> {
    println!("=== Sisyphus Bridge Status ===");

    // 健康
    let health = client.health_check().await?;
    println!("Health: {} (Version: {})", health.status, health.version);
    println!(
        "提示: HTTP 侧可用 `sisyphus doctor --http <http_addr>` 或 `./scripts/single-node-readiness-check.sh` 检查。"
    );

    // 熔断器
    println!("\n--- Circuit Breakers ---");
    let breakers = client.get_breaker_metrics(None).await?;
    if breakers.is_empty() {
        println!("No breaker metrics available.");
    } else {
        println!(
            "{:<20} {:>10} {:>10} {:>10}",
            "Model", "State", "Failures", "Successes"
        );
        println!("{}", "-".repeat(55));
        for b in breakers {
            println!(
                "{:<20} {:>10} {:>10} {:>10}",
                b.model_name, b.state, b.failure_count, b.success_count
            );
        }
    }

    // 最近基准
    println!("\n--- Recent Benchmarks ---");
    let benchmarks = client.list_benchmarks().await?;
    if benchmarks.is_empty() {
        println!("  (none)");
    }
    for b in benchmarks.iter().take(5) {
        println!(
            "  {} — {:.2}s ({})",
            b.name,
            b.duration_secs,
            if b.success { "PASS" } else { "FAIL" }
        );
    }

    Ok(())
}

/// Run list agents.
pub async fn run_list_agents(client: &mut BridgeClient) -> Result<()> {
    let agents = client.list_agents().await?;
    if agents.is_empty() {
        println!("No agents registered.");
        println!("提示: 确认 bridge 已加载代理；可用 `sisyphus doctor` 检查连接与 SOP/服务状态。");
        return Ok(());
    }

    println!("{:<15} {:<20} Description", "ID", "Name");
    println!("{}", "-".repeat(60));
    for a in agents {
        println!(
            "{:<15} {:<20} {}",
            a.agent_id, a.display_name, a.description
        );
    }
    Ok(())
}

/// Run list tools.
pub async fn run_list_tools(client: &mut BridgeClient) -> Result<()> {
    let tools = client.list_tools().await?;
    if tools.is_empty() {
        println!("No tools registered.");
        println!("提示: 检查 bridge 工具注册与 MCP 配置；本地工具/MCP 管理可用 `sisyphus tool --help`、`sisyphus mcp --help`。");
        return Ok(());
    }

    println!("{:<20} {:<20} {:<10}", "ID", "Name", "Source");
    println!("{}", "-".repeat(55));
    for t in tools {
        println!("{:<20} {:<20} {:<10}", t.tool_id, t.name, t.source);
    }
    Ok(())
}

/// Run list skills.
pub async fn run_list_skills(client: &mut BridgeClient) -> Result<()> {
    let skills = client.list_skills().await?;
    if skills.is_empty() {
        println!("No skills installed.");
        println!(
            "提示: 这是 bridge 侧技能列表；本地技能管理可用 `sisyphus skill list/install/show`。"
        );
        return Ok(());
    }

    println!("{:<20} {:<30} Source", "Name", "Description");
    println!("{}", "-".repeat(65));
    for s in skills {
        println!("{:<20} {:<30} {}", s.name, s.description, s.author);
    }
    Ok(())
}

/// Run exec sop.
pub async fn run_exec_sop(client: &mut BridgeClient, sop_id: &str) -> Result<()> {
    println!("Executing SOP: {sop_id}...");
    let result = client.execute_sop(sop_id).await?;
    let rendered = if let Ok(v) = serde_json::from_str::<serde_json::Value>(&result) {
        serde_json::to_string_pretty(&v).unwrap_or(result.clone())
    } else {
        result
    };
    println!("Result:\n{rendered}");
    Ok(())
}

/// Run decompose goal.
pub async fn run_decompose_goal(client: &mut BridgeClient, goal: &str) -> Result<()> {
    println!("Decomposing goal: {goal}...");
    // 为 CLI 命令使用虚拟 goal_id 与空上下文
    let tasks = client.decompose_goal("cli-goal", goal, "").await?;
    if tasks.is_empty() {
        println!("No tasks generated.");
        println!("提示: 目标分解可能依赖 LLM/规划能力配置；可先用 `sisyphus sop list` 与模板化 SOP 验证执行链路。");
        return Ok(());
    }

    println!("\nGenerated Task DAG:");
    for (i, t) in tasks.iter().enumerate() {
        println!(
            "{i}. [{}] Agent: {} | Tool: {} | Deps: {:?}",
            t.id, t.agent_id, t.tool_id, t.depends_on
        );
    }
    Ok(())
}

/// Run learning insights.
pub async fn run_learning_insights(
    client: &mut BridgeClient,
    strategy_filter: Option<&str>,
) -> Result<()> {
    let response = client.get_learning_insights(strategy_filter).await?;

    println!("=== Meta-Learning Insights ===");
    println!("Total Samples: {}", response.total_samples);
    println!(
        "Overall Success Rate: {:.1}%",
        response.overall_success_rate * 100.0
    );

    if response.strategies.is_empty() {
        println!("\nNo strategy data available yet.");
        println!("提示: 在执行更多任务后再查看；可结合 `sisyphus status` 与执行 trace 观察效果。");
        return Ok(());
    }

    println!(
        "\n{:<15} {:>10} {:>10} {:>15} {:>12}",
        "Strategy", "Uses", "Success", "Avg Duration", "Rate"
    );
    println!("{}", "-".repeat(65));
    for s in &response.strategies {
        println!(
            "{:<15} {:>10} {:>10} {:>12.2}s {:>10.1}%",
            s.strategy,
            s.total_uses,
            s.successes,
            s.avg_duration_secs,
            s.success_rate * 100.0,
        );
    }
    Ok(())
}
