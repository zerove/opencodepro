pub mod llm;
pub mod provider;
pub mod sub;

pub use llm::*;
pub use provider::*;
pub use sub::*;

use metrics::counter;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::sync::watch;

use crate::errors::TypesError;

#[derive(Clone, Debug, Deserialize, Serialize)]
/// Config.
pub struct Config {
    /// LLM client parameters.
    #[serde(default)]
    pub llm: LlmConfig,

    /// Cognitive loop parameters.
    #[serde(default)]
    pub cognitive_loop: CognitiveLoopConfig,

    /// DAG executor parameters.
    #[serde(default)]
    pub executor: ExecutorConfig,

    /// Two-level cache (L1 DashMap + L2 SQLite) parameters.
    #[serde(default)]
    pub cache: CacheConfig,

    /// Prometheus metrics exporter.
    #[serde(default)]
    pub metrics: MetricsConfig,

    /// WASM sandbox parameters.
    #[serde(default)]
    pub sandbox: SandboxConfig,

    /// Optional MCP tool servers (stdio or HTTP).
    #[serde(default)]
    pub mcp: McpConfig,

    /// Remote skill registry parameters.
    #[serde(default)]
    pub registry: RegistryConfig,

    /// Jcode integration parameters.
    #[serde(default)]
    pub jcode: JcodeConfig,

    /// Chaos engineering parameters.
    #[serde(default)]
    pub chaos: ChaosConfig,

    /// Cluster peer protocol parameters (TODO 8.3.1 / ADR-009 § 1).
    /// 默认 disabled（保持单机行为），启用后启动 mDNS + 双向 stream。
    #[serde(default)]
    pub cluster: ClusterConfig,

    /// JWT authentication configuration for REST API.
    #[serde(default)]
    pub jwt: JwtConfig,

    /// Bearer token for gRPC bridge authentication. Empty = no auth.
    #[serde(default)]
    pub auth_token: String,

    /// gRPC bridge address (e.g. `http://[::1]:50051`).
    #[serde(default = "default_bridge_address")]
    pub bridge_address: String,

    /// HTTP REST API address (e.g. `http://[::1]:8080`).
    #[serde(default = "default_http_address")]
    pub http_address: String,
}

fn default_bridge_address() -> String {
    "http://[::1]:50051".into()
}

fn default_http_address() -> String {
    "http://[::1]:8080".into()
}

/// JWT configuration for REST API authentication.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct JwtConfig {
    /// Secret key for signing tokens. MUST be set via config or SISYPHUS_JWT_SECRET env var.
    /// No default provided - must be explicitly configured for production.
    #[serde(default)]
    pub secret: String,
    /// Access token TTL in seconds (default: 900 = 15 minutes)
    #[serde(default = "default_jwt_access_ttl")]
    pub access_ttl_secs: i64,
    /// Refresh token TTL in seconds (default: 604800 = 7 days)
    #[serde(default = "default_jwt_refresh_ttl")]
    pub refresh_ttl_secs: i64,
    /// Token issuer (default: "sisyphus-bridge")
    #[serde(default = "default_jwt_issuer")]
    pub issuer: String,
}

fn default_jwt_access_ttl() -> i64 {
    900
}

fn default_jwt_refresh_ttl() -> i64 {
    604800
}

fn default_jwt_issuer() -> String {
    "sisyphus-bridge".to_string()
}

impl Default for JwtConfig {
    fn default() -> Self {
        Self {
            secret: String::new(), // Empty = must be configured via env or config file
            access_ttl_secs: default_jwt_access_ttl(),
            refresh_ttl_secs: default_jwt_refresh_ttl(),
            issuer: default_jwt_issuer(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            llm: default_llm_config(),
            cognitive_loop: CognitiveLoopConfig::default(),
            executor: ExecutorConfig::default(),
            cache: CacheConfig::default(),
            metrics: MetricsConfig::default(),
            sandbox: SandboxConfig::default(),
            mcp: McpConfig::default(),
            registry: RegistryConfig::default(),
            jcode: JcodeConfig::default(),
            chaos: ChaosConfig::default(),
            cluster: ClusterConfig::default(),
            jwt: JwtConfig::default(),
            auth_token: String::new(),
            bridge_address: default_bridge_address(),
            http_address: default_http_address(),
        }
    }
}

impl Config {
    /// Save configuration to a TOML file.
    pub fn save_to_file(&self, path: &str) -> Result<(), TypesError> {
        let content = toml::to_string_pretty(self).map_err(|e| {
            TypesError::StateStoreError(format!("Config serialization failed: {e}"))
        })?;
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent).map_err(|source| TypesError::ConfigReadError {
                path: path.into(),
                source,
            })?;
        }
        std::fs::write(path, &content).map_err(|source| TypesError::ConfigReadError {
            path: path.into(),
            source,
        })
    }

    /// Load a configuration from a TOML file at `path`.
    ///
    /// Returns [`TypesError::ConfigReadError`] if the file cannot be read and
    /// [`TypesError::ConfigParseError`] if the TOML is malformed. Callers
    /// should treat both as fatal: the SOP engine cannot start without a
    /// valid config.
    pub fn from_file(path: &str) -> Result<Self, TypesError> {
        counter!("sisyphus_types_config_loads_total").increment(1);
        let content =
            std::fs::read_to_string(path).map_err(|source| TypesError::ConfigReadError {
                path: path.into(),
                source,
            })?;
        toml::from_str(&content).map_err(|source| TypesError::ConfigParseError {
            path: path.into(),
            source,
        })
    }

    /// Load configuration from environment variables, overriding file/default values.
    /// Prefix: `SISYPHUS_` — e.g. `SISYPHUS_SANDBOX_DEFAULT_FUEL=500000`.
    pub fn apply_env_overrides(&mut self) {
        fn env_u64(key: &str) -> Option<u64> {
            std::env::var(key).ok().and_then(|v| v.parse().ok())
        }
        fn env_usize(key: &str) -> Option<usize> {
            std::env::var(key).ok().and_then(|v| v.parse().ok())
        }
        fn env_u32(key: &str) -> Option<u32> {
            std::env::var(key).ok().and_then(|v| v.parse().ok())
        }
        fn env_f64(key: &str) -> Option<f64> {
            std::env::var(key).ok().and_then(|v| v.parse().ok())
        }
        fn env_u16(key: &str) -> Option<u16> {
            std::env::var(key).ok().and_then(|v| v.parse().ok())
        }
        fn env_string(key: &str) -> Option<String> {
            std::env::var(key).ok()
        }
        fn env_bool(key: &str) -> Option<bool> {
            std::env::var(key).ok().and_then(|v| match v.to_lowercase().as_str() {
                "true" | "1" | "yes" | "on" => Some(true),
                "false" | "0" | "no" | "off" => Some(false),
                _ => None,
            })
        }
        // Sentinel for explicit Option<T> clearing via env (empty string → None).
        fn env_optional_u64(key: &str) -> Option<Option<u64>> {
            std::env::var(key).ok().map(|v| if v.is_empty() { None } else { v.parse().ok() })
        }

        if let Some(v) = env_string("SISYPHUS_REGISTRY_URL") {
            self.registry.registry_url = v;
        }
        if let Some(v) = env_u64("SISYPHUS_REGISTRY_TIMEOUT_SECONDS") {
            self.registry.timeout_seconds = v;
        }
        if let Some(v) = env_string("SISYPHUS_JCODE_SOCKET_PATH") {
            self.jcode.socket_path = v;
        }
        if let Some(v) = env_string("SISYPHUS_LLM_API_KEY") {
            self.llm.api_key = v;
        }
        if let Some(v) = env_string("SISYPHUS_LLM_ENDPOINT") {
            self.llm.endpoint = v;
        }
        if let Some(v) = env_string("SISYPHUS_LLM_MODEL") {
            self.llm.model = v;
        }
        if let Some(v) = env_string("SISYPHUS_LLM_BUDGET_MODE") {
            self.llm.budget_mode = match v.to_lowercase().as_str() {
                "strict" => BudgetMode::Strict,
                _ => BudgetMode::Warning,
            };
        }
        if let Some(v) = env_u64("SISYPHUS_LLM_FAST_FAIL_COOLING_SECS") {
            self.llm.fast_fail_cooling_secs = v;
        }
        if let Some(v) = env_u64("SISYPHUS_LLM_REQUEST_TIMEOUT_SECS") {
            self.llm.request_timeout_secs = v;
        }
        if let Some(v) = env_u64("SISYPHUS_LLM_CONNECT_TIMEOUT_SECS") {
            self.llm.connect_timeout_secs = v;
        }
        if let Some(v) = env_string("SISYPHUS_LLM_EMBEDDING_MODEL") {
            self.llm.embedding.model = v;
        }
        if let Some(v) = env_string("SISYPHUS_LLM_EMBEDDING_ENDPOINT") {
            self.llm.embedding.endpoint = Some(v);
        }
        if let Some(v) = env_string("SISYPHUS_LLM_EMBEDDING_PROVIDER") {
            self.llm.embedding.provider = v;
        }
        if let Some(v) = env_bool("SISYPHUS_LLM_EMBEDDING_ALLOW_DUMMY") {
            self.llm.embedding.allow_dummy_embeddings = v;
        }
        if let Some(v) = env_u32("SISYPHUS_COGNITIVE_LOOP_MAX_ITERATIONS") {
            self.cognitive_loop.max_iterations = v;
        }
        if let Some(v) = env_u64("SISYPHUS_COGNITIVE_LOOP_LLM_STAGE_TIMEOUT_SECS") {
            self.cognitive_loop.llm_stage_timeout_secs = v;
        }
        if let Some(v) = env_f64("SISYPHUS_COGNITIVE_LOOP_CONVERGENCE_THRESHOLD") {
            self.cognitive_loop.convergence_threshold = v;
        }
        if let Some(v) = env_usize("SISYPHUS_EXECUTOR_MAX_CONCURRENT_TASKS") {
            self.executor.max_concurrent_tasks = v;
        }
        if let Some(v) = env_u64("SISYPHUS_EXECUTOR_DEFAULT_FUEL") {
            self.executor.default_fuel = v;
        }
        if let Some(v) = env_usize("SISYPHUS_CACHE_L1_CAPACITY") {
            self.cache.l1_capacity = v;
        }
        if let Some(v) = env_string("SISYPHUS_CACHE_DATABASE_URL") {
            self.cache.database_url = v;
        }
        if let Some(v) = env_bool("SISYPHUS_CACHE_ENABLE_VECTORS") {
            self.cache.enable_vectors = v;
        }
        if let Some(v) = env_usize("SISYPHUS_CACHE_VECTOR_DIMENSION") {
            self.cache.vector_dimension = v;
        }
        if let Some(v) = env_usize("SISYPHUS_CACHE_BATCH_SIZE") {
            self.cache.batch_size = v;
        }
        if let Some(v) = env_u64("SISYPHUS_CACHE_FLUSH_INTERVAL_MS") {
            self.cache.flush_interval_ms = v;
        }
        if let Some(v) = env_u16("SISYPHUS_METRICS_PROMETHEUS_PORT") {
            self.metrics.prometheus_port = v;
        }
        if let Some(v) = env_u32("SISYPHUS_SANDBOX_MAX_MEMORY_PAGES") {
            self.sandbox.max_memory_pages = v;
        }
        if let Some(v) = env_u64("SISYPHUS_SANDBOX_DEFAULT_FUEL") {
            self.sandbox.default_fuel = v;
        }
        if let Some(v) = env_string("SISYPHUS_SANDBOX_WASM_TOOLS_DIR") {
            self.sandbox.wasm_tools_dir = v;
        }
        if let Some(v) = env_bool("SISYPHUS_SANDBOX_USE_SANDBOXD") {
            self.sandbox.use_sandboxd = v;
        }
        if let Some(v) = env_string("SISYPHUS_SANDBOX_SANDBOXD_PATH") {
            self.sandbox.sandboxd_path = Some(v);
        }
        if let Some(v) = env_string("SISYPHUS_AUTH_TOKEN") {
            self.auth_token = v;
        }
        if let Some(v) = env_string("SISYPHUS_JWT_SECRET") {
            self.jwt.secret = v;
        }
        if let Some(v) = env_u64("SISYPHUS_JWT_ACCESS_TTL_SECS") {
            self.jwt.access_ttl_secs = v as i64;
        }
        if let Some(v) = env_u64("SISYPHUS_JWT_REFRESH_TTL_SECS") {
            self.jwt.refresh_ttl_secs = v as i64;
        }
        if let Some(v) = env_string("SISYPHUS_JWT_ISSUER") {
            self.jwt.issuer = v;
        }
        if let Some(v) = env_string("SISYPHUS_BRIDGE_ADDRESS") {
            self.bridge_address = v;
        }
        if let Some(v) = env_string("SISYPHUS_HTTP_ADDRESS") {
            self.http_address = v;
        }
        if let Some(v) = env_bool("SISYPHUS_CHAOS_ENABLED") {
            self.chaos.enabled = v;
        }
        if let Some(v) = env_f64("SISYPHUS_CHAOS_DEFAULT_FAULT_PROBABILITY") {
            self.chaos.default_fault_probability = v;
        }
        if let Some(v) = env_u64("SISYPHUS_CHAOS_DEFAULT_DURATION_SECONDS") {
            self.chaos.default_duration_seconds = v;
        }
        if let Some(v) = env_bool("SISYPHUS_CLUSTER_ENABLED") {
            self.cluster.enabled = v;
        }
        if let Some(v) = env_string("SISYPHUS_CLUSTER_NODE_ID") {
            self.cluster.node_id = v;
        }
        if let Some(v) = env_string("SISYPHUS_CLUSTER_NAME") {
            self.cluster.cluster_name = v;
        }
        if let Some(v) = env_u64("SISYPHUS_CLUSTER_JOIN_TIMEOUT_SECS") {
            self.cluster.join_timeout_secs = v;
        }
        if let Some(v) = env_u64("SISYPHUS_CLUSTER_PEER_TTL_SECS") {
            self.cluster.peer_ttl_secs = v;
        }
        if let Some(v) = env_f64("SISYPHUS_COGNITIVE_LOOP_ADAPTIVE_DEPTH_INITIAL_BUDGET") {
            self.cognitive_loop.adaptive_depth.initial_budget = v;
        }
        if let Some(v) = env_f64("SISYPHUS_COGNITIVE_LOOP_ADAPTIVE_DEPTH_DECAY") {
            self.cognitive_loop.adaptive_depth.decay = v;
        }
        if let Some(v) = env_f64("SISYPHUS_COGNITIVE_LOOP_ADAPTIVE_DEPTH_MIN_IMPROVEMENT") {
            self.cognitive_loop.adaptive_depth.min_improvement = v;
        }
        if let Some(v) = env_f64("SISYPHUS_COGNITIVE_LOOP_ADAPTIVE_DEPTH_CONVERGENCE_THRESHOLD") {
            self.cognitive_loop.adaptive_depth.convergence_threshold = v;
        }
        // anomaly_detector (cognitive loop v2)
        if let Some(v) = env_f64("SISYPHUS_COGNITIVE_LOOP_ANOMALY_DETECTOR_FUEL_THRESHOLD") {
            self.cognitive_loop.anomaly_detector.fuel_threshold = v;
        }
        if let Some(v) = env_f64("SISYPHUS_COGNITIVE_LOOP_ANOMALY_DETECTOR_DURATION_THRESHOLD") {
            self.cognitive_loop.anomaly_detector.duration_threshold = v;
        }
        if let Some(v) = env_f64("SISYPHUS_COGNITIVE_LOOP_ANOMALY_DETECTOR_ERROR_RATE_THRESHOLD") {
            self.cognitive_loop.anomaly_detector.error_rate_threshold = v;
        }
        if let Some(v) = env_usize("SISYPHUS_COGNITIVE_LOOP_ANOMALY_DETECTOR_HISTORY_WINDOW") {
            self.cognitive_loop.anomaly_detector.history_window = v;
        }
        // token_budget: 空字符串表示清除上限 (None)，非空数字设置上限 (Some(cap))
        if let Some(v) = env_optional_u64("SISYPHUS_COGNITIVE_LOOP_TOKEN_BUDGET_CAP") {
            self.cognitive_loop.token_budget.cap = v;
        }
        if let Some(v) = env_f64("SISYPHUS_COGNITIVE_LOOP_TOKEN_BUDGET_RESERVE_RATIO") {
            self.cognitive_loop.token_budget.reserve_ratio = v;
        }
        // OpenTelemetry
        if let Some(v) = env_bool("SISYPHUS_METRICS_OTEL_ENABLED") {
            self.metrics.opentelemetry.enabled = v;
        }
        if let Some(v) = env_string("SISYPHUS_METRICS_OTEL_ENDPOINT") {
            self.metrics.opentelemetry.endpoint = v;
        }
        if let Some(v) = env_string("SISYPHUS_METRICS_OTEL_SERVICE_NAME") {
            self.metrics.opentelemetry.service_name = v;
        }
        if let Some(v) = env_f64("SISYPHUS_METRICS_OTEL_SAMPLE_RATE") {
            self.metrics.opentelemetry.sample_rate = v;
        }
    }

    /// Validate the loaded config against the engine invariants.
    ///
    /// Checks currently enforced:
    /// * When `auth_token` is set, `jwt.secret` must also be set
    ///   (otherwise the bridge cannot mint API tokens).
    /// * (Future) port ranges, retry bounds, provider model allow-list.
    ///
    /// Called automatically by [`Self::from_file`] and [`Self::apply_env_overrides`].
    pub fn validate(&self) -> Result<(), TypesError> {
        if self.auth_token.is_empty() {
            // auth_token is optional; empty = no auth
        }
        // Validate JWT config: secret is required if auth is enabled via auth_token
        if !self.auth_token.is_empty() && self.jwt.secret.is_empty() {
            return Err(TypesError::ConfigValidationError(
                "jwt.secret must be set when auth_token is configured (use SISYPHUS_JWT_SECRET env var)".to_string()
            ));
        }
        // Also warn if JWT is configured but auth_token is not (for REST API JWT auth)
        if self.jwt.secret.is_empty() && !self.auth_token.is_empty() {
            // This case is covered above
        }
        self.validate_address(&self.bridge_address, "bridge_address")?;
        self.validate_address(&self.http_address, "http_address")?;
        self.cognitive_loop.validate()?;
        self.jcode.validate()?;
        self.executor.validate()?;
        self.cache.validate()?;
        self.metrics.validate()?;
        self.sandbox.validate()?;
        self.registry.validate()?;
        self.chaos.validate()?;
        self.cluster.validate()?;
        Ok(())
    }

    /// Resolve `{env:VAR}` and `{env:VAR|default}` placeholders in all string config values.
    pub fn resolve_env_placeholders(&mut self) {
        let mut content = toml::to_string_pretty(self).unwrap_or_default();
        content = resolve_env_placeholders_str(&content);
        if let Ok(resolved) = toml::from_str::<Config>(&content) {
            *self = resolved;
        }
    }

    fn validate_address(&self, addr: &str, field: &str) -> Result<(), TypesError> {
        let stripped = addr.strip_prefix("http://").or_else(|| addr.strip_prefix("https://"));
        match stripped {
            None => Err(TypesError::ConfigValidationError(format!(
                "{field}: \"{addr}\" must start with http:// or https://"
            ))),
            Some("") => Err(TypesError::ConfigValidationError(format!(
                "{field}: \"{addr}\" has no host after scheme prefix"
            ))),
            Some(rest) => {
                let host_port = rest.split('/').next().unwrap_or(rest);
                if host_port.contains(':') {
                    let port_str = host_port.split(':').next_back().unwrap_or("");
                    port_str.parse::<u16>().map_err(|_| {
                        TypesError::ConfigValidationError(format!(
                            "{field}: \"{addr}\" has invalid port number \"{port_str}\""
                        ))
                    })?;
                }
                Ok(())
            },
        }
    }
}

// Re-export LlmConfig's default function so Config's serde(default) attribute can find it.
// (It's defined in llm.rs, brought into scope via pub use llm::*.)

/// Resolve `{env:VAR}` and `{env:VAR|default}` placeholders in a string.
///
/// Patterns:
/// - `{env:VAR}` replaced with env var value, or empty string if unset
/// - `{env:VAR|default}` replaced with env var value, or `default` if unset
pub fn resolve_env_placeholders_str(input: &str) -> String {
    let mut result = input.to_string();
    while let Some(start) = result.find("{env:") {
        if let Some(end_rel) = result[start..].find('}') {
            let end = start + end_rel;
            let inner = &result[start + 5..end];
            let replacement = if let Some((var, default)) = inner.split_once('|') {
                std::env::var(var).unwrap_or_else(|_| default.to_string())
            } else {
                std::env::var(inner).unwrap_or_default()
            };
            result.replace_range(start..=end, &replacement);
        } else {
            break;
        }
    }
    result
}

/// A handle for watching a configuration file and receiving reload notifications.
///
/// Created by `Config::watch()` or `Config::watch_path()`.
/// Drop the handle to stop watching.
pub struct ConfigWatcher {
    _task: tokio::task::JoinHandle<()>,
    rx: watch::Receiver<Config>,
}

impl ConfigWatcher {
    /// Subscribe to configuration changes.
    pub fn subscribe(&self) -> watch::Receiver<Config> {
        self.rx.clone()
    }

    /// Get the current configuration (last known value).
    pub fn current(&self) -> Config {
        self.rx.borrow().clone()
    }
}

impl Config {
    /// Start watching a configuration file for changes and auto-reload.
    ///
    /// Returns a `ConfigWatcher` that broadcasts updates on changes.
    /// The watcher runs in the background until dropped.
    ///
    /// # Arguments
    /// * `path` - Path to the TOML config file to watch.
    ///
    /// # Example
    /// ```no_run
    /// # use sisyphus_types::Config;
    /// # #[tokio::main(flavor = "current_thread")]
    /// # async fn main() -> Result<(), sisyphus_types::TypesError> {
    /// let mut watcher = Config::watch("config.toml").await?;
    /// let mut rx = watcher.subscribe();
    /// while rx.changed().await.is_ok() {
    ///     println!("Config reloaded: {:?}", *rx.borrow());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn watch(path: impl Into<PathBuf>) -> Result<ConfigWatcher, TypesError> {
        let path = path.into();
        let (tx, rx) = watch::channel(Config::default());

        // Load initial config
        let mut config = Config::from_file(path.to_str().unwrap_or(""))?;
        config.apply_env_overrides();
        config.resolve_env_placeholders();
        config.validate()?;
        tx.send_replace(config.clone());

        let path_clone = path.clone();
        let task = tokio::spawn(async move {
            use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
            use std::time::Duration;

            let (tx_event, mut rx_event) = tokio::sync::mpsc::channel(32);

            let mut watcher = match RecommendedWatcher::new(
                move |res| {
                    if let Ok(event) = res {
                        let _ = tx_event.try_send(event);
                    }
                },
                notify::Config::default().with_poll_interval(Duration::from_millis(100)),
            ) {
                Ok(w) => w,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to create file watcher");
                    return;
                },
            };

            if let Err(e) = watcher.watch(&path_clone, RecursiveMode::NonRecursive) {
                tracing::error!(error = %e, path = %path_clone.display(), "Failed to watch config file");
                return;
            }

            while let Some(event) = rx_event.recv().await {
                // Only react to file modifications
                if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                    tracing::info!(path = %path_clone.display(), "Config file changed, reloading");
                    match Config::from_file(path_clone.to_str().unwrap_or("")) {
                        Ok(mut new_config) => {
                            new_config.apply_env_overrides();
                            new_config.resolve_env_placeholders();
                            if new_config.validate().is_ok() {
                                let _ = tx.send_replace(new_config);
                            } else {
                                tracing::error!(
                                    "Config validation failed after reload, keeping old config"
                                );
                            }
                        },
                        Err(e) => {
                            tracing::error!(error = %e, "Failed to reload config");
                        },
                    }
                }
            }
        });

        Ok(ConfigWatcher { _task: task, rx })
    }

    /// Start watching the config file at the given path.
    /// Alias for `Config::watch()`.
    pub async fn watch_path(path: impl Into<PathBuf>) -> Result<ConfigWatcher, TypesError> {
        Self::watch(path).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// 串行化修改 SISYPHUS_*_ANOMALY_DETECTOR_* 环境变量的测试。
    /// 同进程并行测试间共享 env，必须互斥避免 race。
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    #[test]
    fn test_critique_timeout_validation_bounds() {
        let mut c = CognitiveLoopConfig {
            llm_stage_timeout_secs: 30,
            ..Default::default()
        };
        assert!(c.validate().is_ok());
        c.llm_stage_timeout_secs = 601;
        assert!(c.validate().is_err());
        c.llm_stage_timeout_secs = 600;
        assert!(c.validate().is_ok());
    }

    #[tokio::test]
    async fn test_config_watch_and_reload() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("config.toml");

        // Write initial config
        let initial_config = r#"
cognitive_loop = { max_iterations = 5, convergence_threshold = 0.1, llm_stage_timeout_secs = 12 }
sandbox = { max_memory_pages = 10, default_fuel = 1000000, wasm_tools_dir = "tools" }
"#;
        fs::write(&config_path, initial_config).unwrap();

        // Start watching
        let watcher = Config::watch(&config_path).await.unwrap();
        let mut rx = watcher.subscribe();

        // Get initial config (should be immediately available via watch channel)
        let config = rx.borrow().clone();
        assert_eq!(config.cognitive_loop.max_iterations, 5);
        assert_eq!(config.cognitive_loop.llm_stage_timeout_secs, 12);

        // Modify config file - ensure watcher is running first
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let updated_config = r#"
cognitive_loop = { max_iterations = 10, convergence_threshold = 0.1 }
sandbox = { max_memory_pages = 10, default_fuel = 1000000, wasm_tools_dir = "tools" }
"#;
        fs::write(&config_path, updated_config).unwrap();

        // Give the file system and watcher time to detect the change
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // Wait for reload - use timeout to avoid hanging
        let changed = tokio::time::timeout(std::time::Duration::from_secs(5), rx.changed()).await;
        assert!(changed.is_ok(), "Timeout waiting for config reload");
        let _ = changed.unwrap();

        let config = rx.borrow().clone();
        assert_eq!(config.cognitive_loop.max_iterations, 10);

        drop(watcher);
    }

    #[test]
    fn test_resolve_env_placeholders_str() {
        std::env::set_var("SISYPHUS_TEST_VAR", "hello_world");
        let input = "prefix_{env:SISYPHUS_TEST_VAR}_suffix";
        let result = resolve_env_placeholders_str(input);
        assert_eq!(result, "prefix_hello_world_suffix");
    }

    #[test]
    fn test_resolve_env_placeholders_with_default() {
        std::env::set_var("SISYPHUS_UNLIKELY_VAR_XYZ", "set_value");
        let result = resolve_env_placeholders_str("{env:SISYPHUS_UNLIKELY_VAR_XYZ|fallback}");
        assert_eq!(result, "set_value");
    }

    #[test]
    fn test_resolve_env_placeholders_missing_uses_default() {
        let result =
            resolve_env_placeholders_str("{env:SISYPHUS_DEFINITELY_MISSING_VAR_999|my_default}");
        assert_eq!(result, "my_default");
    }

    #[test]
    fn test_resolve_env_placeholders_missing_no_default() {
        let result = resolve_env_placeholders_str("{env:SISYPHUS_DEFINITELY_MISSING_VAR_999}");
        assert_eq!(result, "");
    }

    #[test]
    fn test_resolve_env_placeholders_no_pattern() {
        let result = resolve_env_placeholders_str("no placeholders here");
        assert_eq!(result, "no placeholders here");
    }

    #[test]
    fn test_apply_env_overrides_cache_batch_and_flush() {
        std::env::set_var("SISYPHUS_CACHE_BATCH_SIZE", "250");
        std::env::set_var("SISYPHUS_CACHE_FLUSH_INTERVAL_MS", "500");
        let mut config = Config::default();
        config.apply_env_overrides();
        assert_eq!(config.cache.batch_size, 250);
        assert_eq!(config.cache.flush_interval_ms, 500);
        std::env::remove_var("SISYPHUS_CACHE_BATCH_SIZE");
        std::env::remove_var("SISYPHUS_CACHE_FLUSH_INTERVAL_MS");
    }

    #[test]
    fn test_apply_env_overrides_llm_timeouts_and_jwt() {
        std::env::set_var("SISYPHUS_LLM_FAST_FAIL_COOLING_SECS", "60");
        std::env::set_var("SISYPHUS_LLM_REQUEST_TIMEOUT_SECS", "10");
        std::env::set_var("SISYPHUS_LLM_CONNECT_TIMEOUT_SECS", "3");
        std::env::set_var("SISYPHUS_JWT_SECRET", "test-secret");
        std::env::set_var("SISYPHUS_JWT_ACCESS_TTL_SECS", "1800");
        std::env::set_var("SISYPHUS_JWT_REFRESH_TTL_SECS", "1209600");
        std::env::set_var("SISYPHUS_JWT_ISSUER", "test-issuer");
        let mut config = Config::default();
        config.apply_env_overrides();
        assert_eq!(config.llm.fast_fail_cooling_secs, 60);
        assert_eq!(config.llm.request_timeout_secs, 10);
        assert_eq!(config.llm.connect_timeout_secs, 3);
        assert_eq!(config.jwt.secret, "test-secret");
        assert_eq!(config.jwt.access_ttl_secs, 1800);
        assert_eq!(config.jwt.refresh_ttl_secs, 1209600);
        assert_eq!(config.jwt.issuer, "test-issuer");
        std::env::remove_var("SISYPHUS_LLM_FAST_FAIL_COOLING_SECS");
        std::env::remove_var("SISYPHUS_LLM_REQUEST_TIMEOUT_SECS");
        std::env::remove_var("SISYPHUS_LLM_CONNECT_TIMEOUT_SECS");
        std::env::remove_var("SISYPHUS_JWT_SECRET");
        std::env::remove_var("SISYPHUS_JWT_ACCESS_TTL_SECS");
        std::env::remove_var("SISYPHUS_JWT_REFRESH_TTL_SECS");
        std::env::remove_var("SISYPHUS_JWT_ISSUER");
    }

    #[test]
    fn test_apply_env_overrides_embedding_allow_dummy() {
        // 默认值应为 false
        let mut config = Config::default();
        config.apply_env_overrides();
        assert!(
            !config.llm.embedding.allow_dummy_embeddings,
            "默认应为 false，生产环境应显式启用"
        );

        // 显式启用
        std::env::set_var("SISYPHUS_LLM_EMBEDDING_ALLOW_DUMMY", "true");
        let mut config = Config::default();
        config.apply_env_overrides();
        assert!(config.llm.embedding.allow_dummy_embeddings);
        std::env::remove_var("SISYPHUS_LLM_EMBEDDING_ALLOW_DUMMY");

        // 显式关闭
        std::env::set_var("SISYPHUS_LLM_EMBEDDING_ALLOW_DUMMY", "false");
        let mut config = Config::default();
        config.apply_env_overrides();
        assert!(!config.llm.embedding.allow_dummy_embeddings);
        std::env::remove_var("SISYPHUS_LLM_EMBEDDING_ALLOW_DUMMY");
    }

    #[test]
    fn test_apply_env_overrides_anomaly_detector() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var(
            "SISYPHUS_COGNITIVE_LOOP_ANOMALY_DETECTOR_FUEL_THRESHOLD",
            "4.5",
        );
        std::env::set_var(
            "SISYPHUS_COGNITIVE_LOOP_ANOMALY_DETECTOR_DURATION_THRESHOLD",
            "5.5",
        );
        std::env::set_var(
            "SISYPHUS_COGNITIVE_LOOP_ANOMALY_DETECTOR_ERROR_RATE_THRESHOLD",
            "0.42",
        );
        std::env::set_var(
            "SISYPHUS_COGNITIVE_LOOP_ANOMALY_DETECTOR_HISTORY_WINDOW",
            "50",
        );

        let mut config = Config::default();
        config.apply_env_overrides();

        assert!((config.cognitive_loop.anomaly_detector.fuel_threshold - 4.5).abs() < 1e-9);
        assert!((config.cognitive_loop.anomaly_detector.duration_threshold - 5.5).abs() < 1e-9);
        assert!((config.cognitive_loop.anomaly_detector.error_rate_threshold - 0.42).abs() < 1e-9);
        assert_eq!(config.cognitive_loop.anomaly_detector.history_window, 50);

        std::env::remove_var("SISYPHUS_COGNITIVE_LOOP_ANOMALY_DETECTOR_FUEL_THRESHOLD");
        std::env::remove_var("SISYPHUS_COGNITIVE_LOOP_ANOMALY_DETECTOR_DURATION_THRESHOLD");
        std::env::remove_var("SISYPHUS_COGNITIVE_LOOP_ANOMALY_DETECTOR_ERROR_RATE_THRESHOLD");
        std::env::remove_var("SISYPHUS_COGNITIVE_LOOP_ANOMALY_DETECTOR_HISTORY_WINDOW");
    }

    #[test]
    fn test_apply_env_overrides_anomaly_detector_missing_keeps_defaults() {
        let _guard = ENV_LOCK.lock().unwrap();
        // 未设置环境变量时应保持 Default 值
        for k in [
            "SISYPHUS_COGNITIVE_LOOP_ANOMALY_DETECTOR_FUEL_THRESHOLD",
            "SISYPHUS_COGNITIVE_LOOP_ANOMALY_DETECTOR_DURATION_THRESHOLD",
            "SISYPHUS_COGNITIVE_LOOP_ANOMALY_DETECTOR_ERROR_RATE_THRESHOLD",
            "SISYPHUS_COGNITIVE_LOOP_ANOMALY_DETECTOR_HISTORY_WINDOW",
        ] {
            std::env::remove_var(k);
        }

        let mut config = Config::default();
        config.apply_env_overrides();
        assert!((config.cognitive_loop.anomaly_detector.fuel_threshold - 2.0).abs() < 1e-9);
        assert!((config.cognitive_loop.anomaly_detector.duration_threshold - 3.0).abs() < 1e-9);
        assert!((config.cognitive_loop.anomaly_detector.error_rate_threshold - 0.3).abs() < 1e-9);
        assert_eq!(config.cognitive_loop.anomaly_detector.history_window, 20);
    }
}
