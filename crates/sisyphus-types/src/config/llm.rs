use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::provider::ProviderConfig;
use crate::types::ModelRouteStrategy;

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

impl Default for AgentLlmOverride {
    fn default() -> Self {
        Self {
            model: "gpt-4o".into(),
            endpoint: None,
            api_key: None,
        }
    }
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

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
/// Budget Mode.
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
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct EmbeddingConfig {
    #[serde(default = "default_embedding_model")]
    pub model: String,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default = "default_embedding_provider")]
    pub provider: String,
    /// Allow using dummy embedding client for testing.
    /// When false and provider is not "openai", startup will fail.
    #[serde(default)]
    pub allow_dummy_embeddings: bool,
}

fn default_embedding_model() -> String {
    "text-embedding-3-small".into()
}

fn default_embedding_provider() -> String {
    "dummy".into()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
/// Llm Config.
pub struct LlmConfig {
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub model: String,
    /// Embedding configuration.
    #[serde(default)]
    pub embedding: EmbeddingConfig,
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
    /// Fast-fail cooling after connection failure (seconds).
    /// During this window, all calls to the failed model are rejected
    /// immediately without network attempts.
    #[serde(default = "default_fast_fail_cooling_secs")]
    pub fast_fail_cooling_secs: u64,
    /// Request timeout for LLM calls (seconds).
    #[serde(default = "default_request_timeout_secs")]
    pub request_timeout_secs: u64,
    /// Connection timeout for LLM calls (seconds).
    #[serde(default = "default_connect_timeout_secs")]
    pub connect_timeout_secs: u64,
}

fn default_fast_fail_cooling_secs() -> u64 {
    30
}

fn default_request_timeout_secs() -> u64 {
    5
}

fn default_connect_timeout_secs() -> u64 {
    2
}

/// Return the built-in default LLM configuration.
///
/// Used when no `[llm]` table exists in `sisyphus.toml` and no env-var
/// override is set. Points at a local llama.cpp server on port 8000.
pub fn default_llm_config() -> LlmConfig {
    LlmConfig {
        api_key: "".into(),
        endpoint: "http://localhost:8000/v1/chat/completions".into(),
        model: "gpt-4o".into(),
        embedding: EmbeddingConfig {
            model: default_embedding_model(),
            endpoint: None,
            provider: default_embedding_provider(),
            allow_dummy_embeddings: false,
        },
        budget_mode: BudgetMode::default(),
        agents: HashMap::new(),
        categories: HashMap::new(),
        providers: Vec::new(),
        fast_fail_cooling_secs: default_fast_fail_cooling_secs(),
        request_timeout_secs: default_request_timeout_secs(),
        connect_timeout_secs: default_connect_timeout_secs(),
    }
}

impl Default for LlmConfig {
    fn default() -> Self {
        default_llm_config()
    }
}
