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
