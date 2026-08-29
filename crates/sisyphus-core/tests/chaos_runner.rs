//! Integration tests for sisyphus-core `chaos_testing` (runner / experiment API).
//!
//! Exercises the public [`ChaosRunner`] and related types:
//! `FaultType`, `ChaosExperiment`, `ChaosExperimentResult`,
//! `success_rate`. The runner enables fault injection under
//! controlled probabilities and target operations.

use sisyphus_core::chaos_testing::{ChaosExperiment, ChaosExperimentResult, ChaosRunner, FaultType};
use std::time::Duration;

// ---------------------------------------------------------------------------
// ChaosRunner construction
// ---------------------------------------------------------------------------

#[test]
fn new_runner_is_not_running() {
    let r = ChaosRunner::new();
    assert!(!r.is_running());
}

#[test]
fn default_runner_is_not_running() {
    let r = ChaosRunner::default();
    assert!(!r.is_running());
}

// ---------------------------------------------------------------------------
// maybe_inject (no experiment)
// ---------------------------------------------------------------------------

#[test]
fn maybe_inject_without_experiment_returns_none() {
    let r = ChaosRunner::new();
    // No experiment started → no injection.
    assert!(r.maybe_inject("any_op").is_none());
    assert!(r.maybe_inject("another").is_none());
}

// ---------------------------------------------------------------------------
// Experiment lifecycle
// ---------------------------------------------------------------------------

#[test]
fn start_experiment_marks_runner_as_running() {
    let r = ChaosRunner::new();
    let exp = ChaosExperiment {
        name: "test".into(),
        fault_types: vec![FaultType::ToolCrash],
        fault_probability: 0.0, // never inject
        duration: Duration::from_secs(60),
        target_operations: None,
    };
    r.start_experiment(exp);
    assert!(r.is_running());
}

#[test]
fn stop_experiment_returns_result() {
    let r = ChaosRunner::new();
    let exp = ChaosExperiment {
        name: "my_exp".into(),
        fault_types: vec![FaultType::LlmTimeout],
        fault_probability: 0.0,
        duration: Duration::from_secs(60),
        target_operations: None,
    };
    r.start_experiment(exp);
    let result = r.stop_experiment();
    assert!(result.is_some());
    let result = result.unwrap();
    assert_eq!(result.name, "my_exp");
    // No operations attempted → success_rate defaults to 1.0.
    assert_eq!(result.operations_attempted, 0);
    assert_eq!(result.operations_succeeded, 0);
    assert_eq!(result.operations_failed, 0);
}

#[test]
fn stop_experiment_marks_runner_as_not_running() {
    let r = ChaosRunner::new();
    let exp = ChaosExperiment {
        name: "x".into(),
        fault_types: vec![],
        fault_probability: 0.0,
        duration: Duration::from_secs(60),
        target_operations: None,
    };
    r.start_experiment(exp);
    let _ = r.stop_experiment();
    assert!(!r.is_running());
}

#[test]
fn stop_experiment_when_not_running_returns_none() {
    let r = ChaosRunner::new();
    assert!(r.stop_experiment().is_none());
}

// ---------------------------------------------------------------------------
// Target operations filter
// ---------------------------------------------------------------------------

#[test]
fn target_operations_filter_excludes_unrelated_ops() {
    let r = ChaosRunner::new();
    let exp = ChaosExperiment {
        name: "filtered".into(),
        fault_types: vec![FaultType::ToolCrash],
        // 100% probability so any targeted op would be injected.
        fault_probability: 1.0,
        duration: Duration::from_secs(60),
        target_operations: Some(vec!["specific_op".to_string()]),
    };
    r.start_experiment(exp);
    // "safe_op" doesn't contain "specific_op" → not targeted → not injected.
    assert!(r.maybe_inject("safe_op").is_none());
    // The op_count was still incremented for the "safe_op" check.
    let result = r.stop_experiment().expect("result");
    assert_eq!(result.operations_attempted, 1);
    assert_eq!(result.faults_injected, 0);
}

// ---------------------------------------------------------------------------
// Probability = 0 means no injection
// ---------------------------------------------------------------------------

#[test]
fn zero_probability_never_injects() {
    let r = ChaosRunner::new();
    let exp = ChaosExperiment {
        name: "no_faults".into(),
        fault_types: vec![FaultType::ToolCrash],
        fault_probability: 0.0,
        duration: Duration::from_secs(60),
        target_operations: None,
    };
    r.start_experiment(exp);
    for _ in 0..50 {
        assert!(r.maybe_inject("any").is_none());
    }
    let result = r.stop_experiment().expect("result");
    assert_eq!(result.faults_injected, 0);
    // All 50 ops are counted as "succeeded" since none were faulted.
    assert_eq!(result.operations_attempted, 50);
    assert_eq!(result.operations_succeeded, 50);
    assert_eq!(result.operations_failed, 0);
}

// ---------------------------------------------------------------------------
// ChaosExperimentResult::success_rate
// ---------------------------------------------------------------------------

#[test]
fn success_rate_with_zero_attempts_returns_one() {
    let r = ChaosExperimentResult {
        name: "x".into(),
        duration: Duration::from_secs(0),
        operations_attempted: 0,
        operations_succeeded: 0,
        operations_failed: 0,
        faults_injected: 0,
        system_recovered: true,
        observations: vec![],
    };
    assert_eq!(r.success_rate(), 1.0);
}

#[test]
fn success_rate_with_all_succeeded() {
    let r = ChaosExperimentResult {
        name: "x".into(),
        duration: Duration::from_secs(0),
        operations_attempted: 10,
        operations_succeeded: 10,
        operations_failed: 0,
        faults_injected: 0,
        system_recovered: true,
        observations: vec![],
    };
    assert!((r.success_rate() - 1.0).abs() < 1e-9);
}

#[test]
fn success_rate_with_half_succeeded() {
    let r = ChaosExperimentResult {
        name: "x".into(),
        duration: Duration::from_secs(0),
        operations_attempted: 4,
        operations_succeeded: 2,
        operations_failed: 2,
        faults_injected: 2,
        system_recovered: false,
        observations: vec![],
    };
    assert!((r.success_rate() - 0.5).abs() < 1e-9);
}

#[test]
fn success_rate_with_all_failed() {
    let r = ChaosExperimentResult {
        name: "x".into(),
        duration: Duration::from_secs(0),
        operations_attempted: 5,
        operations_succeeded: 0,
        operations_failed: 5,
        faults_injected: 5,
        system_recovered: false,
        observations: vec![],
    };
    assert_eq!(r.success_rate(), 0.0);
}

// ---------------------------------------------------------------------------
// FaultType equality
// ---------------------------------------------------------------------------

#[test]
fn fault_type_equality() {
    assert_eq!(FaultType::LlmTimeout, FaultType::LlmTimeout);
    assert_ne!(FaultType::LlmTimeout, FaultType::LlmGarbage);
    assert_ne!(FaultType::ToolCrash, FaultType::ToolHang);
    assert_eq!(
        FaultType::RandomDelay(100),
        FaultType::RandomDelay(100)
    );
    assert_ne!(
        FaultType::RandomDelay(100),
        FaultType::RandomDelay(200)
    );
}
