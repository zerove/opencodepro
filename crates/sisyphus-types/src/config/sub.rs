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
    /// Anomaly detector thresholds (cognitive loop v2).
    /// 用于驱动 `AnomalyDetector::new(...)` 而非硬编码默认值。
    #[serde(default)]
    pub anomaly_detector: AnomalyDetectorConfig,
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
        Self {
            cap: None,
            reserve_ratio: default_reserve_ratio(),
        }
    }
}

impl TokenBudgetConfig {
    /// Validate `cap` and `reserve_ratio`. Empty `cap` (None) is allowed and
    /// disables the budget gate; only when `cap.is_some()` is `reserve_ratio`
    /// required to lie in [0.0, 1.0].
    ///
    /// # Errors
    /// Returns [`TypesError::ConfigValidationError`] describing the violating field.
    pub fn validate(&self) -> Result<(), TypesError> {
        if !(0.0..=1.0).contains(&self.reserve_ratio) {
            return Err(TypesError::ConfigValidationError(
                "cognitive_loop.token_budget.reserve_ratio must be between 0.0 and 1.0".into(),
            ));
        }
        if let Some(cap) = self.cap {
            if cap == 0 {
                return Err(TypesError::ConfigValidationError(
                    "cognitive_loop.token_budget.cap must be > 0 when set".into(),
                ));
            }
        }
        Ok(())
    }
}

fn default_reserve_ratio() -> f64 {
    0.1
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
        anomaly_detector: AnomalyDetectorConfig::default(),
        token_budget: TokenBudgetConfig::default(),
    }
}

// ── AnomalyDetectorConfig ──────────────────────────────────

#[derive(Clone, Debug, Deserialize, Serialize)]
/// Anomaly Detector Config.
pub struct AnomalyDetectorConfig {
    /// 燃料（fuel）异常倍数阈值：观测值 > expected * `fuel_threshold` 触发异常。
    pub fuel_threshold: f64,
    /// 耗时异常倍数阈值：观测值 > expected * `duration_threshold` 触发异常。
    pub duration_threshold: f64,
    /// 错误率阈值：节点历史错误率 > `error_rate_threshold` 触发异常。
    pub error_rate_threshold: f64,
    /// 移动平均的窗口大小（≥ 1）。
    pub history_window: usize,
}

impl Default for AnomalyDetectorConfig {
    fn default() -> Self {
        Self {
            fuel_threshold: 2.0,
            duration_threshold: 3.0,
            error_rate_threshold: 0.3,
            history_window: 20,
        }
    }
}

impl AnomalyDetectorConfig {
    /// Validate all thresholds are finite positive numbers and history_window ≥ 1.
    ///
    /// # Errors
    /// Returns [`TypesError::ConfigValidationError`] on out-of-range values.
    pub fn validate(&self) -> Result<(), TypesError> {
        // 倍数阈值（fuel/duration）必须严格 > 0
        for (name, value) in [
            ("fuel_threshold", self.fuel_threshold),
            ("duration_threshold", self.duration_threshold),
        ] {
            if !value.is_finite() || value <= 0.0 {
                return Err(TypesError::ConfigValidationError(format!(
                    "cognitive_loop.anomaly_detector.{name} must be > 0 (got {value})"
                )));
            }
        }
        // error_rate ∈ [0.0, 1.0] — 0.0 表示"零容忍"，1.0 表示"全部都视为异常"
        if !self.error_rate_threshold.is_finite()
            || !(0.0..=1.0).contains(&self.error_rate_threshold)
        {
            return Err(TypesError::ConfigValidationError(
                "cognitive_loop.anomaly_detector.error_rate_threshold must be a finite number between 0.0 and 1.0"
                    .into(),
            ));
        }
        if self.history_window == 0 {
            return Err(TypesError::ConfigValidationError(
                "cognitive_loop.anomaly_detector.history_window must be ≥ 1".into(),
            ));
        }
        Ok(())
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
        self.anomaly_detector.validate()?;
        self.token_budget.validate()?;
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

// ── ClusterConfig (TODO 8.3.1) ─────────────────────────────────────────────

/// Cluster peer protocol configuration (ADR-009 § 1).
///
/// 默认 `enabled = false` 保持单机行为不变；启用后启动 mDNS discovery +
/// 静态 peer fallback，建立双向 stream 与其他节点通信。
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ClusterConfig {
    /// Whether cluster peer protocol is enabled on startup.
    #[serde(default)]
    pub enabled: bool,

    /// 本节点唯一 ID（默认从 HOSTNAME 派生；可通过 env 覆盖）。
    /// 留空字符串 = 启动时自动派生。
    #[serde(default)]
    pub node_id: String,

    /// 集群名（多套集群并存时区分；默认 `_sisyphus-bridge`）。
    #[serde(default = "default_cluster_name")]
    pub cluster_name: String,

    /// mDNS 服务类型（RFC 6763）；默认 `_sisyphus-bridge._tcp.local`。
    #[serde(default = "default_mdns_service_type")]
    pub mdns_service_type: String,

    /// 静态 peer 列表（K8s 等多播受限环境 fallback）。
    /// 格式：`["node-1:50051", "node-2:50051"]`
    #[serde(default)]
    pub peers: Vec<String>,

    /// Join 超时（秒）
    #[serde(default = "default_cluster_join_timeout_secs")]
    pub join_timeout_secs: u64,

    /// Peer TTL（秒）：超过此时间无心跳视为离线。
    #[serde(default = "default_cluster_peer_ttl_secs")]
    pub peer_ttl_secs: u64,

    /// 联邦策略权重发布周期（秒，v0.4.0 Phase B，ADR-017）。
    ///
    /// 0 = 只消费入站、不发布（仅暴露共享视图的场景）。默认 60。
    #[serde(default = "default_strategy_sync_interval_secs")]
    pub strategy_sync_interval_secs: u64,

    /// 联邦经验聚合策略（v0.4.0 Phase A，ADR-016）。
    ///
    /// 默认 `enabled = false`：入站经验全量进入 CRDT 合并，行为与 v0.3.0 一致。
    #[serde(default)]
    pub federated: crate::experience::FederatedAggregatorConfig,

    /// 执行路径是否消费集群策略权重（v0.4.0 Phase E，ADR-017 Amendment 3）。
    ///
    /// 默认 `enabled = false`：策略提示只依据本地 `MetaLearner`，与 v0.3.0
    /// 行为一致。打开后集群聚合结果只在**本地证据不足**时补位，优先级见
    /// [`crate::strategy_weights::select_strategy`]。
    #[serde(default)]
    pub strategy_selection: crate::strategy_weights::StrategySelectionConfig,
}

fn default_cluster_name() -> String {
    "_sisyphus-bridge".to_string()
}

fn default_mdns_service_type() -> String {
    "_sisyphus-bridge._tcp.local".to_string()
}

fn default_cluster_join_timeout_secs() -> u64 {
    10
}

fn default_cluster_peer_ttl_secs() -> u64 {
    30
}

fn default_strategy_sync_interval_secs() -> u64 {
    60
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            node_id: String::new(),
            cluster_name: default_cluster_name(),
            mdns_service_type: default_mdns_service_type(),
            peers: Vec::new(),
            join_timeout_secs: default_cluster_join_timeout_secs(),
            peer_ttl_secs: default_cluster_peer_ttl_secs(),
            strategy_sync_interval_secs: default_strategy_sync_interval_secs(),
            federated: crate::experience::FederatedAggregatorConfig::default(),
            strategy_selection: crate::strategy_weights::StrategySelectionConfig::default(),
        }
    }
}

impl ClusterConfig {
    /// Validate cluster peer protocol settings.
    ///
    /// 校验：
    /// - join_timeout_secs ≥ 1
    /// - peer_ttl_secs ≥ 5（避免误判瞬时网络抖动）
    /// - peer_ttl_secs > join_timeout_secs（heartbeat 间隔应大于 join 超时）
    /// - 联邦聚合参数（`cluster.federated.*`）见
    ///   [`crate::experience::FederatedAggregatorConfig::validate`]；仅在
    ///   `federated.enabled` 时校验——未启用时这些字段不参与任何判定，
    ///   让它们拦住启动只会制造「改了个没用的值就起不来」的困惑。
    pub fn validate(&self) -> Result<(), crate::errors::TypesError> {
        use crate::errors::TypesError;
        if self.join_timeout_secs < 1 {
            return Err(TypesError::ConfigValidationError(
                "cluster.join_timeout_secs must be ≥ 1".into(),
            ));
        }
        if self.peer_ttl_secs < 5 {
            return Err(TypesError::ConfigValidationError(
                "cluster.peer_ttl_secs must be ≥ 5".into(),
            ));
        }
        if self.peer_ttl_secs <= self.join_timeout_secs {
            return Err(TypesError::ConfigValidationError(
                "cluster.peer_ttl_secs must be > join_timeout_secs".into(),
            ));
        }
        if self.strategy_sync_interval_secs != 0 && self.strategy_sync_interval_secs < 1 {
            return Err(TypesError::ConfigValidationError(
                "cluster.strategy_sync_interval_secs must be 0 or ≥ 1".into(),
            ));
        }
        if self.federated.enabled {
            self.federated.validate()?;
        }
        // 与 `federated` 同样的取舍：未启用时不校验，避免「改了个不参与判定的
        // 值就起不来」。启用后 `min_uses = 0` 必须拒绝——它会让「证据门槛」
        // 这个概念消失（运行时会被 max(1) 静默改成 1）。
        if self.strategy_selection.enabled && self.strategy_selection.min_uses == 0 {
            return Err(TypesError::ConfigValidationError(
                "cluster.strategy_selection.min_uses must be ≥ 1 when enabled".into(),
            ));
        }
        Ok(())
    }
}
