//! Integration tests for sisyphus-types `execution::ExecutionContext`
//! pure constructors and empty state.

use sisyphus_types::execution::ExecutionContext;

#[test]
fn execution_context_new_is_empty() {
    let ctx = ExecutionContext::new();
    assert!(ctx.results.is_empty());
    assert!(ctx.measurements.is_empty());
}

#[test]
fn execution_context_default_trait() {
    let ctx: ExecutionContext = Default::default();
    assert!(ctx.results.is_empty());
}

#[test]
fn execution_context_with_capacity() {
    let ctx = ExecutionContext::with_capacity(10);
    assert!(ctx.results.is_empty());
    assert!(ctx.measurements.is_empty());
}

#[test]
fn execution_context_with_capacity_zero() {
    let ctx = ExecutionContext::with_capacity(0);
    assert!(ctx.results.is_empty());
}

#[test]
fn execution_context_insert_result() {
    let ctx = ExecutionContext::new();
    ctx.results.insert(
        "node1".into(),
        sisyphus_types::execution::NodeOutput(Arc::new(serde_json::json!("result"))),
    );
    assert_eq!(ctx.results.len(), 1);
    assert!(ctx.results.contains_key("node1"));
}

#[test]
fn execution_context_insert_measurement() {
    let ctx = ExecutionContext::new();
    use sisyphus_types::node::ExecutionMeasurement;
    ctx.measurements.insert(
        "n1".into(),
        ExecutionMeasurement {
            fuel_consumed: 100,
            duration_ms: 50,
            peak_memory_bytes: None,
        },
    );
    assert_eq!(ctx.measurements.len(), 1);
}

#[test]
fn execution_context_last_node_completed_starts_none() {
    let ctx = ExecutionContext::new();
    assert!(ctx.last_node_completed.read().unwrap().is_none());
}

use std::sync::Arc;
