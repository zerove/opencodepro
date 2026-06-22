# sisyphus-metrics — Observability

Prometheus metrics + `tracing` structured logging for all Sisyphus operations.

## Initialization

Call once at startup with the desired Prometheus scrape port:

```rust
use sisyphus_metrics::init_metrics;

// Expose metrics on http://0.0.0.0:9100/metrics
init_metrics(9100)?;
```

Configures:
- `metrics-exporter-prometheus` with HTTP listener on the specified port
- `tracing-subscriber` with JSON formatting and `SISYPHUS_LOG` / `RUST_LOG` env filter support
- Default log level: `info`, sisyphus crates: `debug`

## Metrics Dashboard

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `sisyphus_sop_executions_total` | Counter | `sop_id`, `status` | Total SOP executions |
| `sisyphus_sop_execution_duration_seconds` | Histogram | `sop_id` | SOP execution duration |
| `sisyphus_wasm_executions_total` | Counter | `tool_id`, `status` | WASM tool invocations |
| `sisyphus_wasm_execution_duration_seconds` | Histogram | `tool_id` | WASM execution duration |
| `sisyphus_mem_operations_total` | Counter | `operation`, `layer`, `status` | Cache get/set operations |
| `sisyphus_cognitive_iterations_total` | Counter | `sop_id`, `converged` | Cognitive loop iterations |
| `sisyphus_reflections_total` | Counter | `sop_id` | Reflection invocations |
| `sisyphus_simulations_total` | Counter | `sop_id`, `succeeded` | Monte Carlo simulations |
| `sisyphus_evolutions_total` | Counter | `sop_id`, `evolved` | SOP evolution events |
| `sisyphus_bridge_requests_total` | Counter | `success` | gRPC request count |

## Recording

Use the typed record functions:

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

## Error Handling

```rust
pub enum MetricsError {
    #[error("Failed to install Prometheus exporter")]
    PrometheusInstallFailed,
    #[error("Failed to initialize tracing subscriber")]
    TracingInitFailed,
}
```
