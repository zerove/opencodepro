//! 轻量级 criterion bench：测试无需 sandbox 依赖的纯计算路径。
//! 与 `core_benches` 不同，此 bench 可以在 `cargo bench` 不带 --features sandbox 时跑，
//! 适合 CI 快速性能回归检查（每次 release PR 跑 < 30s）。
//!
//! 覆盖：
//! - AnomalyDetector 检测吞吐
//! - DagRenderer 大型 SOP 图谱渲染（10 / 50 / 200 / 500 节点）

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use sisyphus_core::AnomalyDetector;
use sisyphus_core::DagRenderer;
use sisyphus_core::NodeStatus;
use sisyphus_types::Config;

#[inline(always)]
fn black_box<T>(v: T) -> T {
    std::hint::black_box(v)
}

// ---------------------------------------------------------------------------
// AnomalyDetector 检测吞吐
// ---------------------------------------------------------------------------

fn bench_anomaly_detector(c: &mut Criterion) {
    let mut group = c.benchmark_group("anomaly_detector");
    let sizes = [10usize, 100, 1_000];
    for n in sizes.iter() {
        group.throughput(Throughput::Elements(*n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), n, |b, &n| {
            b.iter(|| {
                let detector = AnomalyDetector::default();
                for i in 0..n {
                    let node = format!("n{i}");
                    let _ = detector.observe(&node, 100, 50, true);
                }
            });
        });
    }
    group.finish();
}

// ---------------------------------------------------------------------------
// DagRenderer 大型 SOP 渲染
// ---------------------------------------------------------------------------

fn bench_dag_renderer(c: &mut Criterion) {
    let mut group = c.benchmark_group("dag_renderer");
    // 扩展到 5000 节点（TODO 8.1.1）：验证 DAG 渲染算法在大规模下的性能
    for n in [10usize, 50, 200, 500, 1000, 2000, 5000] {
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            let mut nodes: Vec<(String, NodeStatus)> = Vec::with_capacity(n);
            for i in 0..n {
                let status = match i % 4 {
                    0 => NodeStatus::Pending,
                    1 => NodeStatus::Running,
                    2 => NodeStatus::Completed,
                    _ => NodeStatus::Failed,
                };
                nodes.push((format!("n{i}"), status));
            }
            let mut edges: Vec<(String, String)> = Vec::with_capacity(n.saturating_sub(1) * 2);
            for i in 0..n.saturating_sub(1) {
                edges.push((format!("n{i}"), format!("n{}", i + 1)));
                if i % 5 == 0 && i + 2 < n {
                    edges.push((format!("n{i}"), format!("n{}", i + 2)));
                }
            }
            let renderer = DagRenderer::new();
            b.iter(|| {
                let viz = renderer.render(black_box(&nodes), black_box(&edges));
                let _ = viz.width * viz.height;
            });
        });
    }
    group.finish();
}

// ---------------------------------------------------------------------------
// AnomalyDetector 边界场景
// ---------------------------------------------------------------------------

fn bench_anomaly_detector_boundaries(c: &mut Criterion) {
    let mut group = c.benchmark_group("anomaly_detector_boundaries");
    group.bench_function("observe_1000", |b| {
        let detector = AnomalyDetector::default();
        b.iter(|| {
            for i in 0..1000usize {
                let node = format!("n{i}");
                let _ = detector.observe(black_box(&node), 100, 50, true);
            }
        });
    });
    group.finish();
}

// ---------------------------------------------------------------------------
// CLI startup 模拟（纯 Config 加载，不含 clap / dynamic loader）
// ---------------------------------------------------------------------------

/// 模拟 CLI 启动中"sisyphus_types::Config 构造"这一耗时阶段。
///
/// 真实启动耗时数据见 `docs/benchmarks/cli-startup-v0.2.0-baseline.md`：
/// release 中位数 10ms，debug 中位数 22ms（包含 fork + dylib + clap）。
///
/// 本 bench 仅隔离 Config 部分（CI 可追踪回归），不模拟进程 fork。
fn bench_cli_config_default(c: &mut Criterion) {
    let mut group = c.benchmark_group("cli_startup");
    group.bench_function("config_default", |b| {
        b.iter(|| {
            let cfg = Config::default();
            let _ = black_box(&cfg);
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_anomaly_detector,
    bench_anomaly_detector_boundaries,
    bench_dag_renderer,
    bench_cli_config_default
);
criterion_main!(benches);
