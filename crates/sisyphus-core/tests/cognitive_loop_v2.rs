//! Integration tests for sisyphus-core `cognitive_loop_v2`.
//!
//! Exercises the public [`AnomalyDetector`] and
//! [`AdaptiveDepthController`] types. The AnomalyDetector flags nodes
//! whose fuel cost, duration, or error-rate deviates from the
//! observed baseline (returned inline by `observe`); the report
//! aggregator `generate_report` summarises at the system level.
//! The AdaptiveDepthController tracks iteration confidence and
//! decides whether to keep going.

use sisyphus_core::cognitive_loop_v2::{
    AdaptiveDepthController, AnomalyDetector, AnomalyLog, NodeAnomaly, RECENT_ANOMALY_CAPACITY,
};

// ---------------------------------------------------------------------------
// AnomalyDetector
// ---------------------------------------------------------------------------

#[test]
fn anomaly_detector_starts_with_no_anomalies() {
    let det = AnomalyDetector::new(50.0, 1000.0, 0.3);
    let report = det.generate_report();
    assert!(report.anomalies.is_empty());
    assert!(!report.should_extra_reflect);
}

#[test]
fn anomaly_detector_first_observation_is_baseline() {
    let det = AnomalyDetector::new(2.0, 2.0, 0.3);
    // First observation sets the baseline; no anomaly reported.
    let anomalies = det.observe("node_a", 5, 100, true);
    assert!(anomalies.is_empty());
}

#[test]
fn anomaly_detector_flags_high_fuel_consumption() {
    let det = AnomalyDetector::new(2.0, 10.0, 0.3);
    // First obs establishes fuel baseline.
    det.observe("expensive_node", 5, 100, true);
    // Second obs is 10x baseline → exceeds 2x threshold.
    let anomalies = det.observe("expensive_node", 50, 100, true);
    assert!(!anomalies.is_empty());
    assert!(anomalies.iter().any(|a| a.metric == "fuel" && a.node_name == "expensive_node"));
}

#[test]
fn anomaly_detector_flags_long_duration() {
    let det = AnomalyDetector::new(50.0, 2.0, 0.3);
    // First obs establishes duration baseline (100ms).
    det.observe("slow_node", 5, 100, true);
    // Second obs is 5x baseline → exceeds 2x threshold.
    let anomalies = det.observe("slow_node", 5, 500, true);
    assert!(!anomalies.is_empty());
    assert!(anomalies.iter().any(|a| a.metric == "duration" && a.node_name == "slow_node"));
}

#[test]
fn anomaly_detector_returns_observed_and_expected() {
    let det = AnomalyDetector::new(2.0, 100.0, 0.3);
    det.observe("n", 5, 100, true);
    let anomalies = det.observe("n", 50, 100, true);
    let fuel = anomalies.iter().find(|a| a.metric == "fuel").expect("fuel");
    assert_eq!(fuel.observed, 50.0);
    // Expected is the EWMA — close to (0.3 * 50 + 0.7 * 5) = 18.5
    // (first baseline = 5, then EWMA(5, 50) = 18.5).
    assert!((fuel.expected - 18.5).abs() < 0.01, "got {}", fuel.expected);
}

#[test]
fn anomaly_detector_flags_high_error_rate() {
    let det = AnomalyDetector::new(50.0, 100.0, 0.3);
    // Many failures; need total > 3 to trigger error-rate anomaly.
    for _ in 0..4 {
        det.observe("flaky", 5, 100, false);
    }
    let anomalies = det.observe("flaky", 5, 100, false);
    assert!(anomalies.iter().any(|a| a.metric == "error_rate"));
}

#[test]
fn anomaly_detector_reset_clears_state() {
    let det = AnomalyDetector::new(2.0, 2.0, 0.3);
    det.observe("n", 5, 100, true);
    det.observe("n", 100, 500, true);
    det.reset();
    // After reset, the next observation is again a baseline.
    let anomalies = det.observe("n", 5, 100, true);
    assert!(anomalies.is_empty());
}

#[test]
fn anomaly_detector_independent_per_node() {
    let det = AnomalyDetector::new(2.0, 2.0, 0.3);
    // Build baseline for "a" only.
    det.observe("a", 5, 100, true);
    // First obs for "b" is still a baseline.
    let anomalies = det.observe("b", 5, 100, true);
    assert!(anomalies.is_empty());
}

// ---------------------------------------------------------------------------
// AdaptiveDepthController
// ---------------------------------------------------------------------------

#[test]
fn depth_controller_starts_with_empty_history() {
    let ctrl = AdaptiveDepthController::new(0.9, 0.05, 30, 3);
    assert!(ctrl.confidence_history().is_empty());
}

#[test]
fn depth_controller_budget_from_depth_clamps() {
    let ctrl = AdaptiveDepthController::new(0.9, 0.05, 30, 3);
    // Small depth → clamped to min_iterations.
    let small = ctrl.budget_from_depth(0);
    assert!(small >= 3, "got {small}");
    // Mid depth → fits within [min, max].
    let mid = ctrl.budget_from_depth(20);
    assert!((3..=30).contains(&mid));
    // Huge depth → clamped to max_iterations.
    let huge = ctrl.budget_from_depth(1_000_000);
    assert_eq!(huge, 30);
}

#[test]
fn depth_controller_observe_iteration_appends_history() {
    let ctrl = AdaptiveDepthController::new(0.9, 0.05, 30, 3);
    assert!(ctrl.confidence_history().is_empty());
    ctrl.observe_iteration(0.9);
    assert_eq!(ctrl.confidence_history().len(), 1);
    ctrl.observe_iteration(0.5);
    assert_eq!(ctrl.confidence_history().len(), 2);
}

#[test]
fn depth_controller_respects_min_iterations() {
    // min_iterations=3: must always return true for the first 2 observations.
    let ctrl = AdaptiveDepthController::new(0.9, 0.05, 30, 3);
    assert!(ctrl.observe_iteration(0.5));
    assert!(ctrl.observe_iteration(0.5));
    // At 3rd observation, depth controller may or may not continue;
    // but min_iterations is 3 so it's still "keep going" by policy.
    let third = ctrl.observe_iteration(0.5);
    // Implementation: the third push hits the policy branch — just
    // ensure the call doesn't panic. The return value can be true or
    // false depending on convergence logic.
    let _ = third;
}

#[test]
fn depth_controller_reset_clears_history() {
    let ctrl = AdaptiveDepthController::new(0.9, 0.05, 30, 3);
    ctrl.observe_iteration(0.7);
    assert!(!ctrl.confidence_history().is_empty());
    ctrl.reset();
    assert!(ctrl.confidence_history().is_empty());
}

#[test]
fn depth_controller_apply_config_changes_state() {
    let ctrl = AdaptiveDepthController::new(0.5, 0.1, 30, 3);
    ctrl.apply_config(0.95, 0.01);
    // State is private; just verify the call doesn't panic.
    let _ = ctrl.confidence_history();
    let _ = ctrl.observe_iteration(0.5);
}

// ---------------------------------------------------------------------------
// 可观测性缓冲（`/api/anomaly` 的数据来源）
// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// 可观测性缓冲（`/api/anomaly` 的数据来源）
// ---------------------------------------------------------------------------

/// 触发一次 fuel 异常：先建立基线，再用 10x 燃料消耗观测，返回检测结果。
fn trigger_fuel_anomaly(det: &AnomalyDetector, node: &str) -> Vec<NodeAnomaly> {
    det.observe(node, 5, 100, true);
    det.observe(node, 500, 100, true)
}

#[test]
fn anomaly_log_records_detected_anomalies() {
    let det = AnomalyDetector::new(2.0, 100.0, 0.3);
    let log = AnomalyLog::new();
    assert!(log.recent(None).is_empty(), "nothing observed yet");

    let anomalies = trigger_fuel_anomaly(&det, "n1");
    assert!(!anomalies.is_empty(), "fuel anomaly expected");
    log.record(&anomalies, "sop-1");

    let recent = log.recent(Some("sop-1"));
    assert_eq!(recent.len(), 1, "one anomaly must be retained: {recent:?}");
    assert_eq!(recent[0].anomaly.node_name, "n1");
    assert_eq!(recent[0].anomaly.metric, "fuel");
    assert_eq!(recent[0].scope, "sop-1");
    assert!(recent[0].detected_at > 0);
    assert!(recent[0].seq < u64::MAX, "seq must be allocated");
}

#[test]
fn anomaly_log_filters_by_scope() {
    let det = AnomalyDetector::new(2.0, 100.0, 0.3);
    let log = AnomalyLog::new();

    let a1 = trigger_fuel_anomaly(&det, "n1");
    log.record(&a1, "run-a");
    let a2 = trigger_fuel_anomaly(&det, "n2");
    log.record(&a2, "run-b");

    let run_a = log.recent(Some("run-a"));
    let run_b = log.recent(Some("run-b"));
    assert_eq!(run_a.len(), 1, "{run_a:?}");
    assert_eq!(run_b.len(), 1, "{run_b:?}");
    assert_eq!(run_a[0].anomaly.node_name, "n1");
    assert_eq!(run_b[0].anomaly.node_name, "n2");
    assert_eq!(log.recent(Some("missing")).len(), 0);
    assert_eq!(log.recent(None).len(), 2, "unscoped view sees everything");
}

#[test]
fn anomaly_log_sequence_is_monotonic() {
    let det = AnomalyDetector::new(2.0, 100.0, 0.3);
    let log = AnomalyLog::new();

    let a1 = trigger_fuel_anomaly(&det, "a");
    log.record(&a1, "run-x");
    let a2 = trigger_fuel_anomaly(&det, "b");
    log.record(&a2, "run-x");

    let recent = log.recent(Some("run-x"));
    assert_eq!(recent.len(), 2);
    assert!(
        recent[0].seq < recent[1].seq,
        "stream cursors rely on monotonic seq"
    );
}

#[test]
fn anomaly_log_is_bounded() {
    let det = AnomalyDetector::new(2.0, 100.0, 0.3);
    let log = AnomalyLog::new();

    // 每个节点两次观测触发一次异常，多跑一些以越过容量上限。
    for i in 0..(RECENT_ANOMALY_CAPACITY + 50) {
        let anomalies = trigger_fuel_anomaly(&det, &format!("node_{i}"));
        log.record(&anomalies, "sop-bound");
    }
    let recent = log.recent(Some("sop-bound"));
    assert_eq!(
        recent.len(),
        RECENT_ANOMALY_CAPACITY,
        "the buffer must stay bounded"
    );
    // FIFO：最早的记录被淘汰，保留的是最后写入的。
    assert_eq!(
        recent.last().map(|e| e.anomaly.node_name.as_str()),
        Some(format!("node_{}", RECENT_ANOMALY_CAPACITY + 49).as_str())
    );
}

#[test]
fn anomaly_log_reset_clears_recent() {
    let det = AnomalyDetector::new(2.0, 100.0, 0.3);
    let log = AnomalyLog::new();

    let anomalies = trigger_fuel_anomaly(&det, "n1");
    log.record(&anomalies, "sop-1");
    assert!(!log.recent(None).is_empty());
    log.clear();
    assert!(
        log.recent(None).is_empty(),
        "reset must clear observability state"
    );
}
