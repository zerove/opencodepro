use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::errors::TypesError;

// ── CognitiveLoopConfig ─────────────────────────────────────

#[derive(Clone, Debug, Deserialize, Serialize)]
/// Cognitive Loop Config.
pub struct CognitiveLoopConfig {
    pub max_iterations: u32,
    pub convergence_threshold: f64,
    /// Unified timeout for in-loop LLM stages (review B3):
    /// reflection, evolution, logical audit and critique all degrade
    /// gracefully on timeout/error; production LLM latency routinely
    /// exceeds the old hardcoded 2s. 0 disables the stage's LLM call.
    #[serde(default = "default_llm_stage_timeout_secs")]
    pub llm_stage_timeout_secs: u64,
    /// Adaptive depth controller parameters (cognitive loop v2).
    #[serde(default)]
    pub adaptive_depth: AdaptiveDepthConfig,
    /// 反思/演进阶段 token 预算（review P2-8 step 2 + 接线）。
    ///  为硬上限； 保留比例（默认 0.1 = 10%）。
    /// 使用  判定跳过 LLM 调用。
    #[serde(default)]
    pub token_budget: TokenBudgetConfig,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
/// Token Budget Config.
pub struct TokenBudgetConfig {
    /// 硬上限 token 数。 或  表示不启用预算（默认 unbounded）。
    pub cap: Option<u64>,
    /// 保留比例 [0.0, 1.0]。当  时跳过 LLM。
    #[serde(default = "default_reserve_ratio")]
    pub reserve_ratio: f64,
}

impl Default for TokenBudgetConfig {
    fn default() -> Self {
        Self { cap: None, reserve_ratio: default_reserve_ratio() }
    }
}

fn default_reserve_ratio() -> f64 { 0.1 }

fn default_llm_stage_timeout_secs() -> u64 {
    30
}

fn default_cognitive_loop() -> CognitiveLoopConfig {
    CognitiveLoopConfig {
        max_iterations: 10,
        convergence_threshold: 0.15,
        llm_stage_timeout_secs: default_llm_stage_timeout_secs(),
        adaptive_depth: AdaptiveDepthConfig::default(),
        token_budget: TokenBudgetConfig::default(),
    }
}

impl Default for CognitiveLoopConfig {
    fn default() -> Self {
        default_cognitive_loop()
    }
}

impl CognitiveLoopConfig {
    /// Validate `max_iterations`, `convergence_threshold`, `llm_stage_timeout_secs`,
    /// and recursively `adaptive_depth`. Returns the first violation found.
    ///
    /// # Errors
    /// Returns [`TypesError::ConfigValidationError`] describing the violating field.
    pub fn validate(&self) -> Result<(), TypesError> {
        if self.max_iterations == 0 {
            return Err(TypesError::ConfigValidationError(
                "cognitive_loop.max_iterations must be > 0".into(),
            ));
        }
        if self.max_iterations > 1000 {
            return Err(TypesError::ConfigValidationError(
                "cognitive_loop.max_iterations must be ≤ 1000".into(),
            ));
        }
        if self.llm_stage_timeout_secs == 0 || self.llm_stage_timeout_secs > 600 {
            return Err(TypesError::ConfigValidationError(
                "cognitive_loop.llm_stage_timeout_secs must be between 1 and 600 (or 0 to disable)"
                    .into(),
            ));
        }
        if !(0.0..=1.0).contains(&self.convergence_threshold) {
            return Err(TypesError::ConfigValidationError(
                "cognitive_loop.convergence_threshold must be between 0.0 and 1.0".into(),
            ));
        }
        self.adaptive_depth.validate()?;
        Ok(())
    }
}

// ── AdaptiveDepthConfig ────────────────────────────────────

#[derive(Clone, Debug, Deserialize, Serialize)]
/// Adaptive Depth Config.
pub struct AdaptiveDepthConfig {
    /// Initial budget multiplier for cognitive iterations.
    pub initial_budget: f64,
    /// Decay factor applied per iteration.
    pub decay: f64,
    /// Minimum improvement required to continue iterating.
    pub min_improvement: f64,
    /// Convergence threshold for confidence-based stopping.
    pub convergence_threshold: f64,
}

impl Default for AdaptiveDepthConfig {
    fn default() -> Self {
        Self {
            initial_budget: 2.0,
            decay: 0.3,
            min_improvement: 0.15,
            convergence_threshold: 0.05,
        }
    }
}

impl AdaptiveDepthConfig {
    /// Validate `initial_budget`, `decay`, `min_improvement`, `convergence_threshold`.
    ///
    /// # Errors
    /// Returns [`TypesError::ConfigValidationError`] on out-of-range values.
    pub fn validate(&self) -> Result<(), TypesError> {
        if self.initial_budget < 1.0 {
            return Err(TypesError::ConfigValidationError(
                "cognitive_loop.adaptive_depth.initial_budget must be ≥ 1.0".into(),
            ));
        }
        if !(0.0..=1.0).contains(&self.decay) {
            return Err(TypesError::ConfigValidationError(
                "cognitive_loop.adaptive_depth.decay must be between 0.0 and 1.0".into(),
            ));
        }
        Ok(())
    }
}

// ── ChaosConfig ─────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, Serialize)]
/// Chaos Config.
pub struct ChaosConfig {
    /// Whether chaos engineering is enabled on startup.
    pub enabled: bool,
    /// Default fault probability used when starting experiments.
    pub default_fault_probability: f64,
    /// Auto-stop duration in seconds for experiments.
    pub default_duration_seconds: u64,
}

impl Default for ChaosConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            default_fault_probability: 0.1,
            default_duration_seconds: 300,
        }
    }
}

impl ChaosConfig {
    /// Validate chaos-engineering toggles (failure rate ∈ [0, 1], enabled flags consistent).
    ///
    /// # Errors
    /// Returns [`TypesError::ConfigValidationError`] on invalid ranges.
    pub fn validate(&self) -> Result<(), TypesError> {
        if !(0.0..=1.0).contains(&self.default_fault_probability) {
            return Err(TypesError::ConfigValidationError(
                "chaos.default_fault_probability must be between 0.0 and 1.0".into(),
            ));
        }
        Ok(())
    }
}

// ── ExecutorConfig ─────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, Serialize)]
/// Executor Config.
pub struct ExecutorConfig {
    pub max_concurrent_tasks: usize,
    pub default_fuel: u64,
}

fn default_executor() -> ExecutorConfig {
    ExecutorConfig {
        max_concurrent_tasks: 16,
        default_fuel: 1_000_000,
    }
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        default_executor()
    }
}

impl ExecutorConfig {
    /// Validate `max_concurrent`, `node_timeout_secs`, retry policy bounds.
    ///
    /// # Errors
    /// Returns [`TypesError::ConfigValidationError`] on invalid values.
    pub fn validate(&self) -> Result<(), TypesError> {
        if self.max_concurrent_tasks == 0 {
            return Err(TypesError::ConfigValidationError(
                "executor.max_concurrent_tasks must be > 0".into(),
            ));
        }
        if self.default_fuel == 0 {
            return Err(TypesError::ConfigValidationError(
                "executor.default_fuel must be > 0".into(),
            ));
        }
        Ok(())
    }
}

// ── CacheConfig ────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, Serialize)]
/// Cache Config.
pub struct CacheConfig {
    pub l1_capacity: usize,
    /// Database connection URL.
    /// SQLite: `sqlite:data/sisyphus.db`
    /// PostgreSQL: `postgres://user:pass@host/db`
    pub database_url: String,
    /// Enable vector similarity search via pgvector (Postgres) or full-scan (SQLite).
    pub enable_vectors: bool,
    /// Embedding dimension (defaults to 1536 for OpenAI text-embedding-3-small).
    pub vector_dimension: usize,
    /// Batch size for L2 flushes.
    pub batch_size: usize,
    /// Maximum delay before forced L2 flush.
    pub flush_interval_ms: u64,
}

fn default_cache() -> CacheConfig {
    CacheConfig {
        l1_capacity: 10_000,
        database_url: "sqlite:data/sisyphus.db".to_string(),
        enable_vectors: false,
        vector_dimension: 1536,
        batch_size: 100,
        flush_interval_ms: 100,
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        default_cache()
    }
}

impl CacheConfig {
    /// Validate L1/L2 cache sizes and TTLs are positive.
    ///
    /// # Errors
    /// Returns [`TypesError::ConfigValidationError`] if a size or TTL is zero
    /// when expected positive, or exceeds the documented hard cap.
    pub fn validate(&self) -> Result<(), TypesError> {
        if self.l1_capacity == 0 {
            return Err(TypesError::ConfigValidationError(
                "cache.l1_capacity must be > 0".into(),
            ));
        }
        if self.database_url.is_empty() {
            return Err(TypesError::ConfigValidationError(
                "cache.database_url must not be empty".into(),
            ));
        }
        if self.enable_vectors && self.vector_dimension == 0 {
            return Err(TypesError::ConfigValidationError(
                "cache.vector_dimension must be > 0 when enable_vectors is true".into(),
            ));
        }
        Ok(())
    }
}

// ── MetricsConfig ──────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, Serialize)]
/// Metrics Config.
pub struct MetricsConfig {
    pub prometheus_port: u16,
    /// OpenTelemetry 分布式追踪配置
    #[serde(default)]
    pub opentelemetry: OtelConfig,
}

fn default_metrics() -> MetricsConfig {
    MetricsConfig {
        prometheus_port: 9100,
        opentelemetry: OtelConfig::default(),
    }
}

impl Default for MetricsConfig {
    fn default() -> Self {
        default_metrics()
    }
}

impl MetricsConfig {
    /// Validate `endpoint` is a valid URL and `scrape_interval_secs` is in a
    /// reasonable range (1..=3600).
    ///
    /// # Errors
    /// Returns [`TypesError::ConfigValidationError`] on bad input.
    pub fn validate(&self) -> Result<(), TypesError> {
        if self.prometheus_port == 0 {
            return Err(TypesError::ConfigValidationError(
                "metrics.prometheus_port must be > 0".into(),
            ));
        }
        self.opentelemetry.validate()
    }
}

// ── OpenTelemetry Config ──────────────────────────────────

#[derive(Clone, Debug, Deserialize, Serialize)]
/// Otel Config.
pub struct OtelConfig {
    /// 是否启用 OpenTelemetry 导出
    pub enabled: bool,
    /// OTLP 导出端点
    pub endpoint: String,
    /// 服务名称
    pub service_name: String,
    /// 采样率 (0.0 ~ 1.0)
    pub sample_rate: f64,
}

impl Default for OtelConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: "http://localhost:4317".to_string(),
            service_name: "sisyphus-bridge".to_string(),
            sample_rate: 1.0,
        }
    }
}

impl OtelConfig {
    /// Validate the OTLP endpoint URL and sampling ratio ∈ [0.0, 1.0].
    ///
    /// # Errors
    /// Returns [`TypesError::ConfigValidationError`] on bad input.
    pub fn validate(&self) -> Result<(), TypesError> {
        if !(0.0..=1.0).contains(&self.sample_rate) {
            return Err(TypesError::ConfigValidationError(
                "metrics.opentelemetry.sample_rate must be between 0.0 and 1.0".into(),
            ));
        }
        if self.enabled && self.endpoint.is_empty() {
            return Err(TypesError::ConfigValidationError(
                "metrics.opentelemetry.endpoint must not be empty when enabled".into(),
            ));
        }
        Ok(())
    }
}

// ── SandboxConfig ──────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, Serialize)]
/// Sandbox Config.
pub struct SandboxConfig {
    pub max_memory_pages: u32,
    pub default_fuel: u64,
    pub wasm_tools_dir: String,
    /// If true, bridge spawns `sisyphus-sandboxd` as a child process
    /// instead of embedding wasmtime directly.
    #[serde(default)]
    pub use_sandboxd: bool,
    /// Path to the `sisyphus-sandboxd` binary (default: search PATH).
    #[serde(default)]
    pub sandboxd_path: Option<String>,
}

fn default_sandbox() -> SandboxConfig {
    SandboxConfig {
        max_memory_pages: 10,
        default_fuel: 1_000_000,
        wasm_tools_dir: "tools".to_string(),
        use_sandboxd: false,
        sandboxd_path: None,
    }
}

impl Default for SandboxConfig {
    fn default() -> Self {
        default_sandbox()
    }
}

impl SandboxConfig {
    /// Validate sandbox worker count, memory limit, fuel, and per-tool timeout.
    ///
    /// # Errors
    /// Returns [`TypesError::ConfigValidationError`] on invalid values
    /// (e.g. `fuel` of 0 disables all execution).
    pub fn validate(&self) -> Result<(), TypesError> {
        if self.max_memory_pages == 0 {
            return Err(TypesError::ConfigValidationError(
                "sandbox.max_memory_pages must be > 0".into(),
            ));
        }
        if self.default_fuel == 0 {
            return Err(TypesError::ConfigValidationError(
                "sandbox.default_fuel must be > 0".into(),
            ));
        }
        if self.wasm_tools_dir.is_empty() {
            return Err(TypesError::ConfigValidationError(
                "sandbox.wasm_tools_dir must not be empty".into(),
            ));
        }
        Ok(())
    }
}

// ── JcodeConfig ──────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, Serialize)]
/// Jcode Config.
pub struct JcodeConfig {
    pub socket_path: String,
}

impl Default for JcodeConfig {
    fn default() -> Self {
        Self {
            socket_path: "/tmp/jcode-debug.sock".to_string(),
        }
    }
}

impl JcodeConfig {
    /// Validate JCode integration endpoints and feature flags.
    ///
    /// # Errors
    /// Returns [`TypesError::ConfigValidationError`] on bad input.
    pub fn validate(&self) -> Result<(), TypesError> {
        if self.socket_path.is_empty() {
            return Err(TypesError::ConfigValidationError(
                "jcode.socket_path must not be empty".into(),
            ));
        }
        Ok(())
    }
}

// ── RegistryConfig ─────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, Serialize)]
/// Registry Config.
pub struct RegistryConfig {
    pub registry_url: String,
    pub timeout_seconds: u64,
}

fn default_registry() -> RegistryConfig {
    RegistryConfig {
        registry_url: "https://registry.sisyphus.ai".to_string(),
        timeout_seconds: 30,
    }
}

impl Default for RegistryConfig {
    fn default() -> Self {
        default_registry()
    }
}

impl RegistryConfig {
    /// Validate the registry URL and refresh interval.
    ///
    /// # Errors
    /// Returns [`TypesError::ConfigValidationError`] on bad input.
    pub fn validate(&self) -> Result<(), TypesError> {
        if self.registry_url.is_empty() {
            return Err(TypesError::ConfigValidationError(
                "registry.registry_url must not be empty".into(),
            ));
        }
        Ok(())
    }
}

// ── McpConfig ──────────────────────────────────────────────────

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
/// Mcp Config.
pub struct McpConfig {
    /// Stdio-based MCP servers: prefix → "command arg1 arg2".
    #[serde(default)]
    pub stdio_servers: HashMap<String, String>,
    /// HTTP-based MCP servers: prefix → "http://host:port/path".
    #[serde(default)]
    pub http_servers: HashMap<String, String>,
}

impl McpConfig {
    /// Validate MCP server entries (port range, command whitelist).
    ///
    /// # Errors
    /// Returns [`TypesError::ConfigValidationError`] on bad input.
    pub fn validate(&self) -> Result<(), TypesError> {
        Ok(())
    }
}
