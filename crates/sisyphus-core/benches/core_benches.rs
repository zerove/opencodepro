use async_trait::async_trait;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use sisyphus_core::{AdaptiveDepthController, AnomalyDetector, Executor, StateStore};
use sisyphus_mem::MfsStore;
use sisyphus_sandbox::WasmToolExecutor;
use sisyphus_types::{SOPGraph, ToolExecutor, WasmSOPNode};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

fn make_executor() -> Executor {
    let llm_client = Arc::new(sisyphus_tools_llm::DummyLlmClient);
    let intervention_manager = Arc::new(sisyphus_core::InterventionManager::new());
    let knowledge_manager = Arc::new(sisyphus_knowledge::AssetManager::new(
        std::path::PathBuf::from("/tmp/sisyphus_bench"),
    ));
    let internalization_manager = Arc::new(sisyphus_core::InternalizationManager::new(
        llm_client,
        knowledge_manager,
    ));
    Executor::new(
        Arc::new(sisyphus_tools_executor::executor::test_cmds::TestExecutor::new()),
        intervention_manager,
        internalization_manager,
        None,
    )
}

// ---------------------------------------------------------------------------
// Mock executor for DAG benchmarks
// ---------------------------------------------------------------------------
struct MockToolExecutor;

#[async_trait]
impl ToolExecutor for MockToolExecutor {
    async fn execute(
        &self,
        _tool_id: &str,
        _args: Vec<u8>,
        _fuel_limit: u64,
    ) -> Result<Vec<u8>, sisyphus_types::TypesError> {
        Ok(b"mock-output".to_vec())
    }
}

// ---------------------------------------------------------------------------
// DAG Execution Benchmarks
// ---------------------------------------------------------------------------
fn bench_dag_execution(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let executor = make_executor();
    let tool_executor = Arc::new(MockToolExecutor);

    let mut group = c.benchmark_group("dag_execution");
    group.measurement_time(Duration::from_secs(5));
    group.sample_size(50);

    for n in [5, 10, 20] {
        group.bench_with_input(format!("nodes_{}", n), &n, |b, &nodes_count| {
            b.to_async(&rt).iter(|| async {
                let mut graph = SOPGraph::new();
                let mut prev_node = None;

                for i in 0..nodes_count {
                    let node = graph.add_node(Arc::new(WasmSOPNode {
                        name: format!("node_{}", i),
                        tool_id: "mock_tool".into(),
                        fuel_limit: 1_000_000,
                        executor: tool_executor.clone(),
                    }));
                    if let Some(prev) = prev_node {
                        graph.add_edge(prev, node);
                    }
                    prev_node = Some(node);
                }

                let graph_arc = Arc::new(graph);
                let start_node = graph_arc.graph.node_indices().next().unwrap();
                executor
                    .execute_with_context(
                        black_box(graph_arc),
                        start_node,
                        None,
                        None,
                        tool_executor.clone(),
                        None,
                        None,
                        None,
                    )
                    .await
            })
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// WASM Execution Benchmarks
// ---------------------------------------------------------------------------
fn bench_wasm_execution(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let executor = make_executor();

    // Setup WasmToolExecutor pointing to workspace root tools/
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let tools_path = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tools");
    let tool_executor = Arc::new(
        WasmToolExecutor::with_config(Some(tools_path), None)
            .expect("Failed to create WasmToolExecutor"),
    );

    let mut graph = SOPGraph::new();
    let start_node = graph.add_node(Arc::new(WasmSOPNode {
        name: "wasm_node".into(),
        tool_id: "hello".into(),
        fuel_limit: 1_000_000,
        executor: tool_executor.clone(),
    }));
    let graph_arc = Arc::new(graph);

    let mut group = c.benchmark_group("wasm_execution");
    group.measurement_time(Duration::from_secs(5));
    group.sample_size(50);

    group.bench_function("single_node_hello", |b| {
        b.to_async(&rt).iter(|| async {
            executor
                .execute_with_context(
                    black_box(graph_arc.clone()),
                    start_node,
                    None,
                    None,
                    tool_executor.clone(),
                    None,
                    None,
                    None,
                )
                .await
        })
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// MFS Storage Benchmarks
// ---------------------------------------------------------------------------
fn bench_mfs_storage(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut group = c.benchmark_group("mfs_storage");
    group.measurement_time(Duration::from_secs(5));
    group.sample_size(50);

    let payloads = [
        ("small", 100),
        ("medium_10kb", 10 * 1024),
        ("large_100kb", 100 * 1024),
    ];

    for (name, size) in payloads {
        let payload_str = "a".repeat(size);
        let payload_val = serde_json::Value::String(payload_str);
        let key = format!("bench_key_{}", name);

        // Reuse a single store for set benchmarks to avoid creating many in-memory DBs
        let store = rt.block_on(async {
            MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
                .await
                .unwrap()
        });
        let store_arc = Arc::new(store);

        group.bench_function(format!("set_{}", name), |b| {
            let mut counter = 0u64;
            b.to_async(&rt).iter(|| {
                let store = store_arc.clone();
                let key = key.clone();
                let payload = payload_val.clone();
                let idx = counter;
                counter += 1;
                async move {
                    let iter_key = format!("{}_{}", key, idx);
                    store
                        .set_state_json(
                            black_box("bench_project"),
                            black_box(&iter_key),
                            black_box(payload),
                        )
                        .await
                        .unwrap();
                }
            })
        });

        // Pre-populate store for get benchmark
        let store = rt.block_on(async {
            let s = MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
                .await
                .unwrap();
            s.set_state_json("bench_project", &key, payload_val.clone())
                .await
                .unwrap();
            s
        });
        let store_arc = Arc::new(store);

        group.bench_function(format!("get_{}", name), |b| {
            b.to_async(&rt).iter(|| async {
                store_arc
                    .get_state_json(black_box("bench_project"), black_box(&key))
                    .await
                    .unwrap()
            })
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Cognitive Loop V2 Benchmarks
// ---------------------------------------------------------------------------

fn bench_v2_adaptive_depth_controller(c: &mut Criterion) {
    let mut group = c.benchmark_group("v2_adaptive_depth");
    group.measurement_time(Duration::from_secs(5));
    group.sample_size(50);

    // Simulate convergence under different noise profiles
    for n_iter in [5u64, 10, 20] {
        group.bench_with_input(
            BenchmarkId::new("convergence_ramp", n_iter),
            &n_iter,
            |b, &n| {
                b.iter(|| {
                    let mut controller = AdaptiveDepthController::new(
                        black_box(0.95), // target_confidence
                        black_box(0.01), // min_improvement
                        black_box(20),   // max_iterations
                        black_box(3),    // min_iterations
                    );
                    for i in 0..n {
                        // Simulate increasing confidence (1.0 plateau) with noise
                        let confidence =
                            (i as f64 / n as f64).min(1.0) + (i as f64 * 0.01).sin() * 0.05;
                        let _ = controller.observe_iteration(black_box(confidence));
                    }
                    controller
                })
            },
        );
    }

    // Plateau detection benchmark
    group.bench_function("plateau_early_stop", |b| {
        b.iter(|| {
            let mut controller = AdaptiveDepthController::new(black_box(0.95), 0.01, 20, 5);
            // 10 iterations of flat 0.92 confidence → simulated plateau
            for _ in 0..10 {
                let _ = controller.observe_iteration(black_box(0.92));
            }
            controller
        })
    });

    group.finish();
}

fn bench_v2_anomaly_detector(c: &mut Criterion) {
    let mut group = c.benchmark_group("v2_anomaly_detector");
    group.measurement_time(Duration::from_secs(5));
    group.sample_size(50);

    for n_obs in [100u64, 1_000, 10_000] {
        group.bench_with_input(BenchmarkId::new("observe_bulk", n_obs), &n_obs, |b, &n| {
            b.iter(|| {
                let detector = AnomalyDetector::default();
                for i in 0..n {
                    let node = format!("node_{}", i % 10);
                    let fuel = 100 + (i * 7) % 50; // moderate variance
                    let dur = 50 + (i * 3) % 20;
                    let success = i % 20 != 0; // 5% fail rate
                    detector.observe(
                        black_box(&node),
                        black_box(fuel),
                        black_box(dur),
                        black_box(success),
                    );
                }
                drop(detector);
            })
        });
    }

    group.bench_function("report_generation", |b| {
        b.iter(|| {
            let detector = AnomalyDetector::default();
            for i in 0..1000 {
                let node = format!("node_{}", i % 10);
                detector.observe(&node, 100 + (i * 7) % 50, 50 + (i * 3) % 20, true);
            }
            black_box(detector.generate_report())
        })
    });

    group.finish();
}

criterion_group!(
    name = core_benches;
    config = Criterion::default().warm_up_time(Duration::from_secs(2));
    targets = bench_dag_execution, bench_wasm_execution, bench_mfs_storage,
              bench_v2_adaptive_depth_controller, bench_v2_anomaly_detector,

);
criterion_main!(core_benches);
