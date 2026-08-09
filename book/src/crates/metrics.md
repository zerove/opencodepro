# sisyphus-metrics — 可观测性

所有 Sisyphus 操作的 Prometheus 指标 + `tracing` 结构化日志。

## 初始化

在启动时调用一次，传入所需的 Prometheus 抓取端口：

```rust
use sisyphus_metrics::init_metrics;

// 在 http://0.0.0.0:9100/metrics 暴露指标
init_metrics(9100)?;
```

配置：
- `metrics-exporter-prometheus` 在指定端口监听 HTTP
- `tracing-subscriber` 支持 JSON 格式与 `SISYPHUS_LOG` / `RUST_LOG` 环境变量过滤
- 默认日志级别：`info`，sisyphus crates：`debug`

## 指标仪表盘

| 指标 | 类型 | 标签 | 说明 |
|--------|------|--------|-------------|
| `sisyphus_sop_executions_total` | Counter | `sop_id`, `status` | SOP 执行总次数 |
| `sisyphus_sop_execution_duration_seconds` | Histogram | `sop_id` | SOP 执行时长 |
| `sisyphus_wasm_executions_total` | Counter | `tool_id`, `status` | WASM 工具调用次数 |
| `sisyphus_wasm_execution_duration_seconds` | Histogram | `tool_id` | WASM 执行时长 |
| `sisyphus_mem_operations_total` | Counter | `operation`, `layer`, `status` | 缓存 get/set 操作 |
| `sisyphus_cognitive_iterations_total` | Counter | `sop_id`, `converged` | 认知循环迭代次数 |
| `sisyphus_reflections_total` | Counter | `sop_id` | 反思调用次数 |
| `sisyphus_simulations_total` | Counter | `sop_id`, `succeeded` | 蒙特卡洛模拟次数 |
| `sisyphus_evolutions_total` | Counter | `sop_id`, `evolved` | SOP 演进事件 |
| `sisyphus_bridge_requests_total` | Counter | `success` | gRPC 请求计数 |

## 记录

使用类型化记录函数：

```rust
record_sop_execution("my_sop", true, 1.23);
record_wasm_execution("my_tool", true, 0.45, 1000);
record_cognitive_iteration("my_sop", 3, 0.05, true, 2.1);
record_bridge_request(true, 1.23);
record_mem_operation("get", "l1", "hit", 0.001);
record_simulation("my_sop", true, 0.05);
record_evolution("my_sop", true, 0.05);
record_reflection("my_sop", 0.05);
```

## 错误处理

```rust
pub enum MetricsError {
    #[error("Failed to install Prometheus exporter")]
    PrometheusInstallFailed,
    #[error("Failed to initialize tracing subscriber")]
    TracingInitFailed,
}
```