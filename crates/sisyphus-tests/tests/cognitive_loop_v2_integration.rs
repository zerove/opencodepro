//! Integration tests for Cognitive Loop v2 features.
//!
//! Coverage:
//! - `AdaptiveDepthController` convergence / plateau detection
//! - `AnomalyDetector` fuel spike, error rate, and report generation
//! - `ChaosRunner` fault injection in DAG execution context
//!
//! These tests exercise the public API of `sisyphus_core::cognitive_loop_v2`
//! alongside `ChaosRunner` from `sisyphus_core::chaos_testing`.

use sisyphus_core::{
    AdaptiveDepthController, AnomalyDetector, AnomalySeverity, ChaosExperiment, ChaosRunner,
    FaultType,
};
use std::time::Duration;

// ---------------------------------------------------------------------------
// AdaptiveDepthController
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_adaptive_depth_controller_convergence() {
    // High target + small min_improvement so plateau detection governs stop.
    let controller = AdaptiveDepthController::new(0.99, 0.05, 30, 3);

    // --- Phase 1: improving confidence → should keep returning true ---
    for conf in [0.3, 0.5, 0.7, 0.85] {
        assert!(
            controller.observe_iteration(conf),
            "Should continue while confidence is still improving (conf={})",
            conf
        );
    }

    // --- Phase 2: plateau → should eventually return false ---
    for _ in 0..5 {
        if !controller.observe_iteration(0.90) {
            return; // ✓ Plateau detected, test passes
        }
    }

    panic!(
        "AdaptiveDepthController did not detect convergence plateau \
         within 5 iterations of flat confidence"
    );
}

#[tokio::test]
async fn test_adaptive_depth_controller_continues_on_improvement() {
    // Smaller window + tight min_improvement.
    let controller = AdaptiveDepthController::new(0.99, 0.01, 30, 2);

    // Steady improvement should keep returning true for many iterations.
    for i in 0..10 {
        let conf = 0.3 + (i as f64) * 0.05; // 0.30 → 0.75
        assert!(
            controller.observe_iteration(conf),
            "Should continue while improving (iter={}, conf={})",
            i,
            conf
        );
    }
}

// ---------------------------------------------------------------------------
// AnomalyDetector
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_anomaly_detector_detects_fuel_spike() {
    // Low fuel threshold so a 10× spike is easily flagged.
    let detector = AnomalyDetector::new(1.5, 3.0, 0.3);

    // Establish baseline with normal observations.
    for _ in 0..5 {
        detector.observe("node_a", 100, 50, true);
    }

    // Inject a big fuel spike – anomaly is returned inline from observe().
    let anomalies = detector.observe("node_a", 1000, 50, true);
    assert!(
        !anomalies.is_empty(),
        "Fuel spike should be detected inline"
    );

    let fuel_anomalies: Vec<_> = anomalies.iter().filter(|a| a.metric == "fuel").collect();
    assert!(
        !fuel_anomalies.is_empty(),
        "At least one anomaly is about fuel"
    );

    // Report generation must not panic; may contain global-level anomalies.
    let report = detector.generate_report();
    assert!(
        report.overall_severity == AnomalySeverity::Low
            || report.overall_severity == AnomalySeverity::Medium
            || report.overall_severity == AnomalySeverity::High
            || report.overall_severity == AnomalySeverity::Critical,
        "Report should have a valid severity level"
    );
}

#[tokio::test]
async fn test_anomaly_detector_detects_duration_spike() {
    let detector = AnomalyDetector::new(2.0, 2.0, 0.3);

    // Baseline.
    for _ in 0..4 {
        detector.observe("node_b", 100, 50, true);
    }

    // Duration spike.
    let anomalies = detector.observe("node_b", 100, 300, true);
    assert!(!anomalies.is_empty(), "Duration spike should be detected");

    let dur_anomalies: Vec<_> = anomalies.iter().filter(|a| a.metric == "duration").collect();
    assert!(
        !dur_anomalies.is_empty(),
        "At least one anomaly is about duration"
    );
}

#[tokio::test]
async fn test_anomaly_detector_detects_error_rate() {
    let detector = AnomalyDetector::default();

    // Feed many failures – the error rate threshold (default 0.3) will be exceeded.
    // We need enough observations that the error-rate check (which requires total > 3) fires.
    for _ in 0..6 {
        detector.observe("node_c", 100, 50, false);
    }

    // The next observation should return an error_rate anomaly.
    let anomalies = detector.observe("node_c", 100, 50, false);
    let err_anomalies: Vec<_> = anomalies.iter().filter(|a| a.metric == "error_rate").collect();
    assert!(
        !err_anomalies.is_empty(),
        "High error rate should be detected on observe()"
    );

    // Feed even more failures across multiple nodes so
    // total_error_count > 10 and should_extra_reflect becomes true.
    for i in 0..6 {
        detector.observe(&format!("node_extra_{}", i), 100, 50, false);
    }

    // generate_report() should flag the global error rate and extra reflection.
    let report = detector.generate_report();
    let global_err: Vec<_> =
        report.anomalies.iter().filter(|a| a.metric == "global_error_rate").collect();
    assert!(
        !global_err.is_empty(),
        "Report should contain global error rate anomaly"
    );
    assert!(
        report.should_extra_reflect,
        "High error rate should trigger extra reflection flag"
    );
}

// ---------------------------------------------------------------------------
// AnomalyDetector report generation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_anomaly_detector_report_empty() {
    let detector = AnomalyDetector::default();
    // No observations at all – report should be empty / low severity.
    let report = detector.generate_report();
    assert_eq!(
        report.overall_severity,
        AnomalySeverity::Low,
        "Empty detector should produce Low severity"
    );
    assert!(
        !report.should_extra_reflect,
        "Empty detector should not request extra reflection"
    );
}

#[tokio::test]
async fn test_anomaly_detector_report_triggers_extra_reflect() {
    // Many failures across multiple nodes.
    let detector = AnomalyDetector::default();
    for i in 0..12 {
        detector.observe(&format!("node_{}", i), 100, 50, false);
    }

    let report = detector.generate_report();
    assert!(
        report.should_extra_reflect,
        "Many failures should trigger extra reflection"
    );
}

// ---------------------------------------------------------------------------
// ChaosRunner in DAG execution context
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_chaos_runner_in_dag_context() {
    use sisyphus_core::chaos_testing::dag_test;

    // --- Scenario 1: Diamond DAG with 100 % fault probability ---------------
    let config = dag_test::diamond().expect("diamond DAG");
    assert_eq!(config.nodes.len(), 4, "Diamond DAG has 4 nodes");

    let runner = ChaosRunner::new();
    runner.start_experiment(ChaosExperiment {
        name: "dag_diamond".into(),
        fault_types: vec![FaultType::ToolCrash],
        fault_probability: 1.0,
        duration: Duration::from_secs(5),
        target_operations: None,
    });

    let mut faults = 0u64;
    for node in &config.nodes {
        if runner.maybe_inject(&node.name).is_some() {
            faults += 1;
        }
    }
    assert_eq!(faults, 4, "All 4 DAG nodes should fault with p=1.0");

    let result = runner.stop_experiment().unwrap();
    assert_eq!(result.operations_attempted, 4);
    assert_eq!(result.faults_injected, 4);

    // --- Scenario 2: Linear chain with mixed faults and partial probability --
    let chain = dag_test::linear_chain(6).expect("linear chain");
    let runner2 = ChaosRunner::new();
    runner2.start_experiment(ChaosExperiment {
        name: "dag_linear_mixed".into(),
        fault_types: vec![FaultType::LlmTimeout, FaultType::StorageWriteFailure],
        fault_probability: 0.5,
        duration: Duration::from_secs(5),
        target_operations: None,
    });

    let mut faults2 = 0u64;
    for node in &chain.nodes {
        if let Some(err) = runner2.maybe_inject(&node.name) {
            faults2 += 1;
            let msg = err.to_string();
            assert!(
                msg.contains("LLM timeout") || msg.contains("Storage write"),
                "Unexpected fault message: {}",
                msg
            );
        }
    }
    // With 6 nodes and p=0.5 we should see some faults (not necessarily exactly 3).
    assert!(
        faults2 > 0 && faults2 < 6,
        "Expected 1-5 faults out of 6 with p=0.5, got {}",
        faults2
    );

    let _ = runner2.stop_experiment();

    // --- Scenario 3: Empty DAG edge case ------------------------------------
    let _empty = dag_test::linear_chain(0).expect("empty chain");
    let runner3 = ChaosRunner::new();
    runner3.start_experiment(ChaosExperiment {
        name: "dag_empty".into(),
        fault_types: vec![FaultType::NetworkPartition],
        fault_probability: 1.0,
        duration: Duration::from_secs(5),
        target_operations: None,
    });

    // No nodes to iterate over, so no faults.
    let result3 = runner3.stop_experiment().unwrap();
    assert_eq!(result3.operations_attempted, 0);
    assert_eq!(result3.faults_injected, 0);
}

#[tokio::test]
async fn test_chaos_runner_in_dag_context_targeted() {
    use sisyphus_core::chaos_testing::dag_test;

    // Only target specific DAG nodes.
    let config = dag_test::wide_fanout(10).expect("fanout DAG"); // 1 root + 10 leaves
    let runner = ChaosRunner::new();
    runner.start_experiment(ChaosExperiment {
        name: "dag_targeted".into(),
        fault_types: vec![FaultType::LlmGarbage],
        fault_probability: 1.0,
        duration: Duration::from_secs(5),
        target_operations: Some(vec!["root".into()]), // Only target the root
    });

    let mut faults = 0u64;
    let mut total = 0u64;
    for node in &config.nodes {
        total += 1;
        if runner.maybe_inject(&node.name).is_some() {
            faults += 1;
        }
    }

    assert_eq!(faults, 1, "Only root should be targeted");
    assert_eq!(total, 11, "Fanout DAG has 11 nodes");

    let result = runner.stop_experiment().unwrap();
    assert_eq!(result.faults_injected, 1);
    assert_eq!(result.operations_attempted, 11);
}
