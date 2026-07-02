use serde::{Deserialize, Serialize};

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

fn default_provider_priority() -> u32 {
    10
}
fn default_provider_enabled() -> bool {
    true
}

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
    pub fn resolve_api_key(&self) -> String {
        if let Some(env_var) = self.api_key.strip_prefix("env:") {
            std::env::var(env_var).unwrap_or_else(|_| String::new())
        } else {
            self.api_key.clone()
        }
    }
}
