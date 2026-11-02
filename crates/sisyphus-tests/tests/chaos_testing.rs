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

// ---------------------------------------------------------------------------
// Sustained mixed-load soak (bounded wall-clock budget)
// ---------------------------------------------------------------------------

/// 持续混合负载：混沌注入 + 状态存储读写 + 异常观测同时跑约 800ms。
///
/// 既有用例都是「短脉冲」——几个操作跑完就断言，累积性问题（计数器回绕、
/// 句柄/许可泄漏、锁粒度导致的惊群）永远暴露不出来。这里用固定 wall-clock
/// 预算 + 多线程并发做一次有界 soak：CI 里秒级完成，但确实覆盖了「持续」
/// 这一维度。无故障延迟类混沌（`RandomDelay`）被刻意排除，否则耗时由
/// 注入延迟而非负载决定。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_sustained_mixed_load_soak() {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    let runner = Arc::new(ChaosRunner::new());
    runner.start_experiment(ChaosExperiment {
        name: "soak".into(),
        fault_types: vec![FaultType::LlmTimeout],
        // 1.0 让每个混沌操作都必然命中，便于精确核对计数。
        fault_probability: 1.0,
        duration: Duration::from_secs(30),
        target_operations: None,
    });

    let store = Arc::new(
        sisyphus_mem::MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
            .await
            .expect("Store init failed"),
    );
    let detector = Arc::new(AnomalyDetector::default());

    let stop = Arc::new(AtomicBool::new(false));
    let chaos_ops = Arc::new(AtomicU64::new(0));
    let storage_ops = Arc::new(AtomicU64::new(0));
    let anomaly_obs = Arc::new(AtomicU64::new(0));

    let mut handles = Vec::new();

    // 2 个混沌 worker：每次注入都必然失败（LlmTimeout）。
    for _ in 0..2 {
        let runner = Arc::clone(&runner);
        let stop = Arc::clone(&stop);
        let counter = Arc::clone(&chaos_ops);
        handles.push(tokio::spawn(async move {
            while !stop.load(Ordering::Relaxed) {
                assert!(
                    runner.maybe_inject("soak_op").is_some(),
                    "prob=1.0 must always inject"
                );
                counter.fetch_add(1, Ordering::Relaxed);
                tokio::task::yield_now().await;
            }
        }));
    }

    // 2 个存储 worker：各写自己的键并读回，验证持续读写不丢不脏。
    for worker in 0..2 {
        let store = Arc::clone(&store);
        let stop = Arc::clone(&stop);
        let counter = Arc::clone(&storage_ops);
        handles.push(tokio::spawn(async move {
            let mut n = 0u64;
            while !stop.load(Ordering::Relaxed) {
                let key = format!("soak_{worker}_{}", n % 16);
                let value = serde_json::json!({"worker": worker, "n": n});
                store
                    .set_state_json("soak_project", &key, value.clone())
                    .await
                    .expect("write must succeed");
                let read = store
                    .get_state_json("soak_project", &key)
                    .await
                    .expect("read must succeed")
                    .expect("written key must be readable");
                assert_eq!(read, value, "read-after-write must match");
                n += 1;
                counter.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }

    // 2 个异常观测 worker：稳定基线 + 周期性尖峰，持续喂检测器。
    for worker in 0..2 {
        let detector = Arc::clone(&detector);
        let stop = Arc::clone(&stop);
        let counter = Arc::clone(&anomaly_obs);
        handles.push(tokio::spawn(async move {
            let node = format!("soak_node_{worker}");
            let mut n = 0u64;
            while !stop.load(Ordering::Relaxed) {
                let fuel = if n % 50 == 49 { 10_000 } else { 100 };
                let _ = detector.observe(&node, fuel, 50, !n.is_multiple_of(7));
                n += 1;
                counter.fetch_add(1, Ordering::Relaxed);
                // 必须让出执行权：`observe` 是纯同步函数，缺了这个 await
                // 该任务会永久占住 worker 线程，把 timer driver 一起饿死，
                // 于是 `sleep` 永远不返回（本用例初版就是这样自锁的）。
                tokio::task::yield_now().await;
            }
        }));
    }

    // ── 持续 800ms ──
    tokio::time::sleep(Duration::from_millis(800)).await;
    stop.store(true, Ordering::Relaxed);
    for h in handles {
        h.await.expect("soak worker must not panic");
    }

    // 计数一致性：混沌计数来自 AtomicU64，必须等于成功注入的次数。
    let result = runner.stop_experiment().expect("experiment must have been running");
    let attempted = result.operations_attempted;
    let injected = chaos_ops.load(Ordering::Relaxed);
    assert_eq!(
        attempted, injected,
        "runner op_count must match observed injections"
    );
    assert_eq!(
        result.faults_injected, injected,
        "prob=1.0 means every op faults"
    );

    assert!(
        storage_ops.load(Ordering::Relaxed) > 0,
        "storage workers must make progress"
    );
    assert!(
        anomaly_obs.load(Ordering::Relaxed) > 0,
        "anomaly workers must make progress"
    );

    // 停止后不再注入；检测器仍能生成报告（不 panic）。
    assert!(
        runner.maybe_inject("after_stop").is_none(),
        "stopped runner must not inject"
    );
    let _ = detector.generate_report();
}
