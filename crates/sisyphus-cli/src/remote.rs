use crate::bridge::BridgeClient;
use anyhow::Result;

/// Run benchmarks.
pub async fn run_benchmarks(client: &mut BridgeClient) -> Result<()> {
    let benchmarks = client.list_benchmarks().await?;
    if benchmarks.is_empty() {
        println!("No benchmarks found.");
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
    println!("Result:\n{result}");
    Ok(())
}

/// Run decompose goal.
pub async fn run_decompose_goal(client: &mut BridgeClient, goal: &str) -> Result<()> {
    println!("Decomposing goal: {goal}...");
    // 为 CLI 命令使用虚拟 goal_id 与空上下文
    let tasks = client.decompose_goal("cli-goal", goal, "").await?;
    if tasks.is_empty() {
        println!("No tasks generated.");
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
