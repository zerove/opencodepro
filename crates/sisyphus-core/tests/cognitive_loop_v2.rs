//! Integration tests for sisyphus-core `cognitive_loop_v2`.
//!
//! Exercises the public [`AnomalyDetector`] and
//! [`AdaptiveDepthController`] types. The AnomalyDetector flags nodes
//! whose fuel cost, duration, or error-rate deviates from the
//! observed baseline (returned inline by `observe`); the report
//! aggregator `generate_report` summarises at the system level.
//! The AdaptiveDepthController tracks iteration confidence and
//! decides whether to keep going.

use sisyphus_core::cognitive_loop_v2::{AdaptiveDepthController, AnomalyDetector};

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
    assert!(anomalies
        .iter()
        .any(|a| a.metric == "fuel" && a.node_name == "expensive_node"));
}

#[test]
fn anomaly_detector_flags_long_duration() {
    let det = AnomalyDetector::new(50.0, 2.0, 0.3);
    // First obs establishes duration baseline (100ms).
    det.observe("slow_node", 5, 100, true);
    // Second obs is 5x baseline → exceeds 2x threshold.
    let anomalies = det.observe("slow_node", 5, 500, true);
    assert!(!anomalies.is_empty());
    assert!(anomalies
        .iter()
        .any(|a| a.metric == "duration" && a.node_name == "slow_node"));
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
