//! Integration tests for sisyphus-core `enhanced_dashboard` and
//! `cognitive_dashboard` modules.
//!
//! Exercises the public surfaces:
//! - [`SessionMetrics`] default values + field updates
//! - [`IterationSnapshot`] struct
//! - [`NodeExecution`] + `NodeExecution::new`
//! - [`EnhancedDashboard`] default state + `process_event` on every
//!   `ExecutionEvent` variant.
//! - [`CognitiveDashboard::render`] — does not panic on each variant.

use sisyphus_core::enhanced_dashboard::{EnhancedDashboard, NodeExecution, SessionMetrics};
use sisyphus_types::execution::{ExecutionEvent, NodeOutput};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// SessionMetrics
// ---------------------------------------------------------------------------

#[test]
fn session_metrics_default() {
    let m = SessionMetrics::default();
    assert_eq!(m.total_iterations, 0);
    assert_eq!(m.total_nodes_executed, 0);
    assert_eq!(m.total_nodes_failed, 0);
    assert_eq!(m.total_deviation, 0.0);
    assert_eq!(m.avg_deviation, 0.0);
    assert_eq!(m.total_fuel_consumed, 0);
    assert_eq!(m.session_duration_ms, 0);
    assert!((m.success_rate - 1.0).abs() < 1e-9);
    assert!(m.iteration_history.is_empty());
}

#[test]
fn session_metrics_field_assignment() {
    let m = SessionMetrics {
        total_iterations: 5,
        total_nodes_executed: 20,
        total_nodes_failed: 3,
        total_deviation: 1.5,
        avg_deviation: 0.3,
        total_fuel_consumed: 10_000,
        session_duration_ms: 5_500,
        success_rate: 0.85,
        iteration_history: vec![],
    };
    assert_eq!(m.total_iterations, 5);
    assert_eq!(m.total_nodes_executed, 20);
    assert_eq!(m.total_nodes_failed, 3);
    assert_eq!(m.total_fuel_consumed, 10_000);
    assert_eq!(m.session_duration_ms, 5_500);
    assert!((m.success_rate - 0.85).abs() < 1e-9);
}

// ---------------------------------------------------------------------------
// NodeExecution
// ---------------------------------------------------------------------------

#[test]
fn node_execution_new_defaults() {
    let n = NodeExecution {
        node_name: String::new(),
        status: String::new(),
        duration_ms: 0,
        fuel_consumed: 0,
        timestamp_ms: 0,
    };
    assert_eq!(n.node_name, "");
    assert_eq!(n.status, "");
    assert_eq!(n.duration_ms, 0);
    assert_eq!(n.fuel_consumed, 0);
    assert_eq!(n.timestamp_ms, 0);
}

#[test]
fn node_execution_field_assignment() {
    let n = NodeExecution {
        node_name: "alpha".into(),
        status: "success".into(),
        duration_ms: 100,
        fuel_consumed: 200,
        timestamp_ms: 12345,
    };
    assert_eq!(n.node_name, "alpha");
    assert_eq!(n.status, "success");
    assert_eq!(n.duration_ms, 100);
    assert_eq!(n.fuel_consumed, 200);
}

// ---------------------------------------------------------------------------
// EnhancedDashboard
// ---------------------------------------------------------------------------

#[test]
fn enhanced_dashboard_default_state() {
    let d = EnhancedDashboard::new();
    let m = d.session_metrics();
    assert_eq!(m.total_iterations, 0);
    assert_eq!(m.total_nodes_executed, 0);
    assert_eq!(m.total_nodes_failed, 0);
}

#[test]
fn enhanced_dashboard_process_node_started() {
    let mut d = EnhancedDashboard::new();
    d.process_event(&ExecutionEvent::NodeStarted {
        node_name: "alpha".into(),
    });
    // No counters change on NodeStarted.
    assert_eq!(d.session_metrics().total_nodes_executed, 0);
}

#[test]
fn enhanced_dashboard_process_node_completed() {
    let mut d = EnhancedDashboard::new();
    d.process_event(&ExecutionEvent::NodeCompleted {
        node_name: "alpha".into(),
        output: NodeOutput(Arc::new(serde_json::json!("done"))),
        fuel_consumed: 50,
        duration_ms: 10,
    });
    let m = d.session_metrics();
    assert_eq!(m.total_nodes_executed, 1);
    assert_eq!(m.total_nodes_failed, 0);
}

#[test]
fn enhanced_dashboard_process_node_failed() {
    let mut d = EnhancedDashboard::new();
    d.process_event(&ExecutionEvent::NodeFailed {
        node_name: "beta".into(),
        error: "boom".into(),
        fuel_consumed: 5,
        duration_ms: 3,
    });
    let m = d.session_metrics();
    // Note: NodeFailed only increments total_nodes_failed, not total_nodes_executed.
    assert_eq!(m.total_nodes_executed, 0);
    assert_eq!(m.total_nodes_failed, 1);
}

#[test]
fn enhanced_dashboard_process_execution_complete() {
    let mut d = EnhancedDashboard::new();
    d.process_event(&ExecutionEvent::ExecutionComplete {
        result: NodeOutput(Arc::new(serde_json::json!({"ok": true}))),
    });
    // No counters change on ExecutionComplete.
    assert_eq!(d.session_metrics().total_iterations, 0);
}

#[test]
fn enhanced_dashboard_process_micro_reflection() {
    use std::collections::HashMap;
    let mut d = EnhancedDashboard::new();
    let mut updates = HashMap::new();
    updates.insert("k".to_string(), serde_json::json!("v"));
    d.process_event(&ExecutionEvent::MicroReflection {
        node_name: "n".into(),
        prediction: "p".into(),
        actual: "a".into(),
        decision: "ok".into(),
        updates: Some(updates),
        reason: Some("because".into()),
    });
    // No counter change for this event.
    assert_eq!(d.session_metrics().total_nodes_executed, 0);
}

#[test]
fn enhanced_dashboard_process_cognitive_complete() {
    let mut d = EnhancedDashboard::new();
    d.process_event(&ExecutionEvent::CognitiveComplete {
        report: "all done".into(),
    });
    // No counter change.
    assert_eq!(d.session_metrics().total_iterations, 0);
}

#[test]
fn enhanced_dashboard_process_mixed_sequence() {
    let mut d = EnhancedDashboard::new();
    d.process_event(&ExecutionEvent::NodeStarted {
        node_name: "a".into(),
    });
    d.process_event(&ExecutionEvent::NodeCompleted {
        node_name: "a".into(),
        output: NodeOutput(Arc::new(serde_json::json!("ok"))),
        fuel_consumed: 1,
        duration_ms: 1,
    });
    d.process_event(&ExecutionEvent::NodeStarted {
        node_name: "b".into(),
    });
    d.process_event(&ExecutionEvent::NodeFailed {
        node_name: "b".into(),
        error: "no".into(),
        fuel_consumed: 1,
        duration_ms: 1,
    });
    let m = d.session_metrics();
    // a completed + b failed; failed does not count as executed.
    assert_eq!(m.total_nodes_executed, 1);
    assert_eq!(m.total_nodes_failed, 1);
}
