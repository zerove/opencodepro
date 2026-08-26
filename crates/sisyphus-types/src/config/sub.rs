use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::errors::TypesError;

// ── CognitiveLoopConfig ─────────────────────────────────────

#[derive(Clone, Debug, Deserialize, Serialize)]
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
}

fn default_llm_stage_timeout_secs() -> u64 {
    30
}

fn default_cognitive_loop() -> CognitiveLoopConfig {
    CognitiveLoopConfig {
        max_iterations: 10,
        convergence_threshold: 0.15,
        llm_stage_timeout_secs: default_llm_stage_timeout_secs(),
        adaptive_depth: AdaptiveDepthConfig::default(),
    }
}

impl Default for CognitiveLoopConfig {
    fn default() -> Self {
        default_cognitive_loop()
    }
}

impl CognitiveLoopConfig {
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
pub struct McpConfig {
    /// Stdio-based MCP servers: prefix → "command arg1 arg2".
    #[serde(default)]
    pub stdio_servers: HashMap<String, String>,
    /// HTTP-based MCP servers: prefix → "http://host:port/path".
    #[serde(default)]
    pub http_servers: HashMap<String, String>,
}

impl McpConfig {
    pub fn validate(&self) -> Result<(), TypesError> {
        Ok(())
    }
}
