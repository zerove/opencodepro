//! Observability layer: OpenTelemetry tracing, span helpers, and tracing integration.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use sisyphus_metrics::observability::{init_tracing, TraceConfig};
//!
//! #[tokio::main]
//! async fn main() {
//!     init_tracing(TraceConfig::default()).unwrap();
//!     // ... rest of app
//! }
//! ```
//!
//! With OTLP export (requires `otel` feature):
//! ```rust,ignore
//! use sisyphus_metrics::observability::{init_tracing_with_otlp, TraceConfig};
//!
//! let config = TraceConfig {
//!     otlp_endpoint: Some("http://localhost:4317".to_string()),
//!     service_name: "sisyphus-engine".to_string(),
//!     ..Default::default()
//! };
//! init_tracing_with_otlp(config).unwrap();
//! ```

use metrics::Label;
use thiserror::Error;
use tracing::{info_span, Span};

/// Tracing configuration.
#[derive(Debug, Clone)]
pub struct TraceConfig {
    /// Service name for OpenTelemetry traces.
    pub service_name: String,
    /// OTLP endpoint (e.g., "http://localhost:4317").
    /// If None, only stdout JSON tracing is used.
    pub otlp_endpoint: Option<String>,
    /// Log level filter. Default: "info,sisyphus=debug"
    pub filter: String,
    /// Whether to output JSON-formatted traces to stdout.
    pub json_stdout: bool,
}

impl Default for TraceConfig {
    fn default() -> Self {
        Self {
            service_name: "sisyphus-engine".to_string(),
            otlp_endpoint: None,
            filter: "info,sisyphus=debug".to_string(),
            json_stdout: true,
        }
    }
}

/// Initialize tracing with stdout JSON output.
/// For OTLP export, use [`init_tracing_with_otlp`] instead.
pub fn init_tracing(config: TraceConfig) -> Result<(), ObservabilityError> {
    use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

    let env_filter = EnvFilter::try_new(&config.filter)
        .map_err(|e| ObservabilityError::FilterError(e.to_string()))?;

    let subscriber = tracing_subscriber::registry().with(env_filter);

    if config.json_stdout {
        let fmt_layer =
            fmt::layer().json().with_target(true).with_thread_ids(true).with_thread_names(true);
        subscriber.with(fmt_layer).try_init()?;
    } else {
        let fmt_layer = fmt::layer().with_target(true);
        subscriber.with(fmt_layer).try_init()?;
    }

    Ok(())
}

/// Initialize tracing with OpenTelemetry OTLP export.
/// Requires the `otel` feature.
#[cfg(feature = "otel")]
pub fn init_tracing_with_otlp(config: TraceConfig) -> Result<(), ObservabilityError> {
    use opentelemetry::trace::TracerProvider as _;
    use opentelemetry::KeyValue;
    use opentelemetry_otlp::WithExportConfig;
    use opentelemetry_sdk::{trace as sdk_trace, Resource};
    use opentelemetry_semantic_conventions::resource::SERVICE_NAME;
    use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

    let env_filter = EnvFilter::try_new(&config.filter)
        .map_err(|e| ObservabilityError::FilterError(e.to_string()))?;

    // Configure OTLP exporter
    let endpoint = config.otlp_endpoint.unwrap_or_else(|| "http://localhost:4317".to_string());

    let otlp_exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()
        .map_err(|e| ObservabilityError::OtlpError(e.to_string()))?;

    let provider = sdk_trace::SdkTracerProvider::builder()
        .with_batch_exporter(otlp_exporter)
        .with_resource(
            Resource::builder()
                .with_attributes([KeyValue::new(SERVICE_NAME, config.service_name.clone())])
                .build(),
        )
        .build();

    let tracer = provider.tracer(config.service_name);

    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

    let subscriber = tracing_subscriber::registry().with(env_filter).with(otel_layer);

    if config.json_stdout {
        let fmt_layer =
            fmt::layer().json().with_target(true).with_thread_ids(true).with_thread_names(true);
        subscriber.with(fmt_layer).try_init()?;
    } else {
        subscriber.try_init()?;
    }

    Ok(())
}

// ─── Span Helpers ─────────────────────────────────────────────────────

/// Create a tracing span for a cognitive loop iteration.
pub fn cognitive_loop_span(sop_id: &str, iteration: usize) -> Span {
    info_span!("cognitive_loop", sop_id = sop_id, iteration = iteration,)
}

/// Create a tracing span for DAG node execution.
pub fn node_execution_span(node_name: &str, sop_id: &str) -> Span {
    info_span!("node_execution", node_name = node_name, sop_id = sop_id,)
}

/// Create a tracing span for reflection phase.
pub fn reflection_span(sop_id: &str, iteration: usize) -> Span {
    info_span!("reflection", sop_id = sop_id, iteration = iteration,)
}

/// Create a tracing span for evolution phase.
pub fn evolution_span(sop_id: &str, iteration: usize) -> Span {
    info_span!("evolution", sop_id = sop_id, iteration = iteration,)
}

/// Create a tracing span for memory access.
pub fn memory_span(op: &str, layer: &str) -> Span {
    info_span!("memory_access", operation = op, layer = layer,)
}

/// Create a tracing span for tool execution.
pub fn tool_span(tool_name: &str) -> Span {
    info_span!("tool_execution", tool = tool_name,)
}

// ─── Metrics Label Helpers ────────────────────────────────────────────

/// Create label pairs for metrics.
pub fn sop_labels(sop_id: &str) -> Vec<Label> {
    vec![Label::new("sop_id", sop_id.to_string())]
}

pub fn sop_success_labels(sop_id: &str, success: bool) -> Vec<Label> {
    vec![
        Label::new("sop_id", sop_id.to_string()),
        Label::new("success", success.to_string()),
    ]
}

/// Mem op labels.
pub fn mem_op_labels(op: &str, layer: &str, hit: bool) -> Vec<Label> {
    vec![
        Label::new("op", op.to_string()),
        Label::new("layer", layer.to_string()),
        Label::new("hit", hit.to_string()),
    ]
}

// ─── Error type ───────────────────────────────────────────────────────

#[derive(Error, Debug)]
/// Observability Error.
pub enum ObservabilityError {
    #[error("Tracing filter error: {0}")]
    FilterError(String),

    #[error("OTLP exporter error: {0}")]
    OtlpError(String),

    #[error("Tracing subscriber init failed")]
    TracingInitFailed,
}

impl From<tracing_subscriber::util::TryInitError> for ObservabilityError {
    fn from(_: tracing_subscriber::util::TryInitError) -> Self {
        Self::TracingInitFailed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 在作用域内安装一个最小 registry 订阅者，然后校验 span 的元数据。
    ///
    /// `Span::metadata()` 依赖当前 dispatcher：进程内没有任何订阅者时，
    /// `tracing` 直接返回 `Span::none()`，断言必然拿到 `None`。这批用例原先
    /// 依赖「别的测试恰好装上了进程级全局订阅者」——单独跑
    /// `cargo test -p sisyphus-metrics` 6 个全失败，混在 `cargo test-all` 里
    /// 却全绿。测试的正确性不能取决于同进程里还有谁在跑。
    ///
    /// 这里用 `with_default`（线程局部）而不是 `set_global_default`（进程全局、
    /// 只能成功一次）：前者不与其他用例争抢那一个全局槽位，因此并行执行时
    /// 互不干扰，也不需要 `Once` 之类的串行化。
    fn assert_span_metadata<F>(make_span: F, expected_name: &'static str)
    where
        F: FnOnce() -> Span,
    {
        tracing::subscriber::with_default(tracing_subscriber::registry(), || {
            let span = make_span();
            assert_eq!(
                span.metadata().map(|m| m.name()),
                Some(expected_name),
                "span name mismatch for {expected_name}",
            );
        });
    }

    #[test]
    fn test_trace_config_default() {
        let config = TraceConfig::default();
        assert_eq!(config.service_name, "sisyphus-engine");
        assert_eq!(config.filter, "info,sisyphus=debug");
        assert!(config.json_stdout);
        assert!(config.otlp_endpoint.is_none());
    }

    #[test]
    fn test_cognitive_loop_span_creation() {
        assert_span_metadata(|| cognitive_loop_span("test_sop", 1), "cognitive_loop");
    }

    #[test]
    fn test_node_execution_span_creation() {
        assert_span_metadata(
            || node_execution_span("fetch_data", "test_sop"),
            "node_execution",
        );
    }

    #[test]
    fn test_reflection_span_creation() {
        assert_span_metadata(|| reflection_span("test_sop", 1), "reflection");
    }

    #[test]
    fn test_evolution_span_creation() {
        assert_span_metadata(|| evolution_span("test_sop", 1), "evolution");
    }

    #[test]
    fn test_memory_span_creation() {
        assert_span_metadata(|| memory_span("get", "l1"), "memory_access");
    }

    #[test]
    fn test_tool_span_creation() {
        assert_span_metadata(|| tool_span("bash"), "tool_execution");
    }

    #[test]
    fn test_observability_error_display() {
        let err = ObservabilityError::FilterError("bad filter".to_string());
        assert!(err.to_string().contains("bad filter"));

        let err = ObservabilityError::TracingInitFailed;
        assert_eq!(err.to_string(), "Tracing subscriber init failed");
    }
}
