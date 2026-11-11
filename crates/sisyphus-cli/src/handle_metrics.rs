//! 质量指标聚合命令（T4.1）。
//!
//! 从任务记录目录（每个 `.json` 文件一条 [`TaskRecord`]）聚合：
//! 任务成功率 / 修复率 / 回归率 / 平均耗时。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sisyphus_metrics::quality::{aggregate, QualityMetrics, TaskRecord};

use crate::commands::MetricsCommands;

/// 处理 `sisyphus metrics ...` 子命令。
pub fn handle_metrics(sub: &MetricsCommands) -> Result<()> {
    match sub {
        MetricsCommands::Quality { records, json } => handle_quality(records, *json),
    }
}

/// 聚合任务记录并输出。
fn handle_quality(records_dir: &Path, json: bool) -> Result<()> {
    let metrics = aggregate_dir(records_dir)?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&metrics).context("序列化质量指标失败")?
        );
    } else {
        print_human(&metrics, records_dir);
    }
    Ok(())
}

/// 读取目录下全部任务记录并聚合。
///
/// 目录不存在或没有记录时返回「空指标」（各比率为 `None`），不伪造 0 值。
pub fn aggregate_dir(records_dir: &Path) -> Result<QualityMetrics> {
    let mut records: Vec<TaskRecord> = Vec::new();

    if records_dir.is_dir() {
        let entries = std::fs::read_dir(records_dir)
            .with_context(|| format!("读取任务记录目录失败: {}", records_dir.display()))?;
        let mut paths: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect();
        paths.sort();

        for path in paths {
            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("读取任务记录失败: {}", path.display()))?;
            let record: TaskRecord = serde_json::from_str(&content)
                .with_context(|| format!("解析任务记录失败: {}", path.display()))?;
            records.push(record);
        }
    }

    Ok(aggregate(&records))
}

fn print_human(metrics: &QualityMetrics, records_dir: &Path) {
    println!("=== 质量指标（记录目录: {}）===", records_dir.display());
    println!("记录总数: {}", metrics.total);
    println!(
        "通过 / 失败 / 无法判定: {} / {} / {}",
        metrics.passed, metrics.failed, metrics.blocked
    );
    match metrics.success_rate {
        Some(rate) => println!("任务成功率: {:.1}%", rate * 100.0),
        None => println!("任务成功率: 无样本"),
    }
    match metrics.fix_rate {
        Some(rate) => println!("任务修复率: {:.1}%", rate * 100.0),
        None => println!("任务修复率: 无样本"),
    }
    match metrics.regression_rate {
        Some(rate) => println!("任务回归率: {:.1}%", rate * 100.0),
        None => println!("任务回归率: 无样本"),
    }
    match metrics.mean_duration_ms {
        Some(duration) => println!("平均单任务耗时: {:.0}ms", duration),
        None => println!("平均单任务耗时: 无样本"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_record(dir: &Path, name: &str, status: &str, initial: bool, final_ok: bool, fix: u32) {
        let record = format!(
            r#"{{
              "task_id": "{name}",
              "scenario": "diagnose-fix",
              "status": "{status}",
              "initial_verify_passed": {initial},
              "final_verify_passed": {final_ok},
              "fix_attempts": {fix},
              "regressed": false,
              "duration_ms": 120,
              "recorded_at": "2026-11-11T00:00:00Z"
            }}"#
        );
        std::fs::write(dir.join(format!("{name}.json")), record).expect("写入记录");
    }

    #[test]
    fn missing_directory_yields_empty_metrics() {
        let metrics = aggregate_dir(Path::new("/tmp/opencode/does-not-exist-records")).unwrap();
        assert_eq!(metrics.total, 0);
        assert_eq!(metrics.success_rate, None);
        assert_eq!(metrics.fix_rate, None);
    }

    #[test]
    fn aggregates_records_from_directory() {
        let dir = std::env::temp_dir().join("sisyphus_metrics_records_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("创建目录");

        write_record(&dir, "t1", "pass", true, true, 0);
        write_record(&dir, "t2", "fail", false, false, 1);

        let metrics = aggregate_dir(&dir).unwrap();
        assert_eq!(metrics.total, 2);
        assert_eq!(metrics.passed, 1);
        assert_eq!(metrics.failed, 1);
        assert_eq!(metrics.success_rate, Some(0.5));
        assert_eq!(metrics.fix_rate, Some(0.0));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
