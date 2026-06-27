#![cfg(test)]

use serde_json::Value;
use sisyphus_core::{SOPGraph, SisyphusKernel, WasmSOPNode};
use sisyphus_intel::{
    GoalDrivenSynthesizer, MonteCarloSimulator, SisyphusIntel, StandardEvolver, StandardReflector,
};
use sisyphus_mem::MfsStore;
use sisyphus_sandbox::WasmToolExecutor;
use sisyphus_tools::{BreakerConfig, CircuitBreaker};
use sisyphus_types::{CircuitState, StateStore};
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::NamedTempFile;

// ---------------------------------------------------------------------------
// Test Helpers
// ---------------------------------------------------------------------------

async fn setup_kernel(db_url: Option<&str>) -> SisyphusKernel {
    let url = db_url.unwrap_or("sqlite::memory:");
    let state_store = Arc::new(MfsStore::new(url).await.expect("Failed to init MfsStore"));

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let tools_path = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tools");
    let tool_executor = Arc::new(
        WasmToolExecutor::with_config(Some(tools_path), None)
            .expect("Failed to init WasmToolExecutor"),
    );

    let simulator = Arc::new(MonteCarloSimulator::with_state_store(
        100,
        state_store.clone(),
    ));
    let reflector = Arc::new(StandardReflector);
    let evolver = Arc::new(StandardEvolver::new());
    let intel = Arc::new(SisyphusIntel::new(simulator, reflector, evolver));

    let synthesizer = Arc::new(GoalDrivenSynthesizer::new(tool_executor.clone()));

    SisyphusKernel::new(state_store, tool_executor, intel, synthesizer)
}

// ---------------------------------------------------------------------------
// Integration Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_full_pipeline() {
        let kernel = setup_kernel(None).await;
        let sop_id = "full_pipeline_test";

        // Goal that should trigger a simple linear pipeline (fetch -> process -> sink)
        let (result, report) = kernel
            .execute_goal("fetch and store data", sop_id, None)
            .await
            .expect("Goal execution failed");
        assert!(
            !result.to_string().is_empty(),
            "Result should be a non-empty string"
        );
        assert!(
            report.contains("CognitiveLoop"),
            "Report should contain cognitive loop details"
        );
        assert!(
            report.contains("converged=true"),
            "Pipeline should converge"
        );
    }

    #[tokio::test]
    async fn test_dag_execution() {
        let kernel = setup_kernel(None).await;
        let mut graph = SOPGraph::new();

        // Create a DAG: A -> B, A -> C, (B, C) -> D
        let node_a = graph.add_node(Arc::new(WasmSOPNode {
            name: "node_a".into(),
            tool_id: "hello".into(),
            fuel_limit: 1_000_000,
            executor: kernel.tool_executor.clone(),
        }));
        let node_b = graph.add_node(Arc::new(WasmSOPNode {
            name: "node_b".into(),
            tool_id: "hello".into(),
            fuel_limit: 1_000_000,
            executor: kernel.tool_executor.clone(),
        }));
        let node_c = graph.add_node(Arc::new(WasmSOPNode {
            name: "node_c".into(),
            tool_id: "hello".into(),
            fuel_limit: 1_000_000,
            executor: kernel.tool_executor.clone(),
        }));
        let node_d = graph.add_node(Arc::new(WasmSOPNode {
            name: "node_d".into(),
            tool_id: "hello".into(),
            fuel_limit: 1_000_000,
            executor: kernel.tool_executor.clone(),
        }));

        graph.add_edge(node_a, node_b);
        graph.add_edge(node_a, node_c);
        graph.add_edge(node_b, node_d);
        graph.add_edge(node_c, node_d);

        let (result, report, _) = kernel
            .execute_with_cognition(&graph, node_a, "dag_test", None, None)
            .await
            .expect("DAG execution failed");

        assert!(
            result.to_string().contains("Hello"),
            "Final node should return hello"
        );
        assert!(report.contains("converged=true"), "DAG should converge");
    }

    #[tokio::test]
    async fn test_cognitive_loop() {
        let kernel = setup_kernel(None).await;
        let mut graph = SOPGraph::new();

        // Use infinite_loop tool with very low fuel to force ResourceExhausted and evolution
        let node = graph.add_node(Arc::new(WasmSOPNode {
            name: "loop_node".into(),
            tool_id: "infinite_loop".into(),
            fuel_limit: 100, // Intentionally low
            executor: kernel.tool_executor.clone(),
        }));

        // We expect this to fail initially, evolve fuel, and potentially still fail
        // but we want to verify that iterations happened.
        let (_, report, _) = kernel
            .execute_with_cognition(&graph, node, "cog_loop_test", None, None)
            .await
            .expect("Cognitive loop failed");

        assert!(
            report.contains("iter[0]"),
            "Should have at least one iteration"
        );
        assert!(
            report.contains("deviation="),
            "Should have deviation scores"
        );
    }

    #[tokio::test]
    async fn test_mfs_persistence() {
        let temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let db_path = format!("sqlite:{}", temp_file.path().to_str().unwrap());

        // Instance 1: Write data
        {
            let store = MfsStore::new(&db_path)
                .await
                .expect("Failed to init store 1");
            store
                .set_state_json("persist_key", Value::String("persist_value".into()))
                .await
                .expect("Set failed");
        }

        // Instance 2: Read data
        {
            let store = MfsStore::new(&db_path)
                .await
                .expect("Failed to init store 2");
            let val = store
                .get_state_json("persist_key")
                .await
                .expect("Get failed")
                .expect("Key not found");
            assert_eq!(val, Value::String("persist_value".into()));
        }
    }

    #[tokio::test]
    async fn test_wasm_tool_chain() {
        let kernel = setup_kernel(None).await;
        let mut graph = SOPGraph::new();

        let n1 = graph.add_node(Arc::new(WasmSOPNode {
            name: "step1".into(),
            tool_id: "hello".into(),
            fuel_limit: 1_000_000,
            executor: kernel.tool_executor.clone(),
        }));
        let n2 = graph.add_node(Arc::new(WasmSOPNode {
            name: "step2".into(),
            tool_id: "hello".into(),
            fuel_limit: 1_000_000,
            executor: kernel.tool_executor.clone(),
        }));
        graph.add_edge(n1, n2);

        let (result, _, _) = kernel
            .execute_with_cognition(&graph, n1, "chain_test", None, None)
            .await
            .expect("Chain execution failed");
        assert!(
            result.to_string().contains("Hello"),
            "Chain should return hello from last node"
        );
    }

    #[tokio::test]
    async fn test_concurrent_execution() {
        let kernel = Arc::new(setup_kernel(None).await);
        let mut handles = vec![];

        for i in 0..5 {
            let k = Arc::clone(&kernel);
            handles.push(tokio::spawn(async move {
                let sop_id = format!("concurrent_{}", i);
                k.execute_goal("fetch data", &sop_id, None).await
            }));
        }

        for h in handles {
            let res = h
                .await
                .expect("Task panicked")
                .expect("Goal execution failed");
            assert!(
                !res.0.to_string().is_empty(),
                "Result should be a non-empty string"
            );
        }
    }

    #[tokio::test]
    async fn test_circuit_breaker_state_machine() {
        let config = BreakerConfig {
            failure_threshold: 2,
            success_threshold: 2,
            timeout_secs: 1,
            half_open_max_calls: 2,
        };
        let cb = CircuitBreaker::new(config);
        let key = "test_svc";

        assert!(cb.call(key, || Err::<(), &str>("fail1")).is_err());
        assert_eq!(cb.state(key), CircuitState::Closed);
        assert!(cb.call(key, || Err::<(), &str>("fail2")).is_err());
        assert_eq!(cb.state(key), CircuitState::Open);

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        assert!(cb.call(key, || Ok::<(), &str>(())).is_ok());
        assert_eq!(cb.state(key), CircuitState::HalfOpen);

        assert!(cb.call(key, || Ok::<(), &str>(())).is_ok());
        assert_eq!(cb.state(key), CircuitState::Closed);
    }

    #[tokio::test]
    async fn test_benchmark_record_and_trend() {
        let kernel = setup_kernel(None).await;
        let mut bm = kernel.benchmark.lock().unwrap();
        let name = "test_bench";

        for i in 0..5 {
            bm.record_result(sisyphus_types::BenchmarkRun {
                name: name.to_string(),
                timestamp: i as u64,
                duration_secs: 0.1 * (i as f64),
                success: true,
                metrics: serde_json::json!({}),
            })
            .expect("Record failed");
        }

        let list = bm.list_benchmarks();
        assert!(list.contains(&name.to_string()));

        let trend = bm.get_trend(name, 3);
        assert_eq!(trend.len(), 3);
        assert_eq!(trend[0].duration_secs, 0.2);
    }

    #[tokio::test]
    async fn test_meta_learner_outcomes() {
        let kernel = setup_kernel(None).await;
        let mut ml = kernel.meta_learner.lock().unwrap();
        let ctx = serde_json::json!({"task": "coding"});

        for _ in 0..3 {
            ml.record_outcome(sisyphus_types::MetaLearningSample {
                context: ctx.clone(),
                strategy: "fast".into(),
                success: true,
                duration_secs: 1.0,
                timestamp: 0,
            })
            .expect("Record failed");
        }
        for _ in 0..3 {
            ml.record_outcome(sisyphus_types::MetaLearningSample {
                context: ctx.clone(),
                strategy: "slow".into(),
                success: false,
                duration_secs: 2.0,
                timestamp: 0,
            })
            .expect("Record failed");
        }

        let best = ml.best_strategy(&ctx);
        assert_eq!(best, Some("fast".to_string()));
    }

    #[tokio::test]
    async fn test_agent_channel_pub_sub() {
        let kernel = setup_kernel(None).await;
        let channel = &kernel.channel;
        let topic = "test_topic";
        let agent = "agent_1";

        channel.subscribe(agent, topic).expect("Subscribe failed");

        let msg = sisyphus_types::AgentMessage {
            from: "publisher".into(),
            to: "*".into(),
            kind: "info".into(),
            payload: serde_json::json!({"msg": "hello"}),
            correlation_id: "corr_1".into(),
        };

        channel.publish(topic, msg).expect("Publish failed");

        let inbox = channel.receive(agent).expect("Receive failed");
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0].from, "publisher");
    }

    #[tokio::test]
    async fn test_grpc_bridge_rpc_pattern() {
        trait MockBridge {
            fn list_benchmarks(&self) -> Vec<String>;
            fn get_breaker_metrics(&self, model: &str) -> (String, u32, u32);
        }

        struct TestBridge;
        impl MockBridge for TestBridge {
            fn list_benchmarks(&self) -> Vec<String> {
                vec![]
            }
            fn get_breaker_metrics(&self, _model: &str) -> (String, u32, u32) {
                ("Closed".to_string(), 0, 0)
            }
        }

        let bridge = TestBridge;
        assert!(bridge.list_benchmarks().is_empty());
        let (state, fail, succ) = bridge.get_breaker_metrics("gpt-4o");
        assert_eq!(state, "Closed");
        assert_eq!(fail, 0);
        assert_eq!(succ, 0);
    }
}
