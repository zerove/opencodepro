use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use sisyphus_cli::tools::developer::DevelopmentReport;
use sisyphus_metrics::quality::{TaskRecord, TaskStatus};

#[allow(clippy::too_many_arguments)]
pub(crate) async fn handle_dev(
    goal: &str,
    context: &Option<String>,
    parallel: bool,
    resume: bool,
    timeout_secs: u64,
    verify_cmd: &Option<String>,
    verify_fix_attempts: usize,
    record: &Option<PathBuf>,
    scenario: &str,
    sisyphus_cfg: &sisyphus_types::Config,
) -> Result<()> {
    ensure_recordable(record, verify_cmd)?;

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

    let started = Instant::now();
    match tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs.max(30)),
        developer.develop(goal, context.as_deref().unwrap_or(""), parallel, resume),
    )
    .await
    {
        Ok(Ok(report)) => {
            let duration_ms = started.elapsed().as_millis() as u64;
            println!("{}", sisyphus_cli::tools::developer::format_report(&report));

            // T4.1：闸门失败也要落记录（失败样本是成功率的分母），
            // 因此写在 bail 之前。
            if let Some(path) = record {
                write_dev_task_record(path, scenario, &report, duration_ms)?;
                println!("📝 Task record: {}", path.display());
            }

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

/// `--record` 必须配 `--verify-cmd`：没有客观闸门就没有「成败」可记录。
///
/// 记录一条没有判据的「成功」会直接污染质量指标，因此这里直接拒绝。
fn ensure_recordable(record: &Option<PathBuf>, verify_cmd: &Option<String>) -> Result<()> {
    if record.is_some() && verify_cmd.is_none() {
        anyhow::bail!("--record 需要同时指定 --verify-cmd（没有客观闸门无法判定任务成败）");
    }
    Ok(())
}

/// 把一次 `dev` 闭环落成任务级质量记录（T4.1）。
///
/// 口径与 `verify --record` 保持一致：
/// - `initial_verify_passed`：闸门**首轮**是否通过；
/// - `fix_attempts`：仅统计客观闸门的自动修复次数（步骤内自愈不计入）；
/// - `regressed`：dev 不检测跨任务回归，恒为 false（不伪造）。
fn write_dev_task_record(
    path: &Path,
    scenario: &str,
    report: &DevelopmentReport,
    duration_ms: u64,
) -> Result<()> {
    let gate =
        report.final_verification.as_ref().context("--record 需要闸门结果，但报告中不存在")?;

    let task_id = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "dev".to_string());

    let record = TaskRecord {
        task_id,
        scenario: scenario.to_string(),
        status: if gate.passed {
            TaskStatus::Pass
        } else {
            TaskStatus::Fail
        },
        initial_verify_passed: gate.initial_passed,
        final_verify_passed: gate.passed,
        fix_attempts: gate.fix_attempts as u32,
        regressed: false,
        duration_ms,
        recorded_at: chrono::Utc::now().to_rfc3339(),
    };

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("创建记录目录失败: {}", parent.display()))?;
        }
    }
    let json = serde_json::to_string_pretty(&record).context("序列化任务记录失败")?;
    std::fs::write(path, json).with_context(|| format!("写入任务记录失败: {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sisyphus_cli::tools::developer::{DevOverallStatus, FinalVerification};

    fn report(gate: Option<FinalVerification>) -> DevelopmentReport {
        DevelopmentReport {
            goal: "test goal".to_string(),
            overall_status: DevOverallStatus::Success,
            steps: Vec::new(),
            artifacts: Vec::new(),
            final_verification: gate,
        }
    }

    #[test]
    fn record_requires_objective_gate() {
        let record = Some(PathBuf::from("/tmp/x.json"));
        assert!(ensure_recordable(&record, &None).is_err());
        assert!(ensure_recordable(&record, &Some("cargo check".to_string())).is_ok());
        assert!(ensure_recordable(&None, &None).is_ok());
    }

    #[test]
    fn record_maps_gate_semantics() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("l3-s2-quality-gate.json");
        let gate = FinalVerification {
            command: "cargo clippy -- -D warnings".to_string(),
            initial_passed: false,
            passed: true,
            fix_attempts: 2,
            output: "clippy output".to_string(),
        };

        write_dev_task_record(&path, "quality-gate", &report(Some(gate)), 12_345).expect("写入");

        let raw = std::fs::read_to_string(&path).expect("读取");
        let record: TaskRecord = serde_json::from_str(&raw).expect("解析");
        assert_eq!(record.task_id, "l3-s2-quality-gate");
        assert_eq!(record.scenario, "quality-gate");
        assert_eq!(record.status, TaskStatus::Pass);
        assert!(!record.initial_verify_passed);
        assert!(record.final_verify_passed);
        assert_eq!(record.fix_attempts, 2);
        assert!(!record.regressed);
        assert_eq!(record.duration_ms, 12_345);
    }

    #[test]
    fn record_marks_failed_gate_as_fail() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("failed.json");
        let gate = FinalVerification {
            command: "cargo check".to_string(),
            initial_passed: false,
            passed: false,
            fix_attempts: 2,
            output: "error".to_string(),
        };

        write_dev_task_record(&path, "diagnose-fix", &report(Some(gate)), 1).expect("写入");
        let record: TaskRecord =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("读取")).expect("解析");
        assert_eq!(record.status, TaskStatus::Fail);
        assert!(!record.final_verify_passed);
    }

    #[test]
    fn record_without_gate_is_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("no-gate.json");
        assert!(write_dev_task_record(&path, "dev", &report(None), 1).is_err());
    }
}
