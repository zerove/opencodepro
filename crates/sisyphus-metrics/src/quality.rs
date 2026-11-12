//! 任务级质量指标（T4.1）。
//!
//! 每个任务的一次「验证 → 修复 → 验证」执行落一条结构化记录
//! （[`TaskRecord`]），本模块负责聚合成质量指标：
//!
//! - **任务成功率**：`pass / (pass + fail)`；`blocked` 单独计数（环境缺失等）；
//! - **任务修复率**：初次验证失败且最终通过的任务数 / 初次验证失败的任务数；
//! - **任务回归率**：修复引入回归的任务数 / 执行过修复的任务数。
//!
//! 口径与 `docs/plans/single-node-quality-metrics.md` 一致；无样本时返回
//! `None`（不伪造 0 值）。

use serde::{Deserialize, Serialize};

/// 任务终态。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// 达成验收标准。
    Pass,
    /// 未达成验收标准。
    Fail,
    /// 环境缺失导致无法判定（如缺 LLM 凭据）。
    Blocked,
}

/// 单个任务的一次执行记录。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskRecord {
    /// 任务标识。
    pub task_id: String,
    /// 场景分类（如 `diagnose-fix` / `quality-gate` / `feature`）。
    pub scenario: String,
    /// 终态。
    pub status: TaskStatus,
    /// 首轮验证是否通过（`false` 表示需要修复）。
    pub initial_verify_passed: bool,
    /// 最终验证是否通过。
    pub final_verify_passed: bool,
    /// 修复轮数。
    pub fix_attempts: u32,
    /// 修复是否引入回归。
    pub regressed: bool,
    /// 任务耗时（毫秒）。
    pub duration_ms: u64,
    /// 输入（prompt）token 数；provider 未上报时为 `None`（未知 ≠ 0）。
    #[serde(default)]
    pub prompt_tokens: Option<u64>,
    /// 输出（completion）token 数；provider 未上报时为 `None`。
    #[serde(default)]
    pub completion_tokens: Option<u64>,
    /// 记录时间（RFC3339）。
    pub recorded_at: String,
}

/// 聚合出的质量指标。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QualityMetrics {
    /// 记录总数。
    pub total: usize,
    /// 通过数。
    pub passed: usize,
    /// 失败数。
    pub failed: usize,
    /// 无法判定数。
    pub blocked: usize,
    /// 任务成功率（`pass / (pass + fail)`；无有效样本时 `None`）。
    pub success_rate: Option<f64>,
    /// 任务修复率（初次失败且最终通过 / 初次失败；无初次失败样本时 `None`）。
    pub fix_rate: Option<f64>,
    /// 任务回归率（引入回归 / 执行过修复；无修复样本时 `None`）。
    pub regression_rate: Option<f64>,
    /// 平均单任务耗时（毫秒；无样本时 `None`）。
    pub mean_duration_ms: Option<f64>,
    /// 平均单任务输入 token（仅有 token 样本时 `None`）。
    pub mean_prompt_tokens: Option<f64>,
    /// 平均单任务输出 token（仅有 token 样本时 `None`）。
    pub mean_completion_tokens: Option<f64>,
    /// 平均单任务 token 总量（输入+输出；仅有 token 样本时 `None`）。
    pub mean_total_tokens: Option<f64>,
}

/// 聚合任务记录。
pub fn aggregate(records: &[TaskRecord]) -> QualityMetrics {
    let total = records.len();
    let passed = records.iter().filter(|r| r.status == TaskStatus::Pass).count();
    let failed = records.iter().filter(|r| r.status == TaskStatus::Fail).count();
    let blocked = records.iter().filter(|r| r.status == TaskStatus::Blocked).count();

    let judged = passed + failed;
    let success_rate = (judged > 0).then(|| passed as f64 / judged as f64);

    let initially_failed: Vec<&TaskRecord> = records
        .iter()
        .filter(|r| !r.initial_verify_passed && r.status != TaskStatus::Blocked)
        .collect();
    let fixed = initially_failed
        .iter()
        .filter(|r| r.final_verify_passed && r.status == TaskStatus::Pass)
        .count();
    let fix_rate =
        (!initially_failed.is_empty()).then(|| fixed as f64 / initially_failed.len() as f64);

    let repaired: Vec<&TaskRecord> =
        records.iter().filter(|r| r.fix_attempts > 0 || r.regressed).collect();
    let regressed = repaired.iter().filter(|r| r.regressed).count();
    let regression_rate = (!repaired.is_empty()).then(|| regressed as f64 / repaired.len() as f64);

    let mean_duration_ms = (total > 0).then(|| {
        let sum: u64 = records.iter().map(|r| r.duration_ms).sum();
        sum as f64 / total as f64
    });

    // token 维度只在「provider 真的上报过 usage」的记录上统计；
    // 没有 token 的记录不参与均值（否则会把它算成 0，拉低均值）。
    let token_samples: Vec<&TaskRecord> = records
        .iter()
        .filter(|r| r.prompt_tokens.is_some() || r.completion_tokens.is_some())
        .collect();
    let mean_prompt_tokens = (!token_samples.is_empty()).then(|| {
        let sum: u64 = token_samples.iter().map(|r| r.prompt_tokens.unwrap_or(0)).sum();
        sum as f64 / token_samples.len() as f64
    });
    let mean_completion_tokens = (!token_samples.is_empty()).then(|| {
        let sum: u64 = token_samples.iter().map(|r| r.completion_tokens.unwrap_or(0)).sum();
        sum as f64 / token_samples.len() as f64
    });
    let mean_total_tokens = (!token_samples.is_empty()).then(|| {
        let sum: u64 = token_samples
            .iter()
            .map(|r| r.prompt_tokens.unwrap_or(0) + r.completion_tokens.unwrap_or(0))
            .sum();
        sum as f64 / token_samples.len() as f64
    });

    QualityMetrics {
        total,
        passed,
        failed,
        blocked,
        success_rate,
        fix_rate,
        regression_rate,
        mean_duration_ms,
        mean_prompt_tokens,
        mean_completion_tokens,
        mean_total_tokens,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(
        task_id: &str,
        status: TaskStatus,
        initial_passed: bool,
        final_passed: bool,
        fix_attempts: u32,
        regressed: bool,
    ) -> TaskRecord {
        TaskRecord {
            task_id: task_id.to_string(),
            scenario: "diagnose-fix".to_string(),
            status,
            initial_verify_passed: initial_passed,
            final_verify_passed: final_passed,
            fix_attempts,
            regressed,
            duration_ms: 100,
            prompt_tokens: None,
            completion_tokens: None,
            recorded_at: "2026-11-11T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn empty_records_yield_null_rates() {
        let metrics = aggregate(&[]);
        assert_eq!(metrics.total, 0);
        assert_eq!(metrics.success_rate, None);
        assert_eq!(metrics.fix_rate, None);
        assert_eq!(metrics.regression_rate, None);
        assert_eq!(metrics.mean_duration_ms, None);
    }

    #[test]
    fn token_metrics_are_null_without_token_samples() {
        let records = vec![record("t1", TaskStatus::Pass, true, true, 0, false)];
        let metrics = aggregate(&records);
        assert_eq!(metrics.mean_prompt_tokens, None);
        assert_eq!(metrics.mean_completion_tokens, None);
        assert_eq!(metrics.mean_total_tokens, None);
    }

    #[test]
    fn token_metrics_average_only_token_samples() {
        let mut with_tokens = record("t1", TaskStatus::Pass, true, true, 0, false);
        with_tokens.prompt_tokens = Some(100);
        with_tokens.completion_tokens = Some(50);
        let mut other = record("t2", TaskStatus::Pass, true, true, 0, false);
        other.prompt_tokens = Some(300);
        other.completion_tokens = Some(100);
        // 无 token 的记录不参与均值（否则会被当作 0 拉低均值）
        let no_tokens = record("t3", TaskStatus::Pass, true, true, 0, false);

        let metrics = aggregate(&[with_tokens, other, no_tokens]);
        assert_eq!(metrics.total, 3);
        assert_eq!(metrics.mean_prompt_tokens, Some(200.0));
        assert_eq!(metrics.mean_completion_tokens, Some(75.0));
        assert_eq!(metrics.mean_total_tokens, Some(275.0));
    }

    #[test]
    fn record_without_token_fields_still_deserializes() {
        // 旧记录（无 token 字段）必须仍可解析：缺失即「未知」，不是 0。
        let raw = r#"{
            "task_id": "old",
            "scenario": "diagnose-fix",
            "status": "pass",
            "initial_verify_passed": true,
            "final_verify_passed": true,
            "fix_attempts": 0,
            "regressed": false,
            "duration_ms": 188,
            "recorded_at": "2026-11-11T00:00:00Z"
        }"#;
        let parsed: TaskRecord = serde_json::from_str(raw).expect("旧记录应可解析");
        assert_eq!(parsed.prompt_tokens, None);
        assert_eq!(parsed.completion_tokens, None);
    }

    #[test]
    fn success_rate_excludes_blocked_tasks() {
        let records = vec![
            record("t1", TaskStatus::Pass, true, true, 0, false),
            record("t2", TaskStatus::Fail, false, false, 1, false),
            record("t3", TaskStatus::Blocked, false, false, 0, false),
        ];
        let metrics = aggregate(&records);
        assert_eq!(metrics.total, 3);
        assert_eq!(metrics.passed, 1);
        assert_eq!(metrics.failed, 1);
        assert_eq!(metrics.blocked, 1);
        assert_eq!(metrics.success_rate, Some(0.5));
    }

    #[test]
    fn fix_rate_counts_recovered_tasks() {
        let records = vec![
            record("t1", TaskStatus::Pass, false, true, 1, false),
            record("t2", TaskStatus::Fail, false, false, 2, false),
            record("t3", TaskStatus::Pass, true, true, 0, false),
        ];
        let metrics = aggregate(&records);
        // 初次失败 2 个，其中 1 个最终通过
        assert_eq!(metrics.fix_rate, Some(0.5));
    }

    #[test]
    fn regression_rate_counts_regressed_repairs() {
        let records = vec![
            record("t1", TaskStatus::Pass, false, true, 1, true),
            record("t2", TaskStatus::Pass, false, true, 1, false),
        ];
        let metrics = aggregate(&records);
        assert_eq!(metrics.regression_rate, Some(0.5));
    }

    #[test]
    fn mean_duration_is_computed_over_all_records() {
        let mut a = record("t1", TaskStatus::Pass, true, true, 0, false);
        a.duration_ms = 200;
        let mut b = record("t2", TaskStatus::Pass, true, true, 0, false);
        b.duration_ms = 400;
        let metrics = aggregate(&[a, b]);
        assert_eq!(metrics.mean_duration_ms, Some(300.0));
    }

    #[test]
    fn records_round_trip_through_json() {
        let record = record("t1", TaskStatus::Pass, true, true, 0, false);
        let json = serde_json::to_string(&record).expect("序列化");
        let parsed: TaskRecord = serde_json::from_str(&json).expect("反序列化");
        assert_eq!(parsed, record);
        assert!(json.contains("\"status\":\"pass\""));
    }
}
