//! `sisyphus verify`：有界验证-修复循环的 CLI 入口。
//!
//! 复用 `sisyphus_tools_llm_tools::diagnose::verify_fix_loop_with`：
//! 每轮先验证（check + 可选 lint），未达门禁且仍有轮次时执行一次修复，
//! 直到通过或用尽轮次。轮数上限保证终止。

use std::path::PathBuf;

use anyhow::{Context, Result};

use sisyphus_cli::build::resolve_backend;
use sisyphus_cli::tools::diagnose::{VerifyFixOptions, VerifyFixReport, VerifyRound};

/// 执行验证-修复循环。
#[allow(clippy::too_many_arguments)]
pub fn handle_verify(
    lang: Option<String>,
    package: Option<String>,
    all: bool,
    no_lint: bool,
    fix: bool,
    lint_fixes: bool,
    attempts: usize,
    max_warnings: Option<usize>,
    json: bool,
    record: Option<PathBuf>,
    cwd: Option<PathBuf>,
) -> Result<()> {
    let dir = cwd.as_deref();
    let backend = resolve_backend(lang.as_deref(), dir)?;

    let options = VerifyFixOptions {
        pkg: package.as_deref(),
        all,
        no_lint,
        fix_lint: lint_fixes,
        max_warnings,
        max_rounds: if fix { attempts } else { 1 },
    };
    let report =
        sisyphus_cli::tools::diagnose::verify_fix_loop_with(backend.as_ref(), &options, dir)?;

    if json {
        print_json(backend.name(), &report, max_warnings)?;
    } else {
        print_human(backend.name(), &report, max_warnings);
    }

    // T4.1：可选落一条任务级质量记录，供 `sisyphus metrics quality` 聚合
    if let Some(path) = &record {
        write_task_record(path, &report)?;
    }

    if !report.passed {
        let last = report.last_report.as_ref();
        let errors = last.map(|r| r.total_errors).unwrap_or(0);
        let warnings = last.map(|r| r.total_warnings).unwrap_or(0);
        anyhow::bail!(
            "verify 未通过：{} 轮后仍有 {errors} 个错误、{warnings} 个警告{}",
            report.rounds_used(),
            match max_warnings {
                Some(limit) => format!("（警告门禁：≤{limit}）"),
                None => String::new(),
            }
        );
    }
    Ok(())
}

fn print_human(backend: &str, report: &VerifyFixReport, max_warnings: Option<usize>) {
    println!(
        "=== Verify (backend: {backend}, rounds: {}/{}) ===",
        report.rounds_used(),
        report.max_rounds
    );
    if let Some(limit) = max_warnings {
        println!("warning gate: ≤{limit}");
    }
    for round in &report.rounds {
        println!("{}", round_line(round));
    }
    println!(
        "{}",
        if report.passed {
            format!(
                "✅ verify passed in {}ms ({} round(s), {} fix attempt(s))",
                report.total_duration_ms,
                report.rounds_used(),
                report.fix_attempts()
            )
        } else {
            format!(
                "❌ verify failed after {} round(s), {} fix attempt(s), {}ms",
                report.rounds_used(),
                report.fix_attempts(),
                report.total_duration_ms
            )
        }
    );
    if !report.passed {
        if let Some(last) = report.last_report.as_ref() {
            for diag in last.all_diagnostics.iter().take(10) {
                let loc = match (&diag.file, diag.line) {
                    (Some(f), Some(line)) => format!("{f}:{line}"),
                    (Some(f), None) => f.clone(),
                    _ => "<unknown>".to_string(),
                };
                let code = diag.code.as_deref().unwrap_or("");
                println!("  [{}] {} {} {}", diag.level, code, loc, diag.message);
            }
            if last.all_diagnostics.len() > 10 {
                println!("  ... and {} more", last.all_diagnostics.len() - 10);
            }
        }
    }
}

fn round_line(round: &VerifyRound) -> String {
    let verdict = if round.errors == 0 && round.warnings == 0 {
        "✅"
    } else {
        "❌"
    };
    let mut line = format!(
        "{verdict} round {}: {} error(s), {} warning(s) in {}ms",
        round.round, round.errors, round.warnings, round.duration_ms
    );
    if let Some(fix) = round.fix.as_ref() {
        let state = if fix.success { "✅" } else { "❌" };
        line.push_str(&format!(" → fix {state} in {}ms", fix.duration_ms));
        if let Some(message) = fix.message.as_deref() {
            line.push_str(&format!(" ({message})"));
        }
    }
    line
}

fn print_json(backend: &str, report: &VerifyFixReport, max_warnings: Option<usize>) -> Result<()> {
    let rounds: Vec<serde_json::Value> = report
        .rounds
        .iter()
        .map(|r| {
            serde_json::json!({
                "round": r.round,
                "errors": r.errors,
                "warnings": r.warnings,
                "duration_ms": r.duration_ms,
                "fix": r.fix.as_ref().map(|f| serde_json::json!({
                    "success": f.success,
                    "errors": f.errors,
                    "duration_ms": f.duration_ms,
                    "message": f.message,
                })),
            })
        })
        .collect();
    let payload = serde_json::json!({
        "backend": backend,
        "passed": report.passed,
        "rounds_used": report.rounds_used(),
        "max_rounds": report.max_rounds,
        "fix_attempts": report.fix_attempts(),
        "max_warnings": max_warnings,
        "total_duration_ms": report.total_duration_ms,
        "rounds": rounds,
        "final_errors": report.last_report.as_ref().map(|r| r.total_errors),
        "final_warnings": report.last_report.as_ref().map(|r| r.total_warnings),
    });
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

/// 把一次 verify 执行落成任务级质量记录（T4.1）。
///
/// 记录口径与 `docs/plans/single-node-quality-metrics.md` 一致：
/// 首轮是否通过、最终是否通过、修复轮数、是否回归、耗时。
fn write_task_record(path: &PathBuf, report: &VerifyFixReport) -> Result<()> {
    use sisyphus_metrics::quality::{TaskRecord, TaskStatus};

    let first_round_passed = report
        .rounds
        .first()
        .map(|round| round.errors == 0 && round.fix.is_none())
        .unwrap_or(false);
    let status = if report.passed {
        TaskStatus::Pass
    } else {
        TaskStatus::Fail
    };

    let task_id = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "verify".to_string());

    let record = TaskRecord {
        task_id,
        scenario: "diagnose-fix".to_string(),
        status,
        initial_verify_passed: first_round_passed,
        final_verify_passed: report.passed,
        fix_attempts: report.fix_attempts() as u32,
        // verify 循环不检测「修好目标但破坏其它检查」，因此恒为 false；
        // 回归判定需要跨任务对比，由聚合阶段负责。
        regressed: false,
        duration_ms: report.total_duration_ms as u64,
        // verify 链路目前拿不到 provider 上报的 usage（后端抽象未暴露），
        // 留空表示「未知」，不伪造 0。
        prompt_tokens: None,
        completion_tokens: None,
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
    use sisyphus_cli::tools::diagnose::FixAttempt;

    fn round(fix: Option<FixAttempt>) -> VerifyRound {
        VerifyRound {
            round: 1,
            errors: 2,
            warnings: 1,
            duration_ms: 42,
            fix,
        }
    }

    #[test]
    fn round_line_without_fix() {
        let line = round_line(&round(None));
        assert!(line.starts_with("❌ round 1: 2 error(s), 1 warning(s) in 42ms"));
        assert!(!line.contains("fix"));
    }

    #[test]
    fn round_line_with_successful_fix() {
        let line = round_line(&round(Some(FixAttempt {
            success: true,
            errors: 0,
            duration_ms: 900,
            message: None,
        })));
        assert!(line.contains("fix ✅ in 900ms"));
    }

    #[test]
    fn round_line_reports_fix_failure_message() {
        let line = round_line(&round(Some(FixAttempt {
            success: false,
            errors: 0,
            duration_ms: 5,
            message: Some("cargo fix 不可用".to_string()),
        })));
        assert!(line.contains("fix ❌ in 5ms"));
        assert!(line.contains("cargo fix 不可用"));
    }
}
