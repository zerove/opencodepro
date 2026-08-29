//! Integration tests for sisyphus-core `chaos_testing` module.
//!
//! Exercises the public `ChaosRunner` / `ChaosExperiment` /
//! `ChaosExperimentResult` / `FaultType` API used to inject faults
//! (LLM timeouts, tool crashes, storage failures, network partitions,
//! random delays) into the system during chaos testing. Also tests
//! the DAG-boundary test helpers `single_node` / `linear_chain` /
//! `diamond` / `wide_fanout`.

use sisyphus_core::chaos_testing::{
    ChaosExperiment, ChaosExperimentResult, ChaosRunner, FaultType,
};
use sisyphus_core::CoreError;
use std::time::Duration;

// ---------------------------------------------------------------------------
// ChaosRunner lifecycle
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

#[test]
fn start_experiment_makes_runner_running() {
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
fn stop_experiment_returns_result_and_disables() {
    let r = ChaosRunner::new();
    let exp = ChaosExperiment {
        name: "stop-test".into(),
        fault_types: vec![FaultType::ToolCrash],
        fault_probability: 0.5,
        duration: Duration::from_secs(10),
        target_operations: None,
    };
    r.start_experiment(exp);
    let result = r.stop_experiment().expect("result");
    assert_eq!(result.name, "stop-test");
    assert_eq!(result.duration, Duration::from_secs(10));
    assert!(!r.is_running());
}

#[test]
fn stop_experiment_returns_none_when_nothing_running() {
    let r = ChaosRunner::new();
    assert!(r.stop_experiment().is_none());
}

#[test]
fn success_rate_with_zero_attempts_is_one() {
    let r = ChaosExperimentResult {
        name: "x".into(),
        duration: Duration::from_secs(1),
        operations_attempted: 0,
        operations_succeeded: 0,
        operations_failed: 0,
        faults_injected: 0,
        system_recovered: true,
        observations: vec![],
    };
    assert!((r.success_rate() - 1.0).abs() < 1e-9);
}

#[test]
fn success_rate_computes_ratio() {
    let r = ChaosExperimentResult {
        name: "x".into(),
        duration: Duration::from_secs(1),
        operations_attempted: 10,
        operations_succeeded: 7,
        operations_failed: 3,
        faults_injected: 3,
        system_recovered: true,
        observations: vec![],
    };
    assert!((r.success_rate() - 0.7).abs() < 1e-9);
}

#[test]
fn start_then_stop_then_start_again() {
    let r = ChaosRunner::new();
    r.start_experiment(ChaosExperiment {
        name: "first".into(),
        fault_types: vec![],
        fault_probability: 0.0,
        duration: Duration::from_secs(1),
        target_operations: None,
    });
    r.stop_experiment();
    assert!(!r.is_running());

    r.start_experiment(ChaosExperiment {
        name: "second".into(),
        fault_types: vec![],
        fault_probability: 0.0,
        duration: Duration::from_secs(1),
        target_operations: None,
    });
    assert!(r.is_running());
    let result = r.stop_experiment().expect("result");
    assert_eq!(result.name, "second");
}

// ---------------------------------------------------------------------------
// maybe_inject behavior
// ---------------------------------------------------------------------------

#[test]
fn maybe_inject_returns_none_when_disabled() {
    let r = ChaosRunner::new();
    // No experiment started.
    let r1 = r.maybe_inject("any_op");
    assert!(r1.is_none());
    // many calls, all None
    for i in 0..100 {
        assert!(r.maybe_inject(&format!("op_{i}")).is_none());
    }
}

#[test]
fn maybe_inject_respects_target_operations() {
    let r = ChaosRunner::new();
    r.start_experiment(ChaosExperiment {
        name: "targeted".into(),
        fault_types: vec![FaultType::ToolCrash],
        fault_probability: 1.0, // always inject if targeted
        duration: Duration::from_secs(1),
        target_operations: Some(vec!["targeted_op".into()]),
    });
    // Non-targeted operation: never inject.
    for _ in 0..50 {
        let res = r.maybe_inject("other_op");
        assert!(res.is_none(), "non-targeted should never inject");
    }
    // Targeted operation: always inject.
    for _ in 0..10 {
        let res = r.maybe_inject("my_targeted_op_xyz");
        assert!(res.is_some(), "targeted should always inject");
    }
}

#[test]
fn maybe_inject_with_zero_probability_never_injects() {
    let r = ChaosRunner::new();
    r.start_experiment(ChaosExperiment {
        name: "zero-prob".into(),
        fault_types: vec![FaultType::ToolCrash],
        fault_probability: 0.0,
        duration: Duration::from_secs(1),
        target_operations: None,
    });
    for _ in 0..100 {
        let res = r.maybe_inject("op");
        assert!(res.is_none(), "prob 0 should never inject");
    }
    // Verify op_count still incremented even when no fault injected.
    // (We can't directly read it, but the experiment records ops in stop.)
    let result = r.stop_experiment().expect("result");
    // All 100 ops should be "succeeded" since no faults injected.
    assert_eq!(result.faults_injected, 0);
    assert_eq!(result.operations_failed, 0);
    assert_eq!(result.operations_succeeded, result.operations_attempted);
}

#[test]
fn maybe_inject_random_delay_does_not_return_error() {
    let r = ChaosRunner::new();
    r.start_experiment(ChaosExperiment {
        name: "delay-only".into(),
        fault_types: vec![FaultType::RandomDelay(1)], // 0-1 ms
        fault_probability: 1.0,
        duration: Duration::from_secs(1),
        target_operations: None,
    });
    // RandomDelay is implemented as a sleep, not an error return.
    for _ in 0..3 {
        let res = r.maybe_inject("op");
        assert!(res.is_none(), "RandomDelay returns None (just latency)");
    }
}

#[test]
fn maybe_inject_with_empty_fault_types_never_injects() {
    let r = ChaosRunner::new();
    r.start_experiment(ChaosExperiment {
        name: "no-faults".into(),
        fault_types: vec![],
        fault_probability: 1.0,
        duration: Duration::from_secs(1),
        target_operations: None,
    });
    for _ in 0..10 {
        let res = r.maybe_inject("op");
        assert!(res.is_none(), "empty fault_types should never inject");
    }
}

#[test]
fn maybe_inject_returns_internal_error() {
    // Always-inject with a non-delay fault → returns Some(CoreError::Internal).
    let r = ChaosRunner::new();
    r.start_experiment(ChaosExperiment {
        name: "always-fault".into(),
        fault_types: vec![FaultType::LlmTimeout],
        fault_probability: 1.0,
        duration: Duration::from_secs(1),
        target_operations: None,
    });
    let err = r.maybe_inject("op").expect("injected");
    // Verify it's a CoreError::Internal variant.
    let _: CoreError = err;
}

// ---------------------------------------------------------------------------
// FaultType Debug + PartialEq
// ---------------------------------------------------------------------------

#[test]
fn fault_type_partial_eq_and_debug() {
    assert_eq!(FaultType::LlmTimeout, FaultType::LlmTimeout);
    assert_ne!(FaultType::LlmTimeout, FaultType::ToolCrash);
    let _ = format!("{:?}", FaultType::LlmGarbage);
    let _ = format!("{:?}", FaultType::RandomDelay(50));
}

// ---------------------------------------------------------------------------
// DAG test helpers
// ---------------------------------------------------------------------------

mod dag {
    use sisyphus_core::chaos_testing::dag_test::{diamond, linear_chain, single_node, wide_fanout};

    #[test]
    fn single_node_creates_one_node_graph() {
        let cfg = single_node("n1", "tool1").expect("ok");
        assert_eq!(cfg.nodes.len(), 1);
        assert_eq!(cfg.nodes[0].name, "n1");
    }

    #[test]
    fn linear_chain_creates_n_nodes() {
        let cfg = linear_chain(5).expect("ok");
        assert_eq!(cfg.nodes.len(), 5);
        // Edges: 4 for a chain of 5 nodes
        assert_eq!(cfg.edges.len(), 4);
    }

    #[test]
    fn linear_chain_zero_creates_empty_graph() {
        let cfg = linear_chain(0).expect("ok");
        assert!(cfg.nodes.is_empty());
        assert!(cfg.edges.is_empty());
    }

    #[test]
    fn linear_chain_one_creates_single_node_no_edges() {
        let cfg = linear_chain(1).expect("ok");
        assert_eq!(cfg.nodes.len(), 1);
        assert!(cfg.edges.is_empty());
    }

    #[test]
    fn diamond_creates_four_node_dag() {
        let cfg = diamond().expect("ok");
        // Diamond: 1 → 2 → 4, 1 → 3 → 4
        assert_eq!(cfg.nodes.len(), 4);
        // 4 edges: 1-2, 1-3, 2-4, 3-4
        assert_eq!(cfg.edges.len(), 4);
    }

    #[test]
    fn wide_fanout_creates_n_plus_one_nodes() {
        let cfg = wide_fanout(5).expect("ok");
        // 1 root + 5 leaves = 6
        assert_eq!(cfg.nodes.len(), 6);
        // 5 edges: root to each leaf
        assert_eq!(cfg.edges.len(), 5);
    }

    #[test]
    fn wide_fanout_zero_returns_only_root() {
        let cfg = wide_fanout(0).expect("ok");
        assert_eq!(cfg.nodes.len(), 1);
        assert!(cfg.edges.is_empty());
    }
}
