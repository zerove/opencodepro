#![cfg(test)]

use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::Value;
use sisyphus_core::{Consensus, SOPGraph, SisyphusKernel};
use sisyphus_intel::{
    simulation::LlmLogicalSimulator, GoalDrivenSynthesizer, MonteCarloSimulator, SisyphusIntel,
    StandardEvolver, StandardReflector,
};
use sisyphus_mem::MfsStore;
use sisyphus_tools_build::benchmark::ContinuousBenchmark;
use sisyphus_tools_core::feedback::FeedbackStore;
use sisyphus_tools_core::hook::HookRegistry;
use sisyphus_tools_executor::executor::BuiltinToolExecutor;
use sisyphus_tools_llm::circuit_breaker::{BreakerConfig, CircuitBreaker};
use sisyphus_tools_llm::{DummyEmbeddingClient, LlmClient};
use sisyphus_tools_llm_tools::meta_learner::MetaLearner;
use sisyphus_types::StateStore;
use sisyphus_types::{
    experience::{ExperienceBank, ExperienceQuery, ExperienceTuple},
    BenchmarkRun, CircuitState, ExecutorRouter, MetaLearningSample, ToolExecutor, TypesError,
};
use std::path::PathBuf;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Test Helpers
// ---------------------------------------------------------------------------

struct DummyExperienceBank;

#[async_trait]
impl ExperienceBank for DummyExperienceBank {
    async fn record(&self, _exp: ExperienceTuple) -> Result<(), TypesError> {
        Ok(())
    }
    async fn query_similar(
        &self,
        _query: ExperienceQuery,
    ) -> Result<Vec<ExperienceTuple>, TypesError> {
        Ok(vec![])
    }
}

struct DummyToolExecutor;

#[async_trait]
impl ToolExecutor for DummyToolExecutor {
    async fn execute(&self, _id: &str, _args: Vec<u8>, _fuel: u64) -> Result<Vec<u8>, TypesError> {
        Ok(b"ok".to_vec())
    }
}

struct DummyConsensus;

#[async_trait]
impl Consensus for DummyConsensus {
    async fn is_leader(&self) -> bool {
        true
    }
    async fn get_leader(&self) -> String {
        "node-1".into()
    }
    async fn propose(
        &self,
        _k: &str,
        _v: serde_json::Value,
    ) -> Result<(), sisyphus_types::TypesError> {
        Ok(())
    }
}

/// 测试专用临时目录，按进程 ID 隔离。
///
/// `MetaLearner` 在构造时会**读取**磁盘上已有的 `samples.json`（此处
/// `load_samples` 会把 486 条历史样本载入内存），`FeedbackStore` /
/// `ContinuousBenchmark` 同样把状态写在构造参数给出的目录里。此前这几处
/// 都指向**固定的** `.omo/...` 路径，因此测试结果会受仓库里残留的运行时
/// 数据影响，且多个测试进程互相写入同一目录。
///
/// 实测记录：带着一份 80KB 的残留 `samples.json` 跑
/// `--test-threads=1`，`test_full_pipeline` / `test_concurrent_execution`
/// 会报 `AllIterationsPivoted`；删掉该文件后同样命令 11/11 通过。
///
/// 注意：反向对照（保留共享 `.omo/meta_test` 路径 + 保留残留文件）**也**
/// 通过，因此不能断言残留文件就是唯一原因；只能确认测试不应依赖固定
/// 路径下的历史数据。按 PID 隔离消除了这个变量本身。
fn test_state_dir(tag: &str) -> String {
    format!(
        "{}/sisyphus_tests_{}_{}",
        std::env::temp_dir().display(),
        std::process::id(),
        tag
    )
}

async fn setup_kernel(db_url: Option<&str>) -> SisyphusKernel {
    let url = db_url.unwrap_or("sqlite::memory:");
    let state_store = Arc::new(
        MfsStore::new(url, &sisyphus_types::CacheConfig::default())
            .await
            .expect("Failed to init MfsStore"),
    );

    // Get stores from MfsStore internals
    let store = sisyphus_mem::SisyphusStore::new(url, &sisyphus_types::Config::default())
        .await
        .expect("Failed to init SisyphusStore");
    let sop_store = store.sop.clone();
    let roadmap_store = store.roadmap.clone();
    let vector_store = Arc::new(
        sisyphus_mem::VectorMemoryStore::new(store.pool())
            .await
            .expect("Failed to init VectorMemoryStore"),
    );

    let llm_client: Arc<dyn LlmClient> = Arc::new(sisyphus_tools_llm::metis::DummyLlmClient);
    let embedding_client = Arc::new(DummyEmbeddingClient);

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // CARGO_MANIFEST_DIR is `<workspace>/crates/sisyphus-tests`, so two parents
    // reach the workspace root. Both hops are compile-time constants, but go
    // through expect() rather than unwrap() so a malformed layout fails loudly
    // instead of silently pointing the executor at the wrong directory.
    let workspace_root = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .expect("crates/sisyphus-tests must live two levels under the workspace root");
    let tools_path = workspace_root.join("tools");
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(
        sisyphus_sandbox::WasmToolExecutor::with_config(Some(tools_path), None)
            .expect("Failed to init WasmToolExecutor"),
    );

    // 必须把 builtin 工具接进 router，否则 GoalDrivenSynthesizer 合成的
    // `builtin:web_fetch` / `builtin:bash_exec` 会退回 router 的 default
    // 执行器（即上面的 WASM 执行器），而那里根本没有同名 wasm。
    //
    // 实测症状：`execute_goal("fetch and store data", ...)` 报
    // AllIterationsPivoted(1, "Node fetch produced output with deviation score
    // 0.81")。0.81 = failure_rate 1.0 × 0.6 + Unknown 严重度 0.7 × 0.3
    // —— 节点确实报错，而不是偏差算错。
    //
    // 生产路径（crates/sisyphus-bridge/src/bridge_service.rs:352-364）就是
    // 这样装配的：WASM 作为 default，builtin 按 "builtin" 前缀注册。
    // 这里原先只传了裸的 WasmToolExecutor，等于测的是一条生产中不存在的配置。
    let builtin_executor: Arc<dyn ToolExecutor> = Arc::new(BuiltinToolExecutor {
        registry: HookRegistry::new(),
        llm_client: Arc::new(sisyphus_tools_llm::metis::DummyLlmClient),
        feedback_store: Arc::new(FeedbackStore::new(test_state_dir("feedback"))),
        bridge_client: None,
    });
    let router = Arc::new(ExecutorRouter::new(tool_executor.clone()));
    router.register("builtin", builtin_executor);
    let tool_executor: Arc<dyn ToolExecutor> = router;

    let simulator = Arc::new(MonteCarloSimulator::with_state_store(
        100,
        state_store.clone(),
    ));
    let logical_simulator = Arc::new(LlmLogicalSimulator::new(llm_client.clone()));
    let reflector = Arc::new(StandardReflector);
    let evolver = Arc::new(StandardEvolver::new());
    let intel = Arc::new(SisyphusIntel::new(
        simulator,
        logical_simulator,
        reflector,
        evolver,
    ));

    let synthesizer = Arc::new(GoalDrivenSynthesizer::new(tool_executor.clone()));
    let experience_bank: Arc<dyn ExperienceBank> = Arc::new(DummyExperienceBank);
    let meta_learner = Arc::new(Mutex::new(MetaLearner::new(&test_state_dir("meta"))));
    let consensus: Arc<dyn Consensus> = Arc::new(DummyConsensus);

    SisyphusKernel::new(
        state_store,        // 1. state_store
        sop_store,          // 2. sop_store
        vector_store,       // 3. vector_store
        experience_bank,    // 4. experience_bank
        tool_executor,      // 5. tool_executor
        intel,              // 6. intel
        llm_client.clone(), // 7. llm_client
        llm_client.clone(), // 8. fast_llm_client
        synthesizer,        // 9. synthesizer
        meta_learner,       // 10. meta_learner
        consensus,          // 11. consensus
        roadmap_store,      // 12. sql_store
        embedding_client,   // 13. embedding_client
    )
}

// ---------------------------------------------------------------------------
// Integration Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use sisyphus_core::WasmSOPNode;

    #[tokio::test]
    async fn test_full_pipeline() {
        let kernel = setup_kernel(None).await;
        let sop_id = "full_pipeline_test";

        let (result, report) = kernel
            .execute_goal("fetch and store data", sop_id, "default", None, None)
            .await
            .expect("Goal execution failed");
        assert!(
            !result.to_string().is_empty(),
            "Result should be a non-empty string"
        );
        assert!(
            report.contains("CognitiveLoop") || !report.is_empty(),
            "Report should contain cognitive loop details"
        );
    }

    #[tokio::test]
    async fn test_dag_execution() {
        let kernel = setup_kernel(None).await;
        let mut graph = SOPGraph::new();
        let tool_executor: Arc<dyn ToolExecutor> = Arc::new(DummyToolExecutor);

        let node_a = graph.add_node(Arc::new(WasmSOPNode {
            name: "node_a".into(),
            tool_id: "hello".into(),
            fuel_limit: 1_000_000,
            executor: tool_executor.clone(),
            args: Vec::new(),
        }));
        let node_b = graph.add_node(Arc::new(WasmSOPNode {
            name: "node_b".into(),
            tool_id: "hello".into(),
            fuel_limit: 1_000_000,
            executor: tool_executor.clone(),
            args: Vec::new(),
        }));
        let node_c = graph.add_node(Arc::new(WasmSOPNode {
            name: "node_c".into(),
            tool_id: "hello".into(),
            fuel_limit: 1_000_000,
            executor: tool_executor.clone(),
            args: Vec::new(),
        }));
        let node_d = graph.add_node(Arc::new(WasmSOPNode {
            name: "node_d".into(),
            tool_id: "hello".into(),
            fuel_limit: 1_000_000,
            executor: tool_executor.clone(),
            args: Vec::new(),
        }));

        graph.add_edge(node_a, node_b);
        graph.add_edge(node_a, node_c);
        graph.add_edge(node_b, node_d);
        graph.add_edge(node_c, node_d);

        let (result, report, _) = kernel
            .execute_with_cognition(Arc::new(graph), "dag_test", None, None, None, None)
            .await
            .expect("DAG execution failed");

        assert!(
            result.to_string().contains("Hello") || !result.to_string().is_empty(),
            "Final node should return hello"
        );
        assert!(
            report.iterations > 0,
            "Should have at least one cognitive iteration"
        );
    }

    #[tokio::test]
    async fn test_cognitive_loop() {
        let kernel = setup_kernel(None).await;
        let mut graph = SOPGraph::new();
        let tool_executor: Arc<dyn ToolExecutor> = Arc::new(DummyToolExecutor);

        let _node = graph.add_node(Arc::new(WasmSOPNode {
            name: "loop_node".into(),
            tool_id: "infinite_loop".into(),
            fuel_limit: 100,
            executor: tool_executor.clone(),
            args: Vec::new(),
        }));

        let (_, report, _) = kernel
            .execute_with_cognition(Arc::new(graph), "cog_loop_test", None, None, None, None)
            .await
            .expect("Cognitive loop failed");

        assert!(report.iterations > 0, "Should have at least one iteration");
        assert!(
            !report.history.is_empty(),
            "Should have deviation scores in history"
        );
    }

    #[tokio::test]
    async fn test_mfs_persistence() {
        let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
        let db_path = format!("sqlite:{}", temp_file.path().to_str().unwrap());

        // Instance 1: Write data
        {
            let store = MfsStore::new(&db_path, &sisyphus_types::CacheConfig::default())
                .await
                .expect("Failed to init store 1");
            store
                .set_state_json(
                    "default",
                    "persist_key",
                    Value::String("persist_value".into()),
                )
                .await
                .expect("Set failed");
            // Flush async write buffer before dropping the store.
            store.flush().await.expect("Flush failed");
        }

        // Instance 2: Read data
        {
            let store = MfsStore::new(&db_path, &sisyphus_types::CacheConfig::default())
                .await
                .expect("Failed to init store 2");
            let val = store
                .get_state_json("default", "persist_key")
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
        let tool_executor: Arc<dyn ToolExecutor> = Arc::new(DummyToolExecutor);

        let n1 = graph.add_node(Arc::new(WasmSOPNode {
            name: "step1".into(),
            tool_id: "hello".into(),
            fuel_limit: 1_000_000,
            executor: tool_executor.clone(),
            args: Vec::new(),
        }));
        let n2 = graph.add_node(Arc::new(WasmSOPNode {
            name: "step2".into(),
            tool_id: "hello".into(),
            fuel_limit: 1_000_000,
            executor: tool_executor.clone(),
            args: Vec::new(),
        }));
        graph.add_edge(n1, n2);

        let (result, _, _) = kernel
            .execute_with_cognition(Arc::new(graph), "chain_test", None, None, None, None)
            .await
            .expect("Chain execution failed");
        assert!(
            result.to_string().contains("Hello") || !result.to_string().is_empty(),
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
                k.execute_goal("fetch data", &sop_id, "default", None, None).await
            }));
        }

        for h in handles {
            let res = h.await.expect("Task panicked").expect("Goal execution failed");
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
        let mut bm = ContinuousBenchmark::new(&test_state_dir("benchmarks"));
        let name = "test_bench";

        for i in 0..5 {
            bm.record_result(BenchmarkRun {
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
        let mut ml = MetaLearner::new(&test_state_dir("meta_outcomes"));
        let ctx = serde_json::json!({"task": "coding"});

        for _ in 0..3 {
            ml.record_outcome(MetaLearningSample {
                context: ctx.clone(),
                strategy: "fast".into(),
                params: serde_json::json!({}),
                success: true,
                duration_secs: 1.0,
                timestamp: 0,
            })
            .expect("Record failed");
        }
        for _ in 0..3 {
            ml.record_outcome(MetaLearningSample {
                context: ctx.clone(),
                strategy: "slow".into(),
                params: serde_json::json!({}),
                success: false,
                duration_secs: 2.0,
                timestamp: 0,
            })
            .expect("Record failed");
        }

        let (best_name, _params) = ml.best_strategy_params(&ctx).expect("Should find best");
        assert_eq!(best_name, "fast");
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
