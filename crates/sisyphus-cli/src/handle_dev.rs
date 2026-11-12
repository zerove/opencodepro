use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use sisyphus_cli::dev_approval::CliDevApprover;
use sisyphus_cli::tools::developer::DevelopmentReport;
use sisyphus_metrics::quality::{TaskRecord, TaskStatus};
use sisyphus_types::{LlmClient, LlmTokenUsage};

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
    yes: bool,
    cli_perms: &sisyphus_cli::permission::CliPermission,
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
    // 保留一份 Arc：闭环结束后要读 provider 上报的 token 用量（质量指标的
    // token 成本维度）。`Developer` 只拿到 trait object，读不到用量。
    let llm = Arc::new(sisyphus_cli::tools::llm::DynamicLlmClient::new(config));
    let mut developer = sisyphus_cli::tools::developer::Developer::new(llm.clone());
    // 权限闸：计划里的 RunCommand 是任意 shell，必须走统一权限体系
    // （与 TUI 的 dev 路径同一口径）。非交互环境默认拒绝 —— 自动化必须显式
    // `--yes`，否则模型可以静默执行任意命令。
    developer = developer.with_approval_handler(Arc::new(CliDevApprover::from_env(
        Arc::new(cli_perms.clone()),
        yes,
    )));
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
            // provider 上报了 usage 才有值；否则为「未知」而不是 0。
            let token_usage = llm.token_usage();
            println!("{}", sisyphus_cli::tools::developer::format_report(&report));

            // 客观闸门只检查被测行为，检查不到「顺手毁掉别的代码」→
            // 额外检测公开 API 删除（回归信号）。
            let deleted_apis = detect_deleted_public_apis();
            let regressed = !deleted_apis.is_empty();
            if regressed {
                eprintln!(
                    "⚠️  检测到 {} 处公开 API 被删除（回归风险，闸门不一定能发现）：",
                    deleted_apis.len()
                );
                for line in deleted_apis.iter().take(5) {
                    eprintln!("    - {line}");
                }
                if deleted_apis.len() > 5 {
                    eprintln!("    … 共 {} 处", deleted_apis.len());
                }
            }

            // T4.1：闸门失败也要落记录（失败样本是成功率的分母），
            // 因此写在 bail 之前。
            if let Some(path) = record {
                write_dev_task_record(
                    path,
                    scenario,
                    &report,
                    duration_ms,
                    token_usage,
                    regressed,
                )?;
                println!("📝 Task record: {}", path.display());
            }

            // provider 静默截断 prompt 时必须提示：模型可能根本没看到上下文
            // 开头（本地 ollama 的 num_ctx 较小时会保留尾部、丢掉开头）。
            if llm.prompt_truncated() {
                eprintln!(
                    "⚠️  provider 截断了 prompt（模型可能没有看到上下文开头）。\n    建议：缩短 --context，或换用更长上下文的模型。"
                );
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
            if llm.prompt_truncated() {
                eprintln!(
                    "⚠️  provider 截断了 prompt（模型可能没有看到上下文开头，目标也可能被裁掉）。\n    建议：缩短 --context，或换用更长上下文的模型。"
                );
            }
            // 自动化脚本以退出码判断成败：规划/执行失败同样必须是非零。
            // （闸门失败已单独 bail；这里覆盖「连计划都拿不到」的路径。）
            anyhow::bail!("development failed: {e:#}");
        },
        Err(_) => {
            eprintln!("❌ Development timed out after {timeout_secs}s.");
            anyhow::bail!("development timed out after {timeout_secs}s");
        },
    }
    Ok(())
}

/// 从 `git diff -U0` 输出里找出**被删除的公开 API 行**。
///
/// 客观闸门（`--verify-cmd`）只检查被测行为，**检查不到「agent 顺手毁掉了
/// 别的代码」**：实测 3B 模型用 `CreateFile` 覆盖长文件、删掉 400 个
/// `pub fn helper_N`，而 `cargo test` 依然通过（测试只用到新函数）。
/// 删除公开 API 是回归的强信号，这里把它识别出来。
fn parse_deleted_public_apis(diff: &str) -> Vec<String> {
    diff.lines()
        .filter_map(|line| {
            let rest = line.strip_prefix('-')?;
            if rest.starts_with('-') {
                return None; // 文件头 `--- a/...`
            }
            let trimmed = rest.trim_start();
            let is_public_api = [
                "pub fn ",
                "pub struct ",
                "pub enum ",
                "pub trait ",
                "pub const ",
                "pub static ",
                "pub type ",
                "pub mod ",
                "pub union ",
            ]
            .iter()
            .any(|prefix| trimmed.starts_with(prefix));
            is_public_api.then(|| trimmed.to_string())
        })
        .collect()
}

/// 用 git diff 检测本次闭环是否删除了公开 API。
///
/// 非 git 仓库或没有 diff 时返回空（不猜、不伪造）。
fn detect_deleted_public_apis() -> Vec<String> {
    let out = sisyphus_tools_core::process::run_command_blocking(
        "git",
        ["diff", "-U0", "--no-color", "--", "."],
        sisyphus_tools_core::process::CommandOpts::default(),
    );
    match out {
        Ok(o) if o.exit_code == 0 => parse_deleted_public_apis(&o.stdout),
        _ => Vec::new(),
    }
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
/// - `regressed`：本次闭环是否**删除了公开 API**（闸门检查不到这类破坏，
///   实测模型覆盖长文件删掉 400 个 `pub fn` 而闸门仍 PASS）。
fn write_dev_task_record(
    path: &Path,
    scenario: &str,
    report: &DevelopmentReport,
    duration_ms: u64,
    token_usage: Option<LlmTokenUsage>,
    regressed: bool,
) -> Result<()> {
    let gate =
        report.final_verification.as_ref().context("--record 需要闸门结果，但报告中不存在")?;
    // 引擎版本：没它则指标无法归因到具体版本（见 quality::TaskRecord 的文档）。
    let revision = sisyphus_cli::engine_revision::engine_revision();

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
        duration_ms,
        // provider 未上报 usage 时留空（未知 ≠ 0），避免污染 token 均值。
        prompt_tokens: token_usage.map(|u| u.prompt_tokens),
        completion_tokens: token_usage.map(|u| u.completion_tokens),
        // 公开 API 被删除 = 回归信号（闸门检查不到这一类破坏）。
        regressed,
        // 产出这条记录的引擎版本。没有它，「成功率 0.857」无法归因到具体版本：
        // 不同版本的样本混在一个扁平目录里聚合，比率变化可能只是样本构成变了。
        engine_commit: revision.as_ref().map(|r| r.commit.clone()),
        engine_dirty: revision.as_ref().map(|r| r.dirty),
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

        write_dev_task_record(
            &path,
            "quality-gate",
            &report(Some(gate)),
            12_345,
            None,
            false,
        )
        .expect("写入");

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

        write_dev_task_record(&path, "diagnose-fix", &report(Some(gate)), 1, None, false)
            .expect("写入");
        let record: TaskRecord =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("读取")).expect("解析");
        assert_eq!(record.status, TaskStatus::Fail);
        assert!(!record.final_verify_passed);
    }

    #[test]
    fn record_carries_provider_token_usage() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("tokens.json");
        let gate = FinalVerification {
            command: "cargo test".to_string(),
            initial_passed: true,
            passed: true,
            fix_attempts: 0,
            output: "ok".to_string(),
        };
        let usage = LlmTokenUsage {
            prompt_tokens: 900,
            completion_tokens: 120,
            calls: 2,
        };

        write_dev_task_record(
            &path,
            "feature",
            &report(Some(gate)),
            10,
            Some(usage),
            false,
        )
        .expect("写入");

        let raw = std::fs::read_to_string(&path).expect("读取");
        let record: TaskRecord = serde_json::from_str(&raw).expect("解析");
        assert_eq!(record.prompt_tokens, Some(900));
        assert_eq!(record.completion_tokens, Some(120));
    }

    #[test]
    fn deleted_public_apis_are_detected_in_diff() {
        // 实测：3B 模型用 CreateFile 覆盖长文件、删掉 400 个 pub fn，
        // 而 `cargo test` 依然通过 —— 闸门检查不到这类破坏。
        let diff = "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -10,3 +10,1 @@\n-pub fn helper_1(v: i64) -> i64 { v + 1 }\n-pub fn helper_2(v: i64) -> i64 { v + 2 }\n-    let x = 1;\n+pub fn scale(v: i64, k: i64) -> i64 { v * k }\n";
        let deleted = parse_deleted_public_apis(diff);
        assert_eq!(deleted.len(), 2, "应只识别公开 API 删除: {deleted:?}");
        assert!(deleted[0].contains("helper_1"));
        assert!(deleted[1].contains("helper_2"));
    }

    #[test]
    fn non_api_deletions_are_ignored() {
        let diff = "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,2 +1,1 @@\n-    let x = 1;\n-// 注释\n+pub fn scale() {}\n";
        assert!(parse_deleted_public_apis(diff).is_empty());
    }

    #[test]
    fn record_without_gate_is_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("no-gate.json");
        assert!(write_dev_task_record(&path, "dev", &report(None), 1, None, false).is_err());
    }
}
