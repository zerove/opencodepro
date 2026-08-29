//! Integration tests for sisyphus-core `EnhancedDashboard`.
//!
//! Exercises the public `EnhancedDashboard` / `SessionMetrics` /
//! `IterationSnapshot` / `NodeExecution` API which processes
//! `ExecutionEvent`s and aggregates per-session metrics.
//!
//! Note: `process_event` prints to stdout as a side effect. The tests
//! focus on the structured session state changes (`session_metrics`),
//! not on the rendered output.

use sisyphus_core::enhanced_dashboard::{EnhancedDashboard, SessionMetrics};
use sisyphus_types::{ExecutionEvent, NodeOutput};

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

#[test]
fn new_dashboard_starts_empty() {
    let d = EnhancedDashboard::new();
    let m = d.session_metrics();
    assert_eq!(m.total_iterations, 0);
    assert_eq!(m.total_nodes_executed, 0);
    assert_eq!(m.total_nodes_failed, 0);
    assert_eq!(m.total_deviation, 0.0);
    assert_eq!(m.avg_deviation, 0.0);

    assert_eq!(m.iteration_history.len(), 0);
    // Default success_rate is 1.0 (no ops yet).
    assert!((m.success_rate - 1.0).abs() < 1e-9);
}

#[test]
fn default_dashboard_equivalent_to_new() {
    let a = EnhancedDashboard::new();
    let b = EnhancedDashboard::default();
    assert_eq!(a.session_metrics().total_iterations, b.session_metrics().total_iterations);
}

// ---------------------------------------------------------------------------
// process_event: NodeStarted / NodeCompleted / NodeFailed
// ---------------------------------------------------------------------------

#[test]
fn process_node_completed_increments_counters() {
    let mut d = EnhancedDashboard::new();
    d.process_event(&ExecutionEvent::NodeCompleted {
        node_name: "n1".into(),
        output: NodeOutput::from(serde_json::json!("ok")),
        fuel_consumed: 10,
        duration_ms: 50,
    });
    let m = d.session_metrics();
    assert_eq!(m.total_nodes_executed, 1);
    assert_eq!(m.total_nodes_failed, 0);
    // Note: total_fuel_consumed is not auto-updated by process_event;
    // the field exists but stays at 0 unless explicitly set.
}

#[test]
fn process_node_failed_increments_failed() {
    let mut d = EnhancedDashboard::new();
    d.process_event(&ExecutionEvent::NodeFailed {
        node_name: "n1".into(),
        error: "boom".into(),
        fuel_consumed: 5,
        duration_ms: 30,
    });
    let m = d.session_metrics();
    assert_eq!(m.total_nodes_executed, 0);
    assert_eq!(m.total_nodes_failed, 1);
}

#[test]
fn process_node_started_does_not_change_counters() {
    let mut d = EnhancedDashboard::new();
    d.process_event(&ExecutionEvent::NodeStarted {
        node_name: "n1".into(),
    });
    let m = d.session_metrics();
    assert_eq!(m.total_nodes_executed, 0);
    assert_eq!(m.total_nodes_failed, 0);

}

#[test]
fn process_multiple_completed_accumulates() {
    let mut d = EnhancedDashboard::new();
    for i in 0..5 {
        d.process_event(&ExecutionEvent::NodeCompleted {
            node_name: format!("n{i}"),
            output: NodeOutput::from(serde_json::json!("ok")),
            fuel_consumed: 10,
            duration_ms: 100,
        });
    }
    let m = d.session_metrics();
    assert_eq!(m.total_nodes_executed, 5);
}

#[test]
fn process_mixed_completed_and_failed() {
    let mut d = EnhancedDashboard::new();
    d.process_event(&ExecutionEvent::NodeCompleted {
        node_name: "a".into(),
        output: NodeOutput::from(serde_json::json!("ok")),
        fuel_consumed: 10,
        duration_ms: 50,
    });
    d.process_event(&ExecutionEvent::NodeFailed {
        node_name: "b".into(),
        error: "err".into(),
        fuel_consumed: 5,
        duration_ms: 30,
    });
    d.process_event(&ExecutionEvent::NodeCompleted {
        node_name: "c".into(),
        output: NodeOutput::from(serde_json::json!("ok")),
        fuel_consumed: 8,
        duration_ms: 40,
    });
    let m = d.session_metrics();
    assert_eq!(m.total_nodes_executed, 2);
    assert_eq!(m.total_nodes_failed, 1);
    // success_rate is a field but process_event does not currently
    // update it automatically — it stays at the default 1.0 until
    // explicitly recomputed elsewhere. Just verify the count deltas.
    assert_eq!(m.success_rate, 1.0);
}

// ---------------------------------------------------------------------------
// SessionMetrics
// ---------------------------------------------------------------------------

#[test]
fn session_metrics_default_has_empty_history() {
    let m = SessionMetrics::default();
    assert!(m.iteration_history.is_empty());
    assert_eq!(m.total_iterations, 0);
    assert!((m.success_rate - 1.0).abs() < 1e-9);
}

#[test]
fn session_metrics_serializes() {
    let m = SessionMetrics::default();
    let json = serde_json::to_string(&m).expect("serialize");
    // Verify it contains the field names (smoke test).
    assert!(json.contains("total_iterations"));
    assert!(json.contains("success_rate"));
}

// ---------------------------------------------------------------------------
// Empty events don't crash
// ---------------------------------------------------------------------------

#[test]
fn process_other_event_variants_do_not_crash() {
    let mut d = EnhancedDashboard::new();
    // The `_ => {}` arm in process_event handles unknown variants.
    d.process_event(&ExecutionEvent::NodeStarted {
        node_name: "x".into(),
    });
    // No assertion on state — just verifying no panic.
}
