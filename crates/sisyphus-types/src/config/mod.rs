pub mod llm;
pub mod provider;
pub mod sub;

pub use llm::*;
pub use provider::*;
pub use sub::*;

use serde::{Deserialize, Serialize};

use crate::errors::TypesError;

#[derive(Clone, Debug, Deserialize, Serialize)]
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
            auth_token: String::new(),
            bridge_address: default_bridge_address(),
            http_address: default_http_address(),
        }
    }
}

impl Config {
    /// Save configuration to a TOML file.
    pub fn save_to_file(&self, path: &str) -> Result<(), TypesError> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| TypesError::StateStoreError(format!("Config serialization failed: {e}")))?;
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

    /// Load configuration from a TOML file.
    pub fn from_file(path: &str) -> Result<Self, TypesError> {
        let content = std::fs::read_to_string(path)
            .map_err(|source| TypesError::ConfigReadError { path: path.into(), source })?;
        toml::from_str(&content)
            .map_err(|source| TypesError::ConfigParseError { path: path.into(), source })
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

        if let Some(v) = env_u32("SISYPHUS_COGNITIVE_LOOP_MAX_ITERATIONS") {
            self.cognitive_loop.max_iterations = v;
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
        if let Some(v) = env_string("SISYPHUS_CACHE_SQLITE_PATH") {
            self.cache.sqlite_path = v;
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
        if let Some(v) = env_string("SISYPHUS_AUTH_TOKEN") {
            self.auth_token = v;
        }
        if let Some(v) = env_string("SISYPHUS_BRIDGE_ADDRESS") {
            self.bridge_address = v;
        }
        if let Some(v) = env_string("SISYPHUS_HTTP_ADDRESS") {
            self.http_address = v;
        }
    }

    /// Validate all config fields, returning an error on first invalid value.
    pub fn validate(&self) -> Result<(), TypesError> {
        if self.auth_token.is_empty() {
            // auth_token is optional; empty = no auth
        }
        self.validate_address(&self.bridge_address, "bridge_address")?;
        self.validate_address(&self.http_address, "http_address")?;
        self.cognitive_loop.validate()?;
        self.executor.validate()?;
        self.cache.validate()?;
        self.metrics.validate()?;
        self.sandbox.validate()?;
        Ok(())
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
            }
        }
    }
}

// Re-export LlmConfig's default function so Config's serde(default) attribute can find it.
// (It's defined in llm.rs, brought into scope via pub use llm::*.)
