use std::sync::Arc;

use anyhow::Result;

pub(crate) async fn handle_dev(
    goal: &str,
    context: &Option<String>,
    parallel: bool,
    resume: bool,
    sisyphus_cfg: &sisyphus_types::Config,
) -> Result<()> {
    println!("🧠 Sisyphus developer agent");
    println!("Goal: {goal}");
    if let Some(ctx) = context {
        println!("Context: {ctx}");
    }
    println!();

    let config = Arc::new(std::sync::RwLock::new(sisyphus_cfg.clone()));
    let llm = Arc::new(sisyphus_tools::llm::DynamicLlmClient::new(config));
    let developer = sisyphus_tools::developer::Developer::new(llm);

    match tokio::time::timeout(
        std::time::Duration::from_secs(300),
        developer.develop(goal, context.as_deref().unwrap_or(""), parallel, resume),
    )
    .await
    {
        Ok(Ok(report)) => {
            println!("{}", sisyphus_tools::developer::format_report(&report));
            if report.overall_status
                == sisyphus_tools::developer::DevOverallStatus::Success
            {
                println!("✅ Development completed successfully.");
            } else {
                println!("⚠️  Development completed with issues.");
            }
        }
        Ok(Err(e)) => {
            eprintln!("❌ Development failed: {e:#}");
        }
        Err(_) => {
            eprintln!("❌ Development timed out after 5 minutes.");
        }
    }
    Ok(())
}
