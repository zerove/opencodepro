use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::types::ModelRouteStrategy;

/// Supported LLM provider kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProviderKind {
    /// OpenAI-compatible API.
    #[serde(rename = "openai")]
    OpenAi,
    /// Google Generative AI (Gemini).
    #[serde(rename = "google")]
    Google,
    /// Anthropic Claude.
    #[serde(rename = "anthropic")]
    Anthropic,
}

/// Configuration for a single LLM provider instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// Human-readable name (e.g. "google-gemini-2", "anthropic-claude-sonnet")
    pub name: String,
    /// Provider kind
    pub kind: ProviderKind,
    /// API endpoint URL
    pub endpoint: String,
    /// API key (prefixed with `env:` to read from environment)
    pub api_key: String,
    /// Model name to use
    pub model: String,
    /// Optional base URL for provider-specific endpoints
    #[serde(default)]
    pub base_url: Option<String>,
    /// Priority in fallback chain (lower = tried first)
    #[serde(default = "default_provider_priority")]
    pub priority: u32,
    /// Whether this provider is active
    #[serde(default = "default_provider_enabled")]
    pub enabled: bool,
    /// Max tokens for responses
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// Temperature for generation
    #[serde(default)]
    pub temperature: Option<f64>,
}

fn default_provider_priority() -> u32 { 10 }
fn default_provider_enabled() -> bool { true }

impl ProviderKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderKind::OpenAi => "openai",
            ProviderKind::Google => "google",
            ProviderKind::Anthropic => "anthropic",
        }
    }
}

impl std::str::FromStr for ProviderKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "openai" | "open-ai" => Ok(ProviderKind::OpenAi),
            "google" | "gemini" => Ok(ProviderKind::Google),
            "anthropic" | "claude" => Ok(ProviderKind::Anthropic),
            _ => Err(format!("unknown provider: {s}")),
        }
    }
}

impl std::fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl ProviderConfig {
    /// Resolve the API key: if prefixed with `env:`, read from environment.
    ///
    /// # Example
    ///
    /// ```
    /// use sisyphus_types::ProviderConfig;
    /// let cfg = ProviderConfig {
    ///     name: "test".into(),
    ///     kind: sisyphus_types::ProviderKind::OpenAi,
    ///     endpoint: "http://localhost:8000".into(),
    ///     api_key: "sk-my-key".into(),
    ///     model: "gpt-4o".into(),
    ///     base_url: None,
    ///     priority: 10,
    ///     enabled: true,
    ///     max_tokens: None,
    ///     temperature: None,
    /// };
    /// assert_eq!(cfg.resolve_api_key(), "sk-my-key");
    /// ```
    pub fn resolve_api_key(&self) -> String {
        if let Some(env_var) = self.api_key.strip_prefix("env:") {
            std::env::var(env_var).unwrap_or_else(|_| String::new())
        } else {
            self.api_key.clone()
        }
    }
}

use crate::errors::TypesError;

/// Per-agent model override.
///
/// Any field set here takes precedence over the base `LlmConfig`.
/// Missing fields fall back to the base config values.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AgentLlmOverride {
    pub model: String,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
}

/// Per-category model override and routing strategy.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CategoryConfig {
    pub model: String,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub strategy: Option<ModelRouteStrategy>,
}

impl Default for AgentLlmOverride {
    fn default() -> Self {
        Self {
            model: "gpt-4o".into(),
            endpoint: None,
            api_key: None,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub enum BudgetMode {
    /// Log a warning but allow the LLM call to proceed.
    #[default]
    Warning,
    /// Block the LLM call and return an error if the budget is exceeded.
    Strict,
}

/// Deserializable TOML configuration for all Sisyphus components.
///
/// Use `Config::from_file(path)` or `Config::default()` and override
/// individual fields via environment variables.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LlmConfig {
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub model: String,
    /// Budget enforcement mode. Defaults to `Warning`.
    #[serde(default)]
    pub budget_mode: BudgetMode,
    /// Per-agent model overrides. Key = agent_id (e.g. "metis", "librarian").
    #[serde(default)]
    pub agents: HashMap<String, AgentLlmOverride>,
    /// Per-category model overrides and routing strategies.
    #[serde(default)]
    pub categories: HashMap<String, CategoryConfig>,
    /// Optional multi-provider configuration for fallback support.
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
}

pub fn default_llm_config() -> LlmConfig {
    LlmConfig {
        api_key: "".into(),
        endpoint: "http://localhost:8000/v1/chat/completions".into(),
        model: "gpt-4o".into(),
        budget_mode: BudgetMode::default(),
        agents: HashMap::new(),
        categories: HashMap::new(),
        providers: Vec::new(),
    }
}

impl Default for LlmConfig {
    fn default() -> Self {
        default_llm_config()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Config {
    /// LLM client parameters.
    #[serde(default = "default_llm_config")]
    pub llm: LlmConfig,

    /// Cognitive loop parameters.

    #[serde(default = "default_cognitive_loop")]
    pub cognitive_loop: CognitiveLoopConfig,

    /// DAG executor parameters.
    #[serde(default = "default_executor")]
    pub executor: ExecutorConfig,

    /// Two-level cache (L1 DashMap + L2 SQLite) parameters.
    #[serde(default = "default_cache")]
    pub cache: CacheConfig,

    /// Prometheus metrics exporter.
    #[serde(default = "default_metrics")]
    pub metrics: MetricsConfig,

    /// WASM sandbox parameters.
    #[serde(default = "default_sandbox")]
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

impl Default for Config {
    fn default() -> Self {
        Self {
            llm: default_llm_config(),
            cognitive_loop: default_cognitive_loop(),
            executor: default_executor(),
            cache: default_cache(),
            metrics: default_metrics(),
            sandbox: default_sandbox(),
            mcp: McpConfig::default(),
            auth_token: String::new(),
            bridge_address: default_bridge_address(),
            http_address: default_http_address(),
        }
    }
}

fn default_bridge_address() -> String {
    "http://[::1]:50051".into()
}

fn default_http_address() -> String {
    "http://[::1]:8080".into()
}

impl Config {
    /// Save configuration to a TOML file.
    pub fn save_to_file(&self, path: &str) -> Result<(), TypesError> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| TypesError::StateStoreError(format!("Config serialization failed: {e}")))?;
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent)
                .map_err(|source| TypesError::ConfigReadError { path: path.into(), source })?;
        }
        std::fs::write(path, &content)
            .map_err(|source| TypesError::ConfigReadError { path: path.into(), source })
    }

    /// Load configuration from a TOML file.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use sisyphus_types::Config;
    /// // This will return an Err if the file doesn't exist
    /// let result = Config::from_file("/tmp/non-existent-config.toml");
    /// assert!(result.is_err());
    /// ```
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
    ///
    /// # Example
    ///
    /// ```
    /// use sisyphus_types::Config;
    /// let cfg = Config::default();
    /// assert!(cfg.validate().is_ok());
    /// ```
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
            Some(rest) if rest.is_empty() => Err(TypesError::ConfigValidationError(format!(
                "{field}: \"{addr}\" has no host after scheme prefix"
            ))),
            Some(rest) => {
                // Validate the host:port part, ignore path/query if present
                let host_port = rest.split('/').next().unwrap_or(rest);
                if host_port.contains(':') {
                    let port_str = host_port.split(':').last().unwrap_or("");
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
        if !(0.0..=1.0).contains(&self.convergence_threshold) {
            return Err(TypesError::ConfigValidationError(
                "cognitive_loop.convergence_threshold must be between 0.0 and 1.0".into(),
            ));
        }
        Ok(())
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

impl CacheConfig {
    pub fn validate(&self) -> Result<(), TypesError> {
        if self.l1_capacity == 0 {
            return Err(TypesError::ConfigValidationError(
                "cache.l1_capacity must be > 0".into(),
            ));
        }
        if self.sqlite_path.is_empty() {
            return Err(TypesError::ConfigValidationError(
                "cache.sqlite_path must not be empty".into(),
            ));
        }
        Ok(())
    }
}

impl MetricsConfig {
    pub fn validate(&self) -> Result<(), TypesError> {
        if self.prometheus_port == 0 {
            return Err(TypesError::ConfigValidationError(
                "metrics.prometheus_port must be > 0".into(),
            ));
        }
        Ok(())
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

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CognitiveLoopConfig {
    pub max_iterations: u32,
    pub convergence_threshold: f64,
}

fn default_cognitive_loop() -> CognitiveLoopConfig {
    CognitiveLoopConfig {
        max_iterations: 10,
        convergence_threshold: 0.15,
    }
}

impl Default for CognitiveLoopConfig {
    fn default() -> Self {
        default_cognitive_loop()
    }
}

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

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CacheConfig {
    pub l1_capacity: usize,
    pub sqlite_path: String,
}

fn default_cache() -> CacheConfig {
    CacheConfig {
        l1_capacity: 10_000,
        sqlite_path: "data/sisyphus.db".to_string(),
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        default_cache()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MetricsConfig {
    pub prometheus_port: u16,
}

fn default_metrics() -> MetricsConfig {
    MetricsConfig {
        prometheus_port: 9100,
    }
}

impl Default for MetricsConfig {
    fn default() -> Self {
        default_metrics()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SandboxConfig {
    pub max_memory_pages: u32,
    pub default_fuel: u64,
    pub wasm_tools_dir: String,
}

fn default_sandbox() -> SandboxConfig {
    SandboxConfig {
        max_memory_pages: 10,
        default_fuel: 1_000_000,
        wasm_tools_dir: "tools".to_string(),
    }
}

impl Default for SandboxConfig {
    fn default() -> Self {
        default_sandbox()
    }
}

/// Configuration for MCP (Model Context Protocol) tool servers.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct McpConfig {
    /// Stdio-based MCP servers: prefix → "command arg1 arg2".
    #[serde(default)]
    pub stdio_servers: HashMap<String, String>,
    /// HTTP-based MCP servers: prefix → "http://host:port/path".
    #[serde(default)]
    pub http_servers: HashMap<String, String>,
}
