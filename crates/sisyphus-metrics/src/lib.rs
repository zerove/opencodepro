#![allow(clippy::cast_precision_loss)]
use metrics_exporter_prometheus::PrometheusBuilder;
use std::net::SocketAddr;
use thiserror::Error;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub mod observability;

/// Initialize metrics and tracing for the Sisyphus system.
///
/// `prometheus_port` controls the scrape endpoint listen port (default: 9100).
pub fn init_metrics(prometheus_port: u16) -> Result<(), MetricsError> {
    // Initialize Prometheus HTTP exporter on the configured port
    let addr: SocketAddr = ([0, 0, 0, 0], prometheus_port).into();
    PrometheusBuilder::new()
        .with_http_listener(addr)
        .install()
        .map_err(|_| MetricsError::PrometheusInstallFailed)?;

    // Initialize tracing with JSON formatting
    let fmt_layer =
        fmt::layer().json().with_target(true).with_thread_ids(true).with_thread_names(true);

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,sisyphus=debug"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .try_init()
        .map_err(|_| MetricsError::TracingInitFailed)?;

    Ok(())
}

/// Record a cognitive iteration metric.
///
/// Emits:
/// - `cognitive_loop_iterations_total` (counter, sop_id)
/// - `cognitive_loop_iteration_duration_seconds` (histogram, sop_id)
/// - `cognitive_loop_deviation` (gauge, sop_id)
/// - `cognitive_loop_convergence_total` (counter, sop_id; only if converged)
pub fn record_cognitive_iteration(
    sop_id: &str,
    _iteration: usize,
    deviation: f64,
    converged: bool,
    duration_secs: f64,
) {
    metrics::increment_counter!("cognitive_loop_iterations_total", "sop_id" => sop_id.to_string());
    metrics::histogram!(
        "cognitive_loop_iteration_duration_seconds",
        duration_secs,
        "sop_id" => sop_id.to_string()
    );
    metrics::gauge!("cognitive_loop_deviation", deviation, "sop_id" => sop_id.to_string());
    if converged {
        metrics::increment_counter!(
            "cognitive_loop_convergence_total",
            "sop_id" => sop_id.to_string()
        );
    }
}

/// Record SOP execution metric.
///
/// Emits:
/// - `sop_executions_total` (counter, sop_id/success)
/// - `sop_execution_duration_seconds` (histogram, sop_id)
pub fn record_sop_execution(sop_id: &str, success: bool, duration_secs: f64) {
    metrics::increment_counter!(
        "sop_executions_total",
        "sop_id" => sop_id.to_string(),
        "success" => success.to_string()
    );
    metrics::histogram!(
        "sop_execution_duration_seconds",
        duration_secs,
        "sop_id" => sop_id.to_string()
    );
}

/// Record WASM execution metric.
///
/// Emits:
/// - `wasm_executions_total` (counter, tool_id/success)
/// - `wasm_execution_duration_seconds` (histogram, tool_id)
/// - `wasm_fuel_used` (gauge, tool_id)
pub fn record_wasm_execution(tool_id: &str, success: bool, duration_secs: f64, fuel_used: u64) {
    metrics::increment_counter!(
        "wasm_executions_total",
        "tool_id" => tool_id.to_string(),
        "success" => success.to_string()
    );
    metrics::histogram!(
        "wasm_execution_duration_seconds",
        duration_secs,
        "tool_id" => tool_id.to_string()
    );
    metrics::gauge!("wasm_fuel_used", fuel_used as f64, "tool_id" => tool_id.to_string());
}

/// Record memory operation metric.
///
/// Emits:
/// - `mem_operations_total` (counter, op/layer/hit)
/// - `mem_operation_duration_seconds` (histogram, op/layer)
pub fn record_mem_operation(op: &str, layer: &str, hit: bool, duration_secs: f64) {
    metrics::increment_counter!(
        "mem_operations_total",
        "op" => op.to_string(),
        "layer" => layer.to_string(),
        "hit" => hit.to_string()
    );
    metrics::histogram!(
        "mem_operation_duration_seconds",
        duration_secs,
        "op" => op.to_string(),
        "layer" => layer.to_string()
    );
}

/// Record simulation metric.
///
/// Emits:
/// - `simulations_total` (counter, sop_id/success)
/// - `simulation_duration_seconds` (histogram, sop_id)
/// - `simulation_estimated_cost` (gauge, sop_id)
pub fn record_simulation(sop_id: &str, success: bool, duration_secs: f64, estimated_cost: u64) {
    metrics::increment_counter!(
        "simulations_total",
        "sop_id" => sop_id.to_string(),
        "success" => success.to_string()
    );
    metrics::histogram!(
        "simulation_duration_seconds",
        duration_secs,
        "sop_id" => sop_id.to_string()
    );
    metrics::gauge!(
        "simulation_estimated_cost",
        estimated_cost as f64,
        "sop_id" => sop_id.to_string()
    );
}

/// Record reflection metric.
///
/// Emits:
/// - `reflections_total` (counter, sop_id)
/// - `reflection_duration_seconds` (histogram, sop_id)
/// - `reflection_deviation` (gauge, sop_id)
pub fn record_reflection(sop_id: &str, deviation: f64, duration_secs: f64) {
    metrics::increment_counter!("reflections_total", "sop_id" => sop_id.to_string());
    metrics::histogram!(
        "reflection_duration_seconds",
        duration_secs,
        "sop_id" => sop_id.to_string()
    );
    metrics::gauge!("reflection_deviation", deviation, "sop_id" => sop_id.to_string());
}

/// Record evolution metric.
///
/// Emits:
/// - `evolutions_total` (counter, sop_id/evolved)
/// - `evolution_duration_seconds` (histogram, sop_id)
/// - `evolution_fuel_changes_total` (counter, sop_id; only if fuel_changed)
pub fn record_evolution(sop_id: &str, evolved: bool, duration_secs: f64, fuel_changed: bool) {
    metrics::increment_counter!(
        "evolutions_total",
        "sop_id" => sop_id.to_string(),
        "evolved" => evolved.to_string()
    );
    metrics::histogram!(
        "evolution_duration_seconds",
        duration_secs,
        "sop_id" => sop_id.to_string()
    );
    if fuel_changed {
        metrics::increment_counter!(
            "evolution_fuel_changes_total",
            "sop_id" => sop_id.to_string()
        );
    }
}

/// Record bridge request metric.
///
/// Emits:
/// - `bridge_requests_total` (counter, success)
/// - `bridge_request_duration_seconds` (histogram, no labels)
pub fn record_bridge_request(success: bool, duration_secs: f64) {
    metrics::increment_counter!("bridge_requests_total", "success" => success.to_string());
    metrics::histogram!("bridge_request_duration_seconds", duration_secs);
}

#[derive(Error, Debug)]
/// Metrics Error.
pub enum MetricsError {
    #[error("Prometheus exporter installation failed")]
    PrometheusInstallFailed,
    #[error("Tracing initialization failed")]
    TracingInitFailed,
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Smoke tests: record_* functions must not panic without a recorder ---
    // These functions are no-ops when no global metrics recorder is installed,
    // so we verify they execute without error.

    #[test]
    fn test_record_cognitive_iteration_smoke() {
        record_cognitive_iteration("test_sop", 1, 0.5, false, 1.0);
        record_cognitive_iteration("test_sop", 2, 0.1, true, 0.5);
    }

    #[test]
    fn test_record_cognitive_iteration_converged_smoke() {
        record_cognitive_iteration("test_sop", 3, 0.05, true, 0.3);
    }

    #[test]
    fn test_record_sop_execution_smoke() {
        record_sop_execution("test_sop", true, 2.0);
        record_sop_execution("test_sop", false, 5.0);
    }

    #[test]
    fn test_record_wasm_execution_smoke() {
        record_wasm_execution("tool_a", true, 1.0, 500);
        record_wasm_execution("tool_b", false, 3.0, 1000);
        record_wasm_execution("tool_c", true, 0.5, 0);
    }

    #[test]
    fn test_record_mem_operation_smoke() {
        record_mem_operation("get", "l1", true, 0.001);
        record_mem_operation("set", "l2", false, 0.002);
        record_mem_operation("scan", "disk", true, 0.1);
    }

    #[test]
    fn test_record_simulation_smoke() {
        record_simulation("sim_1", true, 10.0, 5000);
        record_simulation("sim_2", false, 20.0, 10_000);
    }

    #[test]
    fn test_record_reflection_smoke() {
        record_reflection("ref_1", 0.3, 5.0);
        record_reflection("ref_2", 0.0, 1.0);
    }

    #[test]
    fn test_record_evolution_smoke() {
        record_evolution("evo_1", true, 2.0, true);
        record_evolution("evo_2", false, 1.0, false);
        record_evolution("evo_3", true, 3.0, false);
    }

    #[test]
    fn test_record_bridge_request_smoke() {
        record_bridge_request(true, 0.5);
        record_bridge_request(false, 2.0);
    }

    #[test]
    fn test_metrics_error_display() {
        let err = MetricsError::PrometheusInstallFailed;
        assert_eq!(err.to_string(), "Prometheus exporter installation failed");

        let err = MetricsError::TracingInitFailed;
        assert_eq!(err.to_string(), "Tracing initialization failed");
    }

    #[test]
    fn test_metrics_error_debug() {
        let err = MetricsError::PrometheusInstallFailed;
        assert!(format!("{:?}", err).contains("PrometheusInstallFailed"));
    }
}
