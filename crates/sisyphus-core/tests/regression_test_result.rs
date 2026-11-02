//! 集成测试：core RegressionTestResult 成功比率
//!
//! 覆盖 `sisyphus_core::self_programming_manager`：
//! - `RegressionTestResult::success_rate` 边界（0/全过/部分/无测试）
//! - `QualityReport`/`QualityAxis` 字段

use sisyphus_core::self_programming_manager::{QualityAxis, QualityReport, RegressionTestResult};

fn make_result(passed: u64, total: u64) -> RegressionTestResult {
    RegressionTestResult {
        tool_id: "t1".to_string(),
        harness_available: total > 0,
        unit_tests_passed: total == passed,
        total_tests: total,
        passed,
        failed: total.saturating_sub(passed),
        duration_ms: 100,
        failures: vec![],
    }
}

#[test]
fn success_rate_zero_tests_returns_one() {
    // No tests yet → return 1.0 (optimistic default)
    let r = make_result(0, 0);
    assert_eq!(r.success_rate(), 1.0);
}

#[test]
fn success_rate_all_passed() {
    let r = make_result(10, 10);
    assert!((r.success_rate() - 1.0).abs() < 1e-9);
}

#[test]
fn success_rate_none_passed() {
    let r = make_result(0, 10);
    assert_eq!(r.success_rate(), 0.0);
}

#[test]
fn success_rate_half() {
    let r = make_result(5, 10);
    assert!((r.success_rate() - 0.5).abs() < 1e-9);
}

#[test]
fn success_rate_quarter() {
    let r = make_result(25, 100);
    assert!((r.success_rate() - 0.25).abs() < 1e-9);
}

#[test]
fn success_rate_three_quarters() {
    let r = make_result(75, 100);
    assert!((r.success_rate() - 0.75).abs() < 1e-9);
}

#[test]
fn success_rate_one_of_one() {
    let r = make_result(1, 1);
    assert!((r.success_rate() - 1.0).abs() < 1e-9);
}

#[test]
fn result_field_access() {
    let r = make_result(5, 10);
    assert_eq!(r.tool_id, "t1");
    assert!(!r.unit_tests_passed);
    assert!(r.harness_available);
    assert_eq!(r.total_tests, 10);
    assert_eq!(r.passed, 5);
    assert_eq!(r.failed, 5);
    assert_eq!(r.duration_ms, 100);
    assert!(r.failures.is_empty());
}

#[test]
fn result_with_failures() {
    let r = RegressionTestResult {
        tool_id: "t".to_string(),
        harness_available: true,
        unit_tests_passed: false,
        total_tests: 3,
        passed: 1,
        failed: 2,
        duration_ms: 50,
        failures: vec!["test_a".to_string(), "test_b".to_string()],
    };
    assert_eq!(r.failures.len(), 2);
    assert!((r.success_rate() - 1.0 / 3.0).abs() < 1e-9);
}

#[test]
fn result_clone() {
    let r = make_result(3, 5);
    let r2 = r.clone();
    assert_eq!(r.tool_id, r2.tool_id);
    assert_eq!(r.passed, r2.passed);
    assert_eq!(r.total_tests, r2.total_tests);
    assert_eq!(r.failures, r2.failures);
}

#[test]
fn result_debug() {
    let r = make_result(1, 1);
    let s = format!("{r:?}");
    assert!(s.contains("RegressionTestResult"));
    assert!(s.contains("t1"));
}

#[test]
fn quality_axis_construction() {
    let a = QualityAxis {
        name: "correctness".to_string(),
        score: 0.9,
        weight: 0.5,
    };
    assert_eq!(a.name, "correctness");
    assert!((a.weight - 0.5).abs() < 1e-9);
    assert!((a.score - 0.9).abs() < 1e-9);
}

#[test]
fn quality_axis_clone() {
    let a = QualityAxis {
        name: "n".to_string(),
        score: 0.0,
        weight: 1.0,
    };
    let a2 = a.clone();
    assert_eq!(a.name, a2.name);
    assert_eq!(a.score, a2.score);
}

#[test]
fn quality_axis_debug() {
    let a = QualityAxis {
        name: "perf".to_string(),
        score: 0.8,
        weight: 0.3,
    };
    let s = format!("{a:?}");
    assert!(s.contains("QualityAxis"));
    assert!(s.contains("perf"));
}

#[test]
fn quality_report_construction() {
    let r = QualityReport {
        tool_id: "t".to_string(),
        overall_score: 0.5,
        axes: vec![
            QualityAxis {
                name: "a".to_string(),
                score: 1.0,
                weight: 0.5,
            },
            QualityAxis {
                name: "b".to_string(),
                score: 0.0,
                weight: 0.5,
            },
        ],
        unit_tests_passed: true,
        integration_tests_passed: false,
        blockers: vec![],
        recommendations: vec!["add tests".to_string()],
    };
    assert_eq!(r.tool_id, "t");
    assert_eq!(r.axes.len(), 2);
    assert!((r.overall_score - 0.5).abs() < 1e-9);
    assert!(r.unit_tests_passed);
    assert!(!r.integration_tests_passed);
    assert_eq!(r.recommendations.len(), 1);
}

#[test]
fn quality_report_empty_axes() {
    let r = QualityReport {
        tool_id: "t".to_string(),
        overall_score: 1.0,
        axes: vec![],
        unit_tests_passed: true,
        integration_tests_passed: true,
        blockers: vec![],
        recommendations: vec![],
    };
    assert!(r.axes.is_empty());
    assert_eq!(r.overall_score, 1.0);
    assert!(r.recommendations.is_empty());
}

#[test]
fn quality_report_clone() {
    let r = QualityReport {
        tool_id: "t".to_string(),
        overall_score: 0.5,
        axes: vec![],
        unit_tests_passed: true,
        integration_tests_passed: true,
        blockers: vec![],
        recommendations: vec!["r1".to_string()],
    };
    let r2 = r.clone();
    assert_eq!(r.tool_id, r2.tool_id);
    assert_eq!(r.axes.len(), r2.axes.len());
    assert_eq!(r.recommendations, r2.recommendations);
}

// ---------------------------------------------------------------------------
// Additional coverage
// ---------------------------------------------------------------------------

#[test]
fn success_rate_one_of_two() {
    let r = make_result(1, 2);
    assert!((r.success_rate() - 0.5).abs() < 1e-9);
}

#[test]
fn success_rate_three_of_seven() {
    let r = make_result(3, 7);
    let rate = r.success_rate();
    assert!((rate - 3.0 / 7.0).abs() < 1e-9);
}

#[test]
fn success_rate_max_u64() {
    let r = RegressionTestResult {
        tool_id: "t".into(),
        harness_available: true,
        unit_tests_passed: true,
        total_tests: u64::MAX,
        passed: u64::MAX - 1,
        failed: 1,
        duration_ms: 0,
        failures: vec!["f1".into()],
    };
    let rate = r.success_rate();
    assert!((rate - 1.0).abs() < 0.0001);
}

#[test]
fn result_with_zero_duration() {
    let r = RegressionTestResult {
        tool_id: "instant".into(),
        harness_available: true,
        unit_tests_passed: true,
        total_tests: 1,
        passed: 1,
        failed: 0,
        duration_ms: 0,
        failures: vec![],
    };
    assert_eq!(r.duration_ms, 0);
    assert!((r.success_rate() - 1.0).abs() < 1e-9);
}

#[test]
fn result_with_many_failures() {
    let r = RegressionTestResult {
        tool_id: "noisy".into(),
        harness_available: true,
        unit_tests_passed: false,
        total_tests: 100,
        passed: 50,
        failed: 50,
        duration_ms: 60000,
        failures: (0..50).map(|i| format!("failure_{}", i)).collect(),
    };
    assert_eq!(r.failures.len(), 50);
    assert!((r.success_rate() - 0.5).abs() < 1e-9);
}

#[test]
fn result_unicode_tool_id() {
    let r = RegressionTestResult {
        tool_id: "工具-α".into(),
        harness_available: true,
        unit_tests_passed: true,
        total_tests: 1,
        passed: 1,
        failed: 0,
        duration_ms: 100,
        failures: vec![],
    };
    assert_eq!(r.tool_id, "工具-α");
    let cloned = r.clone();
    assert_eq!(cloned.tool_id, "工具-α");
}

#[test]
fn quality_axis_unicode() {
    let a = QualityAxis {
        name: "性能".into(),
        score: 0.95,
        weight: 1.0,
    };
    assert_eq!(a.name, "性能");
}

#[test]
fn quality_axis_negative_score() {
    let a = QualityAxis {
        name: "p".into(),
        score: -1.0,
        weight: 1.0,
    };
    assert_eq!(a.score, -1.0);
}

#[test]
fn quality_axis_above_one_score() {
    let a = QualityAxis {
        name: "p".into(),
        score: 1.5,
        weight: 1.0,
    };
    assert_eq!(a.score, 1.5);
}

#[test]
fn quality_report_recommendations_unicode() {
    let r = QualityReport {
        tool_id: "t".into(),
        overall_score: 0.8,
        axes: vec![],
        unit_tests_passed: true,
        integration_tests_passed: true,
        blockers: vec![],
        recommendations: vec!["改进性能".into(), "增加文档".into()],
    };
    assert_eq!(r.recommendations.len(), 2);
    assert_eq!(r.recommendations[0], "改进性能");
}

#[test]
fn quality_report_empty_recommendations() {
    let r = QualityReport {
        tool_id: "t".into(),
        overall_score: 1.0,
        axes: vec![],
        unit_tests_passed: true,
        integration_tests_passed: true,
        blockers: vec![],
        recommendations: vec![],
    };
    assert!(r.recommendations.is_empty());
}
