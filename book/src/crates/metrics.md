# sisyphus-metrics — Observability

Prometheus metrics instrumentation for all Sisyphus operations. Uses the standard `metrics` crate with `metrics-exporter-prometheus`.

## Metrics Dashboard

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `sisyphus_sop_executions_total` | Counter | `sop_id`, `status` | Total SOP executions |
| `sisyphus_sop_execution_duration_seconds` | Histogram | `sop_id` | SOP execution time |
| `sisyphus_wasm_executions_total` | Counter | `tool_id`, `status` | WASM tool invocations |
| `sisyphus_wasm_execution_duration_seconds` | Histogram | `tool_id` | WASM execution time |
| `sisyphus_mem_operations_total` | Counter | `operation`, `layer`, `status` | Cache operations (get/set) |
| `sisyphus_cognitive_iterations_total` | Counter | `sop_id`, `converged` | Cognitive loop iterations |
| `sisyphus_reflections_total` | Counter | `sop_id` | Reflection calls |
| `sisyphus_simulations_total` | Counter | `sop_id`, `succeeded` | Monte Carlo simulations |
| `sisyphus_evolutions_total` | Counter | `sop_id`, `evolved` | SOP evolution events |
| `sisyphus_bridge_requests_total` | Counter | `endpoint`, `status` | API request count |

## Usage

Call `init_metrics()` at application startup to initialize the Prometheus exporter on port 9100:

```rust
use sisyphus_metrics::init_metrics;

init_metrics();
// Metrics available at http://localhost:9100/metrics
```

Record operations using the typed record functions:

```rust
record_sop_execution("my_sop", true, 1.23);
record_wasm_execution("my_tool", true, 0.45, 1000);
record_cognitive_iteration("my_sop", 3, 0.05, true, 2.1);
```
