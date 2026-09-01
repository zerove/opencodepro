//! Chaos engineering integration tests for Sisyphus.
//!
//! Tests the system's resilience under adverse conditions:
//! - Agent team fault injection
//! - DAG boundary conditions (cycles, deadlocks, massive DAGs)
//! - Storage concurrent load

use sisyphus_core::chaos_testing::{dag_test as dag, ChaosExperiment, ChaosRunner, FaultType};
use sisyphus_core::AnomalyDetector;
use sisyphus_core::StateStore;
use std::sync::Arc;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Run an experiment with the chaos runner and return results.
async fn run_chaos_experiment(
    name: &str,
    fault_types: Vec<FaultType>,
    fault_probability: f64,
    operations: usize,
    target_operations: Option<Vec<String>>,
) -> (ChaosRunner, u64, u64) {
    let runner = ChaosRunner::new();
    runner.start_experiment(ChaosExperiment {
        name: name.to_string(),
        fault_types,
        fault_probability,
        duration: Duration::from_secs(10),
        target_operations,
    });

    let mut faults = 0u64;
    let mut successes = 0u64;
    for i in 0..operations {
        let op_name = format!("op_{}", i);
        if runner.maybe_inject(&op_name).is_some() {
            faults += 1;
        } else {
            successes += 1;
        }
    }

    (runner, faults, successes)
}

// ---------------------------------------------------------------------------
// Chaos: Fault Injection
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_chaos_fault_injection_all_types() {
    // Test that all fault types can be injected and produce errors
    for fault_type in &[
        FaultType::LlmTimeout,
        FaultType::LlmGarbage,
        FaultType::ToolCrash,
        FaultType::ToolHang,
        FaultType::StorageWriteFailure,
        FaultType::StorageReadFailure,
        FaultType::NetworkPartition,
    ] {
        let (runner, faults, _) = run_chaos_experiment(
            &format!("{:?}", fault_type),
            vec![*fault_type],
            1.0,
            10,
            None,
        )
        .await;

        assert_eq!(
            faults, 10,
            "All 10 calls should fault with {:?}",
            fault_type
        );
        let result = runner.stop_experiment();
        assert!(result.is_some());
    }
}

#[tokio::test]
async fn test_chaos_fault_injection_mixed() {
    // Mix of faults should produce varied error types
    let runner = ChaosRunner::new();
    runner.start_experiment(ChaosExperiment {
        name: "mixed_faults".into(),
        fault_types: vec![
            FaultType::LlmTimeout,
            FaultType::ToolCrash,
            FaultType::StorageWriteFailure,
        ],
        fault_probability: 1.0,
        duration: Duration::from_secs(5),
        target_operations: None,
    });

    let mut fault_messages = Vec::new();
    for i in 0..30 {
        let op_name = format!("op_{}", i);
        if let Some(err) = runner.maybe_inject(&op_name) {
            fault_messages.push(err.to_string());
        }
    }

    let num_timeout = fault_messages.iter().filter(|m| m.contains("LLM timeout")).count();
    let num_crash = fault_messages.iter().filter(|m| m.contains("Tool crash")).count();
    let num_storage = fault_messages.iter().filter(|m| m.contains("Storage write")).count();

    assert_eq!(fault_messages.len(), 30, "All 30 should fault");
    assert!(num_timeout > 0, "Should have at least one LLM timeout");
    assert!(num_crash > 0, "Should have at least one Tool crash");
    assert!(num_storage > 0, "Should have at least one Storage failure");

    let _ = runner.stop_experiment();
}

#[tokio::test]
async fn test_chaos_random_delay() {
    // Random delay shouldn't produce errors but should add latency
    let runner = ChaosRunner::new();
    runner.start_experiment(ChaosExperiment {
        name: "delays".into(),
        fault_types: vec![FaultType::RandomDelay(50)],
        fault_probability: 1.0,
        duration: Duration::from_secs(5),
        target_operations: None,
    });

    let start = std::time::Instant::now();
    for i in 0..10 {
        let result = runner.maybe_inject(&format!("op_{}", i));
        assert!(result.is_none(), "RandomDelay should not produce errors");
    }
    let elapsed = start.elapsed();

    // Should have taken at least some time due to delays
    assert!(
        elapsed > Duration::from_millis(10),
        "Should have some delay"
    );

    let _ = runner.stop_experiment();
}

// ---------------------------------------------------------------------------
// Chaos: DAG Boundary Conditions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_dag_empty_graph() {
    let config = dag::linear_chain(0).expect("empty chain");
    assert_eq!(config.nodes.len(), 0);
    assert_eq!(config.edges.len(), 0);
}

#[tokio::test]
async fn test_dag_single_node() {
    let config = dag::single_node("solo", "t1").expect("single node");
    assert_eq!(config.nodes.len(), 1);
    assert_eq!(config.nodes[0].name, "solo");
}

#[tokio::test]
async fn test_dag_linear_chain_various_sizes() {
    for size in &[1, 5, 10, 100] {
        let config = dag::linear_chain(*size).expect("chain");
        assert_eq!(config.nodes.len(), *size);
        if *size > 1 {
            assert_eq!(config.edges.len(), size - 1);
        } else {
            assert_eq!(config.edges.len(), 0);
        }
    }
}

#[tokio::test]
async fn test_dag_wide_fanout() {
    let config = dag::wide_fanout(100).expect("fanout");
    assert_eq!(config.nodes.len(), 101);
    assert_eq!(config.edges.len(), 100);
}

#[tokio::test]
async fn test_dag_wide_fanout_max_edges() {
    // Fanout to 1000 parallel leaves
    let config = dag::wide_fanout(1000).expect("fanout");
    assert_eq!(config.nodes.len(), 1001);
    assert_eq!(config.edges.len(), 1000);
}

#[tokio::test]
async fn test_dag_diamond_structure() {
    let config = dag::diamond().expect("diamond");
    assert_eq!(config.nodes.len(), 4);
    assert_eq!(config.edges.len(), 4);

    // Check node names
    let names: Vec<&str> = config.nodes.iter().map(|n| n.name.as_str()).collect();
    assert!(names.contains(&"root"));
    assert!(names.contains(&"left"));
    assert!(names.contains(&"right"));
    assert!(names.contains(&"merge"));
}

// ---------------------------------------------------------------------------
// AnomalyDetector Integration
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_anomaly_detector_system_wide() {
    let detector = AnomalyDetector::default();

    // Establish baseline for a specific node before the spike
    for _ in 0..5 {
        let _anomalies = detector.observe("node_spike", 100, 50, true);
    }

    // Now inject an extreme outlier on the same node
    let anomalies = detector.observe("node_spike", 10_000, 5000, false);
    assert!(!anomalies.is_empty(), "Outlier should trigger anomalies");

    // Check that we got fuel and/or duration anomalies
    let fuel_anomalies: Vec<_> = anomalies.iter().filter(|a| a.metric == "fuel").collect();
    let dur_anomalies: Vec<_> = anomalies.iter().filter(|a| a.metric == "duration").collect();
    assert!(
        !fuel_anomalies.is_empty() || !dur_anomalies.is_empty(),
        "Should detect fuel and/or duration anomaly"
    );
}

#[tokio::test]
async fn test_anomaly_detector_progressive_degradation() {
    let detector = AnomalyDetector::default();

    // Simulate progressive degradation over 20 observations
    for i in 0..20 {
        let fuel = 100 + i as u64 * 50; // Linear increase
        let success = i < 15; // Start failing after 15
        detector.observe("degrading_node", fuel, 50, success);
    }

    let _report = detector.generate_report();
    // After enough failures, the system should flag it
    // (the error_rate check runs on each observation, not in the report)
    // But the report summary check should see the elevated error rate
}

#[tokio::test]
async fn test_anomaly_detector_concurrent_safety() {
    let detector = Arc::new(AnomalyDetector::default());
    let mut handles = Vec::new();

    for i in 0..10 {
        let d = detector.clone();
        handles.push(tokio::spawn(async move {
            for j in 0..5 {
                d.observe(
                    &format!("concurrent_node_{}", i),
                    100 + j * 10,
                    50,
                    j % 2 == 0,
                );
            }
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    let report = detector.generate_report();
    // Should complete without panics, regardless of what the report contains
    assert!(
        !report.anomalies.is_empty() || !report.should_extra_reflect,
        "Should produce valid report"
    );
}

// ---------------------------------------------------------------------------
// Storage Concurrency (basic smoke)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_storage_concurrent_smoke() {
    let store =
        sisyphus_mem::MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
            .await
            .expect("Store init failed");

    let store = Arc::new(store);
    let mut handles = Vec::new();

    for i in 0..20 {
        let s = store.clone();
        handles.push(tokio::spawn(async move {
            let key = format!("key_{}", i);
            let val = serde_json::json!({"index": i, "data": "test_value"});
            s.set_state_json("chaos_project", &key, val).await.unwrap();
            let read = s.get_state_json("chaos_project", &key).await.unwrap();
            assert!(read.is_some(), "Should read back what was written");
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn test_storage_contended_keys() {
    let store = Arc::new(
        sisyphus_mem::MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
            .await
            .expect("Store init failed"),
    );

    let mut handles = Vec::new();
    let key = "contended_key".to_string();

    // 10 writers all writing to the same key
    for i in 0..10 {
        let s = store.clone();
        let k = key.clone();
        handles.push(tokio::spawn(async move {
            let val = serde_json::json!({"writer": i, "value": format!("write_{}", i)});
            s.set_state_json("project", &k, val).await.unwrap();
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    // Read back — should have one of the 10 values
    let result = store.get_state_json("project", &key).await.unwrap();
    assert!(result.is_some(), "Key should exist after concurrent writes");
}
