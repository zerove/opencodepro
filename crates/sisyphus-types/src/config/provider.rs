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
    /// Groq (OpenAI-compatible, ultra-low latency).
    #[serde(rename = "groq")]
    Groq,
    /// Cohere (custom API).
    #[serde(rename = "cohere")]
    Cohere,
    /// Mistral AI (OpenAI-compatible).
    #[serde(rename = "mistral")]
    Mistral,
    /// Together AI (OpenAI-compatible, open models).
    #[serde(rename = "together")]
    Together,
    /// OpenRouter (OpenAI-compatible, multi-model aggregator).
    /// 透传 OpenAI 协议；自动注入 OpenRouter 推荐的
    /// `HTTP-Referer` 与 `X-Title` headers。
    #[serde(rename = "openrouter")]
    OpenRouter,
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
            ProviderKind::Groq => "groq",
            ProviderKind::Cohere => "cohere",
            ProviderKind::Mistral => "mistral",
            ProviderKind::Together => "together",
            ProviderKind::OpenRouter => "openrouter",
        }
    }

    /// 默认 API endpoint（OpenAI 兼容时直接复用 `OpenAiCompatibleProvider`）。
    pub fn default_endpoint(&self) -> Option<&'static str> {
        match self {
            ProviderKind::OpenAi => Some("https://api.openai.com/v1/chat/completions"),
            ProviderKind::Google => None, // 自定义协议
            ProviderKind::Anthropic => None, // 自定义协议
            ProviderKind::Groq => Some("https://api.groq.com/openai/v1/chat/completions"),
            ProviderKind::Cohere => None, // 自定义协议
            ProviderKind::Mistral => Some("https://api.mistral.ai/v1/chat/completions"),
            ProviderKind::Together => Some("https://api.together.xyz/v1/chat/completions"),
            ProviderKind::OpenRouter => Some("https://openrouter.ai/api/v1/chat/completions"),
        }
    }

    /// 推荐默认模型。OpenAI 兼容 provider 共享 `OpenAiCompatibleProvider` 客户端。
    pub fn default_model(&self) -> &'static str {
        match self {
            ProviderKind::OpenAi => "gpt-4o-mini",
            ProviderKind::Google => "gemini-1.5-flash",
            ProviderKind::Anthropic => "claude-3-5-sonnet-20241022",
            ProviderKind::Groq => "llama-3.1-70b-versatile",
            ProviderKind::Cohere => "command-r-plus",
            ProviderKind::Mistral => "mistral-large-latest",
            ProviderKind::Together => "meta-llama/Llama-3.3-70B-Instruct-Turbo",
            ProviderKind::OpenRouter => "anthropic/claude-3.5-sonnet",
        }
    }

    /// 是否可直接复用 `OpenAiCompatibleProvider`（无需新建 provider 文件）。
    pub fn is_openai_compatible(&self) -> bool {
        matches!(
            self,
            ProviderKind::OpenAi
                | ProviderKind::Groq
                | ProviderKind::Mistral
                | ProviderKind::Together
                | ProviderKind::OpenRouter
        )
    }
}

impl std::str::FromStr for ProviderKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "openai" | "open-ai" => Ok(ProviderKind::OpenAi),
            "google" | "gemini" => Ok(ProviderKind::Google),
            "anthropic" | "claude" => Ok(ProviderKind::Anthropic),
            "groq" => Ok(ProviderKind::Groq),
            "cohere" => Ok(ProviderKind::Cohere),
            "mistral" => Ok(ProviderKind::Mistral),
            "together" | "together-ai" => Ok(ProviderKind::Together),
            "openrouter" | "open-router" => Ok(ProviderKind::OpenRouter),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_round_trip_through_str() {
        for k in [
            ProviderKind::OpenAi,
            ProviderKind::Google,
            ProviderKind::Anthropic,
            ProviderKind::Groq,
            ProviderKind::Cohere,
            ProviderKind::Mistral,
            ProviderKind::Together,
        ] {
            let s = k.as_str();
            let parsed: ProviderKind = s.parse().expect("parse");
            assert_eq!(parsed, k);
        }
    }

    #[test]
    fn kind_aliases_resolve() {
        assert_eq!("open-ai".parse::<ProviderKind>().unwrap(), ProviderKind::OpenAi);
        assert_eq!("claude".parse::<ProviderKind>().unwrap(), ProviderKind::Anthropic);
        assert_eq!("gemini".parse::<ProviderKind>().unwrap(), ProviderKind::Google);
        assert_eq!("together-ai".parse::<ProviderKind>().unwrap(), ProviderKind::Together);
        assert_eq!("openrouter".parse::<ProviderKind>().unwrap(), ProviderKind::OpenRouter);
        assert_eq!("open-router".parse::<ProviderKind>().unwrap(), ProviderKind::OpenRouter);
        assert!("unknown-provider".parse::<ProviderKind>().is_err());
    }

    #[test]
    fn openai_compatible_classification() {
        assert!(ProviderKind::OpenAi.is_openai_compatible());
        assert!(ProviderKind::Groq.is_openai_compatible());
        assert!(ProviderKind::Mistral.is_openai_compatible());
        assert!(ProviderKind::Together.is_openai_compatible());
        assert!(!ProviderKind::Google.is_openai_compatible());
        assert!(!ProviderKind::Anthropic.is_openai_compatible());
        assert!(!ProviderKind::Cohere.is_openai_compatible());
    }

    #[test]
    fn default_endpoints_only_for_openai_compatible() {
        assert!(ProviderKind::OpenAi.default_endpoint().is_some());
        assert!(ProviderKind::Groq.default_endpoint().is_some());
        assert!(ProviderKind::Mistral.default_endpoint().is_some());
        assert!(ProviderKind::Together.default_endpoint().is_some());
        assert!(ProviderKind::OpenRouter.default_endpoint().is_some());
        assert!(ProviderKind::Google.default_endpoint().is_none());
        assert!(ProviderKind::Anthropic.default_endpoint().is_none());
        assert!(ProviderKind::Cohere.default_endpoint().is_none());
    }

    #[test]
    fn default_model_always_defined() {
        for k in [
            ProviderKind::OpenAi,
            ProviderKind::Google,
            ProviderKind::Anthropic,
            ProviderKind::Groq,
            ProviderKind::Cohere,
            ProviderKind::Mistral,
            ProviderKind::Together,
        ] {
            assert!(!k.default_model().is_empty());
        }
    }

    #[test]
    fn resolve_api_key_direct() {
        let cfg = ProviderConfig {
            name: "test".into(),
            kind: ProviderKind::OpenAi,
            endpoint: "https://api.openai.com/v1/chat/completions".into(),
            api_key: "sk-12345".into(),
            model: "gpt-4o-mini".into(),
            base_url: None,
            priority: 10,
            enabled: true,
            max_tokens: None,
            temperature: None,
        };
        assert_eq!(cfg.resolve_api_key(), "sk-12345");
    }

    #[test]
    fn resolve_api_key_from_env() {
        // 临时设置一个 env 变量
        std::env::set_var("SISYPHUS_TEST_KEY", "from-env-12345");
        let cfg = ProviderConfig {
            name: "test".into(),
            kind: ProviderKind::Groq,
            endpoint: "https://api.groq.com/openai/v1/chat/completions".into(),
            api_key: "env:SISYPHUS_TEST_KEY".into(),
            model: "llama-3.1-70b-versatile".into(),
            base_url: None,
            priority: 10,
            enabled: true,
            max_tokens: None,
            temperature: None,
        };
        assert_eq!(cfg.resolve_api_key(), "from-env-12345");
    }
}
