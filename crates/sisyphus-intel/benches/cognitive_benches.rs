use async_trait::async_trait;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use sisyphus_intel::*;
use sisyphus_types::{ExecutionContext, SOPGraph, ToolExecutor, WasmSOPNode};
use std::sync::Arc;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Mock executor for benchmarks
// ---------------------------------------------------------------------------
struct BenchExecutor;

#[async_trait]
impl ToolExecutor for BenchExecutor {
    async fn execute(
        &self,
        _tool_id: &str,
        _args: Vec<u8>,
        _fuel_limit: u64,
    ) -> Result<Vec<u8>, sisyphus_types::TypesError> {
        Ok(b"bench-output".to_vec())
    }
}

// ---------------------------------------------------------------------------
// Synthesizer benchmark
// ---------------------------------------------------------------------------
fn bench_synthesis(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let executor = Arc::new(BenchExecutor);
    let synthesizer = GoalDrivenSynthesizer::new(executor);

    let mut group = c.benchmark_group("synthesis");
    group.measurement_time(Duration::from_secs(5));
    group.sample_size(50);

    group.bench_function("single_action", |b| {
        b.to_async(&rt).iter(|| async {
            let ctx = ExecutionContext::new();
            synthesizer
                .synthesize(
                    black_box("fetch data"),
                    black_box(&ctx),
                    black_box("bench"),
                    black_box(sisyphus_types::WorkMode::Standard),
                    None,
                )
                .await
        })
    });

    group.bench_function("linear_pipeline", |b| {
        b.to_async(&rt).iter(|| async {
            let ctx = ExecutionContext::new();
            synthesizer
                .synthesize(
                    black_box("fetch process store"),
                    black_box(&ctx),
                    black_box("bench"),
                    black_box(sisyphus_types::WorkMode::Standard),
                    None,
                )
                .await
        })
    });

    group.bench_function("parallel_fan_out", |b| {
        b.to_async(&rt).iter(|| async {
            let ctx = ExecutionContext::new();
            synthesizer
                .synthesize(
                    black_box("fetch and process parallel"),
                    black_box(&ctx),
                    black_box("bench"),
                    black_box(sisyphus_types::WorkMode::Standard),
                    None,
                )
                .await
        })
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Simulator benchmark
// ---------------------------------------------------------------------------
fn bench_simulation(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let executor = Arc::new(BenchExecutor);

    // Build a multi-node graph
    let mut graph = SOPGraph::new();
    let nodes: Vec<_> = (0..10)
        .map(|i| {
            graph.add_node(Arc::new(WasmSOPNode {
                name: format!("node_{}", i),
                tool_id: "fast_tool".into(),
                fuel_limit: 1_000_000,
                executor: executor.clone(),
            }))
        })
        .collect();
    // Chain them linearly
    for i in 0..9 {
        graph.add_edge(nodes[i], nodes[i + 1]);
    }

    let sim = MonteCarloSimulator::new(1000);

    let mut group = c.benchmark_group("simulation");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(30);

    group.bench_function("mc_1000_iter", |b| {
        b.to_async(&rt).iter(|| async {
            let ctx = ExecutionContext::new();
            sim.simulate(
                black_box(&graph),
                black_box(&ctx),
                black_box("bench"),
                black_box("bench_sop"),
                None,
            )
            .await
        })
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Reflection benchmark
// ---------------------------------------------------------------------------
fn bench_reflection(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let reflector = StandardReflector;

    // Build a trace with mixed results
    let trace: String = (0..100)
        .map(|i| {
            let status = if i % 10 == 0 { "error" } else { "ok" };
            let output = if i % 10 == 0 {
                "ResourceExhausted: fuel"
            } else {
                "completed"
            };
            format!(
                r#"{{"node":"n{}","status":"{}","output":"{}"}}"#,
                i, status, output
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let prediction = SimulationResult {
        path_id: "bench".into(),
        success_probability: 0.7,
        estimated_cost: 500,
        predicted_stability: 0.8,
        predicted_effort: 100,
        predicted_outcome: "mixed".into(),
    };

    let mut group = c.benchmark_group("reflection");
    group.measurement_time(Duration::from_secs(5));
    group.sample_size(50);

    group.bench_function("reflect_100_nodes", |b| {
        b.to_async(&rt).iter(|| async {
            reflector
                .reflect(black_box(&trace), black_box(&prediction), None)
                .await
        })
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Evolution benchmark
// ---------------------------------------------------------------------------
fn bench_evolution(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let executor = Arc::new(BenchExecutor);

    // Build a 20-node graph
    let mut graph = SOPGraph::new();
    let nodes: Vec<_> = (0..20)
        .map(|i| {
            graph.add_node(Arc::new(WasmSOPNode {
                name: format!("task_{}", i),
                tool_id: "fast_tool".into(),
                fuel_limit: 1_000_000,
                executor: executor.clone(),
            }))
        })
        .collect();
    for i in 0..19 {
        graph.add_edge(nodes[i], nodes[i + 1]);
    }

    let report = ReflectionReport {
        original_sop_id: "bench_sop".into(),
        deviation_score: 0.6,
        suggested_improvements: vec![
            "Resource exhaustion — increase fuel or split long-running nodes".into(),
            "Add retry logic to unstable nodes".into(),
        ],
        evolved_sop_structure: None,
        records: vec![],
    };

    let prediction = SimulationResult {
        path_id: "bench".into(),
        success_probability: 0.5,
        estimated_cost: 2_000_000,
        predicted_stability: 0.4,
        predicted_effort: 500,
        predicted_outcome: "needs_fuel".into(),
    };

    let mut group = c.benchmark_group("evolution");
    group.measurement_time(Duration::from_secs(5));
    group.sample_size(50);

    // StandardEvolver with default SimpleFuelPricer
    group.bench_function("evolve_simple_pricer", |b| {
        b.to_async(&rt).iter(|| async {
            let evolver = StandardEvolver::new();
            evolver
                .evolve(
                    black_box(&graph),
                    black_box(&report),
                    black_box(&prediction),
                    None,
                )
                .await
        })
    });

    // StandardEvolver with AdaptiveFuelPricer
    group.bench_function("evolve_adaptive_pricer", |b| {
        b.to_async(&rt).iter(|| async {
            let evolver = StandardEvolver::with_fuel_pricer(Arc::new(AdaptiveFuelPricer::new()));
            evolver
                .evolve(
                    black_box(&graph),
                    black_box(&report),
                    black_box(&prediction),
                    None,
                )
                .await
        })
    });

    // Evolution with no-op (low deviation, no suggestions triggered)
    let clean_report = ReflectionReport {
        original_sop_id: "bench_sop".into(),
        deviation_score: 0.1,
        suggested_improvements: vec!["SOP is performing within expected parameters".into()],
        evolved_sop_structure: None,
    };

    group.bench_function("evolve_noop", |b| {
        b.to_async(&rt).iter(|| async {
            let evolver = StandardEvolver::new();
            evolver
                .evolve(
                    black_box(&graph),
                    black_box(&clean_report),
                    black_box(&prediction),
                    None,
                )
                .await
        })
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// ErrorCategory benchmark
// ---------------------------------------------------------------------------
fn bench_error_category(c: &mut Criterion) {
    let mut group = c.benchmark_group("error_category");
    group.measurement_time(Duration::from_secs(3));
    group.sample_size(100);

    group.bench_function("classify_resource_exhausted", |b| {
        b.iter(|| {
            ErrorCategory::classify(black_box(
                "ResourceExhausted: fuel limit exceeded at module",
            ))
        })
    });

    group.bench_function("classify_timeout", |b| {
        b.iter(|| ErrorCategory::classify(black_box("timeout after 30 seconds of execution")))
    });

    group.bench_function("classify_unknown", |b| {
        b.iter(|| ErrorCategory::classify(black_box("random error message without keywords")))
    });

    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default().warm_up_time(Duration::from_secs(2));
    targets = bench_synthesis, bench_simulation, bench_reflection, bench_evolution, bench_error_category
);
criterion_main!(benches);
