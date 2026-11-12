use std::sync::Arc;

use anyhow::Result;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn handle_dev(
    goal: &str,
    context: &Option<String>,
    parallel: bool,
    resume: bool,
    timeout_secs: u64,
    verify_cmd: &Option<String>,
    verify_fix_attempts: usize,
    sisyphus_cfg: &sisyphus_types::Config,
) -> Result<()> {
    println!("🧠 Sisyphus developer agent");
    println!("Goal: {goal}");
    if let Some(ctx) = context {
        println!("Context: {ctx}");
    }
    println!();

    let config = Arc::new(std::sync::RwLock::new(sisyphus_cfg.clone()));
    let llm = Arc::new(sisyphus_cli::tools::llm::DynamicLlmClient::new(config));
    let mut developer = sisyphus_cli::tools::developer::Developer::new(llm);
    if let Some(cmd) = verify_cmd {
        println!("Objective verification: `{cmd}` (fix attempts: {verify_fix_attempts})");
        developer = developer
            .with_final_verification(cmd.clone())
            .with_final_fix_attempts(verify_fix_attempts);
    }

    match tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs.max(30)),
        developer.develop(goal, context.as_deref().unwrap_or(""), parallel, resume),
    )
    .await
    {
        Ok(Ok(report)) => {
            println!("{}", sisyphus_cli::tools::developer::format_report(&report));
            match &report.final_verification {
                Some(gate) if !gate.passed => {
                    println!(
                        "❌ Objective verification failed after {} fix attempt(s): `{}`",
                        gate.fix_attempts, gate.command
                    );
                    // 自动化脚本以退出码判断成败：闸门失败必须是非零，
                    // 不能只打印一行 ❌ 然后返回 0。
                    anyhow::bail!("objective verification failed: `{}`", gate.command);
                },
                Some(gate) => {
                    println!(
                        "✅ Objective verification passed: `{}` ({} fix attempt(s))",
                        gate.command, gate.fix_attempts
                    );
                    if report.overall_status
                        != sisyphus_cli::tools::developer::DevOverallStatus::Success
                    {
                        println!("⚠️  Some steps failed; see the per-step status above.");
                    }
                },
                None if report.overall_status
                    == sisyphus_cli::tools::developer::DevOverallStatus::Success =>
                {
                    println!("✅ Development completed successfully.");
                },
                None => println!("⚠️  Development completed with issues."),
            }
        },
        Ok(Err(e)) => {
            eprintln!("❌ Development failed: {e:#}");
        },
        Err(_) => {
            eprintln!("❌ Development timed out after {timeout_secs}s.");
        },
    }
    Ok(())
}
