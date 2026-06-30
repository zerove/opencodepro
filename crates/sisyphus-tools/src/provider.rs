//! Multi-provider LLM abstraction.
//!
//! Defines a common [`LlmProvider`] trait that each provider (OpenAI-compatible,
//! Google Gemini, Anthropic Claude, etc.) implements, plus a [`ProviderRegistry`]
//! that manages active providers and supports automatic fallback chaining.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sisyphus_types::{ProviderConfig, ProviderKind};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

// ── Provider trait ─────────────────────────────────────────────────────────

/// Result of a chat completion call.
#[derive(Debug, Clone)]
pub struct ProviderResponse {
    /// The generated text content.
    pub content: String,
    /// Model used (may differ from requested model if provider aliases).
    pub model: String,
    /// Input token count (if known).
    pub input_tokens: Option<u64>,
    /// Output token count (if known).
    pub output_tokens: Option<u64>,
}

/// Error from a provider call.
#[derive(Debug, Clone)]
pub enum ProviderError {
    /// Transient error — the caller should retry or fall back.
    Transient(String),
    /// Permanent error — do not retry with this provider.
    Permanent(String),
    /// Rate limited — wait before retrying.
    RateLimited(String),
    /// Authentication failure.
    Auth(String),
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderError::Transient(msg) => write!(f, "Transient: {msg}"),
            ProviderError::Permanent(msg) => write!(f, "Permanent: {msg}"),
            ProviderError::RateLimited(msg) => write!(f, "Rate limited: {msg}"),
            ProviderError::Auth(msg) => write!(f, "Auth: {msg}"),
        }
    }
}

impl std::error::Error for ProviderError {}

/// Abstract interface for an LLM provider.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Return the provider kind.
    fn kind(&self) -> ProviderKind;

    /// Return the provider name.
    fn name(&self) -> &str;

    /// Return the configured model name.
    fn model(&self) -> &str;

    /// Send a chat completion request.
    ///
    /// `system` — system prompt (may be empty).
    /// `messages` — conversation history plus the latest user message.
    /// `json_mode` — if true, request structured JSON output.
    async fn chat(
        &self,
        system: &str,
        messages: &[ChatMessage],
        json_mode: bool,
    ) -> Result<ProviderResponse, ProviderError>;
}

/// A single chat message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChatRole {
    System,
    User,
    Assistant,
}

impl ChatRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            ChatRole::System => "system",
            ChatRole::User => "user",
            ChatRole::Assistant => "assistant",
        }
    }
}

// ── OpenAI-compatible provider ─────────────────────────────────────────────

/// Provider that speaks the OpenAI chat completions protocol.
///
/// Compatible with: OpenAI, vLLM, Ollama, Groq, Together AI, Fireworks, etc.
pub struct OpenAiCompatibleProvider {
    config: ProviderConfig,
    client: reqwest::Client,
}

impl OpenAiCompatibleProvider {
    pub fn new(config: ProviderConfig) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| format!("Failed to build HTTP client: {e}"))?;
        Ok(Self { config, client })
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatibleProvider {
    fn kind(&self) -> ProviderKind {
        self.config.kind
    }

    fn name(&self) -> &str {
        &self.config.name
    }

    fn model(&self) -> &str {
        &self.config.model
    }

    async fn chat(
        &self,
        system: &str,
        messages: &[ChatMessage],
        json_mode: bool,
    ) -> Result<ProviderResponse, ProviderError> {
        let api_key = self.config.resolve_api_key();

        let mut formatted_messages: Vec<Value> = Vec::new();

        if !system.is_empty() {
            formatted_messages.push(json!({
                "role": "system",
                "content": system
            }));
        }

        for msg in messages {
            formatted_messages.push(json!({
                "role": msg.role.as_str(),
                "content": msg.content
            }));
        }

        let mut body = json!({
            "model": self.config.model,
            "messages": formatted_messages,
            "temperature": self.config.temperature.unwrap_or(0.1),
        });

        if let Some(maxt) = self.config.max_tokens {
            body["max_tokens"] = json!(maxt);
        }

        if json_mode {
            body["response_format"] = json!({ "type": "json_object" });
        }

        let res = self
            .client
            .post(&self.config.endpoint)
            .header("Authorization", format!("Bearer {}", api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Transient(format!("HTTP request failed: {e}")))?;

        let status = res.status();
        let response_json: Value = res
            .json()
            .await
            .map_err(|e| ProviderError::Transient(format!("Failed to parse response: {e}")))?;

        if !status.is_success() {
            let error_msg = response_json["error"]["message"]
                .as_str()
                .unwrap_or("unknown error");
            if status.as_u16() == 429 {
                return Err(ProviderError::RateLimited(error_msg.to_string()));
            }
            if status.as_u16() == 401 || status.as_u16() == 403 {
                return Err(ProviderError::Auth(error_msg.to_string()));
            }
            return Err(ProviderError::Transient(format!(
                "API error ({}): {}",
                status.as_u16(),
                error_msg
            )));
        }

        let content = response_json["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| ProviderError::Permanent("Response missing content".to_string()))?;

        let model = response_json["model"]
            .as_str()
            .unwrap_or(&self.config.model)
            .to_string();

        let input_tokens = response_json["usage"]["prompt_tokens"].as_u64();
        let output_tokens = response_json["usage"]["completion_tokens"].as_u64();

        Ok(ProviderResponse {
            content,
            model,
            input_tokens,
            output_tokens,
        })
    }
}

// ── Google Gemini provider ─────────────────────────────────────────────────

/// Provider that translates requests to Google Generative Language API format.
///
/// Uses the OpenAI-compatible endpoint at
/// `https://generativelanguage.googleapis.com/v1beta/openai/chat/completions`
/// which Gemini supports natively. Falls back to the native API if needed.
pub struct GoogleGeminiProvider {
    config: ProviderConfig,
    client: reqwest::Client,
}

impl GoogleGeminiProvider {
    pub fn new(config: ProviderConfig) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| format!("Failed to build HTTP client: {e}"))?;
        Ok(Self { config, client })
    }

}

#[async_trait]
impl LlmProvider for GoogleGeminiProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Google
    }

    fn name(&self) -> &str {
        &self.config.name
    }

    fn model(&self) -> &str {
        &self.config.model
    }

    async fn chat(
        &self,
        system: &str,
        messages: &[ChatMessage],
        json_mode: bool,
    ) -> Result<ProviderResponse, ProviderError> {
        let api_key = self.config.resolve_api_key();

        // Try OpenAI-compatible endpoint first (Gemini supports it natively)
        let endpoint = self.config.endpoint.clone();
        let mut formatted_messages: Vec<Value> = Vec::new();

        if !system.is_empty() {
            formatted_messages.push(json!({
                "role": "system",
                "content": system
            }));
        }

        for msg in messages {
            formatted_messages.push(json!({
                "role": msg.role.as_str(),
                "content": msg.content
            }));
        }

        let mut body = json!({
            "model": self.config.model,
            "messages": formatted_messages,
            "temperature": self.config.temperature.unwrap_or(0.1),
        });

        if let Some(maxt) = self.config.max_tokens {
            body["max_tokens"] = json!(maxt);
        }

        if json_mode {
            body["response_format"] = json!({ "type": "json_object" });
        }

        let res = self
            .client
            .post(&endpoint)
            .header("x-goog-api-key", &api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Transient(format!("HTTP request failed: {e}")))?;

        let status = res.status();
        let response_json: Value = res
            .json()
            .await
            .map_err(|e| ProviderError::Transient(format!("Failed to parse response: {e}")))?;

        if !status.is_success() {
            let error_msg = response_json["error"]["message"]
                .as_str()
                .or_else(|| {
                    response_json
                        .get("error")
                        .and_then(|e| e.as_str())
                })
                .unwrap_or("unknown error");
            if status.as_u16() == 429 {
                return Err(ProviderError::RateLimited(error_msg.to_string()));
            }
            return Err(ProviderError::Transient(format!(
                "API error ({}): {}",
                status.as_u16(),
                error_msg
            )));
        }

        // Parse both OpenAI-style and Gemini-native response formats
        let content = response_json["choices"][0]["message"]["content"]
            .as_str()
            .or_else(|| {
                response_json["candidates"][0]["content"]["parts"]
                    .as_array()
                    .and_then(|parts| parts.first())
                    .and_then(|p| p["text"].as_str())
            })
            .map(|s| s.to_string())
            .ok_or_else(|| {
                ProviderError::Permanent("Response missing content field".to_string())
            })?;

        let model = response_json["model"]
            .as_str()
            .unwrap_or(&self.config.model)
            .to_string();

        let input_tokens = response_json["usage"]["prompt_tokens"].as_u64();
        let output_tokens = response_json["usage"]["completion_tokens"].as_u64();

        Ok(ProviderResponse {
            content,
            model,
            input_tokens,
            output_tokens,
        })
    }
}

// ── Anthropic Claude provider ──────────────────────────────────────────────

/// Provider that translates requests to Anthropic Messages API format.
pub struct AnthropicProvider {
    config: ProviderConfig,
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(config: ProviderConfig) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(180))
            .build()
            .map_err(|e| format!("Failed to build HTTP client: {e}"))?;
        Ok(Self { config, client })
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Anthropic
    }

    fn name(&self) -> &str {
        &self.config.name
    }

    fn model(&self) -> &str {
        &self.config.model
    }

    async fn chat(
        &self,
        system: &str,
        messages: &[ChatMessage],
        _json_mode: bool,
    ) -> Result<ProviderResponse, ProviderError> {
        let api_key = self.config.resolve_api_key();

        // Anthropic uses a different message format
        let mut anthropic_messages: Vec<Value> = Vec::new();
        for msg in messages {
            anthropic_messages.push(json!({
                "role": if msg.role == ChatRole::Assistant { "assistant" } else { "user" },
                "content": msg.content
            }));
        }

        let mut body = json!({
            "model": self.config.model,
            "messages": anthropic_messages,
            "max_tokens": self.config.max_tokens.unwrap_or(4096),
            "temperature": self.config.temperature.unwrap_or(0.1),
        });

        // Anthropic puts system prompt at top level, not in messages array
        if !system.is_empty() {
            body["system"] = json!(system);
        }

        let res = self
            .client
            .post(&self.config.endpoint)
            .header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Transient(format!("HTTP request failed: {e}")))?;

        let status = res.status();
        let response_json: Value = res
            .json()
            .await
            .map_err(|e| ProviderError::Transient(format!("Failed to parse response: {e}")))?;

        if !status.is_success() {
            let error_msg = response_json["error"]["message"]
                .as_str()
                .unwrap_or("unknown error");
            if status.as_u16() == 429 {
                return Err(ProviderError::RateLimited(error_msg.to_string()));
            }
            if status.as_u16() == 401 {
                return Err(ProviderError::Auth(error_msg.to_string()));
            }
            return Err(ProviderError::Transient(format!(
                "API error ({}): {}",
                status.as_u16(),
                error_msg
            )));
        }

        // Anthropic response format: { content: [{ type: "text", text: "..." }] }
        let content = response_json["content"]
            .as_array()
            .and_then(|arr| {
                arr.iter()
                    .find(|c| c["type"] == "text")
                    .and_then(|c| c["text"].as_str())
            })
            .map(|s| s.to_string())
            .ok_or_else(|| {
                ProviderError::Permanent("Response missing text content".to_string())
            })?;

        let model = response_json["model"]
            .as_str()
            .unwrap_or(&self.config.model)
            .to_string();

        let input_tokens = response_json["usage"]["input_tokens"].as_u64();
        let output_tokens = response_json["usage"]["output_tokens"].as_u64();

        Ok(ProviderResponse {
            content,
            model,
            input_tokens,
            output_tokens,
        })
    }
}

// ── Provider Registry ──────────────────────────────────────────────────────

/// Manages multiple LLM providers and supports fallback chains.
pub struct ProviderRegistry {
    /// All registered providers, keyed by name.
    providers: HashMap<String, Arc<dyn LlmProvider>>,
    /// Ordered list of provider names for fallback (by priority).
    fallback_order: Vec<String>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
            fallback_order: Vec::new(),
        }
    }

    /// Register a provider. If a provider with the same name exists, it is replaced.
    pub fn register(&mut self, provider: Arc<dyn LlmProvider>) {
        let name = provider.name().to_string();
        self.providers.insert(name.clone(), provider);

        // Rebuild fallback order if not already present
        if !self.fallback_order.contains(&name) {
            self.fallback_order.push(name);
        }
    }

    /// Get a provider by name.
    pub fn get(&self, name: &str) -> Option<Arc<dyn LlmProvider>> {
        self.providers.get(name).cloned()
    }

    /// Get all registered provider names.
    pub fn list(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }

    /// Remove a provider by name.
    pub fn remove(&mut self, name: &str) -> bool {
        self.fallback_order.retain(|n| n != name);
        self.providers.remove(name).is_some()
    }

    /// Register a fallback chain: providers are tried in priority order.
    ///
    /// This replaces any existing fallback order.
    pub fn set_fallback_order(&mut self, order: Vec<String>) {
        self.fallback_order = order;
    }

    /// Get the current fallback order.
    pub fn fallback_order(&self) -> &[String] {
        &self.fallback_order
    }

    /// Call the first available provider with automatic fallback.
    ///
    /// On transient error or rate-limit, the next provider in the chain is tried.
    /// Permanent errors and auth errors are returned immediately.
    pub async fn chat_with_fallback(
        &self,
        system: &str,
        messages: &[ChatMessage],
        json_mode: bool,
    ) -> Result<ProviderResponse, ProviderError> {
        let mut last_error = None;

        for name in &self.fallback_order {
            if let Some(provider) = self.providers.get(name) {
                match provider.chat(system, messages, json_mode).await {
                    Ok(response) => return Ok(response),
                    Err(ProviderError::Transient(e)) => {
                        last_error = Some(ProviderError::Transient(format!(
                            "{}: {e}",
                            provider.name()
                        )));
                        continue; // try next
                    }
                    Err(ProviderError::RateLimited(e)) => {
                        last_error = Some(ProviderError::RateLimited(format!(
                            "{}: {e}",
                            provider.name()
                        )));
                        continue; // try next
                    }
                    Err(e @ ProviderError::Permanent(_)) => return Err(e),
                    Err(e @ ProviderError::Auth(_)) => return Err(e),
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            ProviderError::Permanent("No providers available in fallback chain".to_string())
        }))
    }

    /// Call a specific provider by name.
    pub async fn chat_with_provider(
        &self,
        name: &str,
        system: &str,
        messages: &[ChatMessage],
        json_mode: bool,
    ) -> Result<ProviderResponse, ProviderError> {
        match self.providers.get(name) {
            Some(provider) => provider.chat(system, messages, json_mode).await,
            None => Err(ProviderError::Permanent(format!(
                "Provider '{name}' not found"
            ))),
        }
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Build the default provider registry from config.
pub fn build_provider_registry(
    providers_cfg: &[ProviderConfig],
) -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();
    let mut fallback_order: Vec<String> = Vec::new();

    let mut sorted = providers_cfg.to_vec();
    sorted.sort_by_key(|p| p.priority);

    for cfg in sorted {
        if !cfg.enabled {
            continue;
        }

        let provider: Result<Arc<dyn LlmProvider>, String> = match cfg.kind {
            ProviderKind::OpenAi | ProviderKind::Google => {
                if cfg.kind == ProviderKind::Google {
                    GoogleGeminiProvider::new(cfg.clone()).map(|p| Arc::new(p) as Arc<dyn LlmProvider>)
                } else {
                    OpenAiCompatibleProvider::new(cfg.clone())
                        .map(|p| Arc::new(p) as Arc<dyn LlmProvider>)
                }
            }
            ProviderKind::Anthropic => AnthropicProvider::new(cfg.clone())
                .map(|p| Arc::new(p) as Arc<dyn LlmProvider>),
        };

        match provider {
            Ok(p) => {
                let name = p.name().to_string();
                registry.register(p);
                fallback_order.push(name);
            }
            Err(e) => {
                eprintln!("[provider] Failed to build '{}': {e}", cfg.name);
            }
        }
    }

    registry.set_fallback_order(fallback_order);
    registry
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_openai_config() -> ProviderConfig {
        ProviderConfig {
            name: "test-openai".into(),
            kind: ProviderKind::OpenAi,
            endpoint: "http://localhost:9999/v1/chat/completions".into(),
            api_key: "test-key".into(),
            model: "gpt-4o-mini".into(),
            base_url: None,
            priority: 10,
            enabled: true,
            max_tokens: Some(1024),
            temperature: Some(0.1),
        }
    }

    #[test]
    fn test_provider_kind_from_str() {
        use std::str::FromStr;
        assert_eq!(ProviderKind::from_str("openai"), Ok(ProviderKind::OpenAi));
        assert_eq!(ProviderKind::from_str("google"), Ok(ProviderKind::Google));
        assert_eq!(ProviderKind::from_str("gemini"), Ok(ProviderKind::Google));
        assert_eq!(
            ProviderKind::from_str("anthropic"),
            Ok(ProviderKind::Anthropic)
        );
        assert_eq!(ProviderKind::from_str("claude"), Ok(ProviderKind::Anthropic));
        assert!(ProviderKind::from_str("unknown").is_err());
    }

    #[test]
    fn test_provider_kind_display() {
        assert_eq!(ProviderKind::OpenAi.to_string(), "openai");
        assert_eq!(ProviderKind::Google.to_string(), "google");
        assert_eq!(ProviderKind::Anthropic.to_string(), "anthropic");
    }

    #[test]
    fn test_resolve_api_key_direct() {
        let cfg = ProviderConfig {
            api_key: "sk-abc123".into(),
            ..make_openai_config()
        };
        assert_eq!(cfg.resolve_api_key(), "sk-abc123");
    }

    #[test]
    fn test_resolve_api_key_env_var() {
        let cfg = ProviderConfig {
            api_key: "env:MY_TEST_KEY".into(),
            ..make_openai_config()
        };
        // env var not set, should return empty
        assert_eq!(cfg.resolve_api_key(), "");
    }

    #[test]
    fn test_resolve_api_key_env_var_set() {
        unsafe { std::env::set_var("MY_TEST_KEY_42", "resolved-value") };
        let cfg = ProviderConfig {
            api_key: "env:MY_TEST_KEY_42".into(),
            ..make_openai_config()
        };
        assert_eq!(cfg.resolve_api_key(), "resolved-value");
        unsafe { std::env::remove_var("MY_TEST_KEY_42") };
    }

    #[test]
    fn test_provider_registry_register_and_list() {
        let mut registry = ProviderRegistry::new();
        assert!(registry.list().is_empty());

        let provider = OpenAiCompatibleProvider::new(make_openai_config()).unwrap();
        registry.register(Arc::new(provider));

        let names = registry.list();
        assert_eq!(names.len(), 1);
        assert!(names.contains(&"test-openai".to_string()));
    }

    #[test]
    fn test_provider_registry_get_missing() {
        let registry = ProviderRegistry::new();
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_provider_registry_remove() {
        let mut registry = ProviderRegistry::new();
        let provider = OpenAiCompatibleProvider::new(make_openai_config()).unwrap();
        registry.register(Arc::new(provider));

        assert!(registry.remove("test-openai"));
        assert!(registry.list().is_empty());
        assert!(!registry.remove("test-openai")); // already removed
    }

    #[tokio::test]
    async fn test_provider_registry_fallback_order() {
        let mut registry = ProviderRegistry::new();
        registry.set_fallback_order(vec![
            "primary".into(),
            "secondary".into(),
        ]);
        assert_eq!(registry.fallback_order().len(), 2);

        // If no providers match fallback, chat_with_fallback returns error
        let result = registry
            .chat_with_fallback("sys", &[], false)
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn test_openai_compatible_provider_new() {
        let provider = OpenAiCompatibleProvider::new(make_openai_config()).unwrap();
        assert_eq!(provider.name(), "test-openai");
        assert_eq!(provider.kind(), ProviderKind::OpenAi);
        assert_eq!(provider.model(), "gpt-4o-mini");
    }

    #[test]
    fn test_google_gemini_provider_new() {
        let cfg = ProviderConfig {
            name: "test-gemini".into(),
            kind: ProviderKind::Google,
            endpoint: "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
                .into(),
            api_key: "env:GOOGLE_GENERATIVE_AI_API_KEY".into(),
            model: "gemini-2.0-flash".into(),
            base_url: None,
            priority: 20,
            enabled: true,
            max_tokens: Some(4096),
            temperature: Some(0.3),
        };
        let provider = GoogleGeminiProvider::new(cfg).unwrap();
        assert_eq!(provider.name(), "test-gemini");
        assert_eq!(provider.kind(), ProviderKind::Google);
        assert_eq!(provider.model(), "gemini-2.0-flash");
    }

    #[test]
    fn test_anthropic_provider_new() {
        let cfg = ProviderConfig {
            name: "test-claude".into(),
            kind: ProviderKind::Anthropic,
            endpoint: "https://api.anthropic.com/v1/messages".into(),
            api_key: "env:ANTHROPIC_API_KEY".into(),
            model: "claude-sonnet-4-20250514".into(),
            base_url: None,
            priority: 5,
            enabled: true,
            max_tokens: Some(8192),
            temperature: Some(0.2),
        };
        let provider = AnthropicProvider::new(cfg).unwrap();
        assert_eq!(provider.name(), "test-claude");
        assert_eq!(provider.kind(), ProviderKind::Anthropic);
        assert_eq!(provider.model(), "claude-sonnet-4-20250514");
    }

    #[test]
    fn test_provider_config_serialize_roundtrip() {
        let cfg = make_openai_config();
        let json = serde_json::to_string(&cfg).unwrap();
        let deserialized: ProviderConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, cfg.name);
        assert_eq!(deserialized.kind, cfg.kind);
        assert_eq!(deserialized.model, cfg.model);
    }

    #[test]
    fn test_build_provider_registry_sorts_by_priority() {
        let cfgs = vec![
            ProviderConfig {
                name: "low-pri".into(),
                priority: 100,
                enabled: true,
                ..make_openai_config()
            },
            ProviderConfig {
                name: "high-pri".into(),
                priority: 1,
                enabled: true,
                ..make_openai_config()
            },
        ];
        let registry = build_provider_registry(&cfgs);
        let order = registry.fallback_order();
        // high-pri (priority 1) should come before low-pri (priority 100)
        assert_eq!(order[0], "high-pri");
        assert_eq!(order[1], "low-pri");
    }

    #[test]
    fn test_build_provider_registry_skips_disabled() {
        let cfgs = vec![
            ProviderConfig {
                name: "enabled-one".into(),
                enabled: true,
                ..make_openai_config()
            },
            ProviderConfig {
                name: "disabled-one".into(),
                enabled: false,
                ..make_openai_config()
            },
        ];
        let registry = build_provider_registry(&cfgs);
        let names = registry.list();
        assert!(names.contains(&"enabled-one".to_string()));
        assert!(!names.contains(&"disabled-one".to_string()));
    }

    #[test]
    fn test_chat_role_as_str() {
        assert_eq!(ChatRole::System.as_str(), "system");
        assert_eq!(ChatRole::User.as_str(), "user");
        assert_eq!(ChatRole::Assistant.as_str(), "assistant");
    }

    #[test]
    fn test_provider_message_serde() {
        let msg = ChatMessage {
            role: ChatRole::User,
            content: "Hello".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: ChatMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.role, ChatRole::User);
        assert_eq!(deserialized.content, "Hello");
    }

    #[test]
    fn test_provider_error_display() {
        let err = ProviderError::Transient("timeout".into());
        assert_eq!(err.to_string(), "Transient: timeout");

        let err = ProviderError::Permanent("bad request".into());
        assert_eq!(err.to_string(), "Permanent: bad request");

        let err = ProviderError::RateLimited("too many".into());
        assert_eq!(err.to_string(), "Rate limited: too many");

        let err = ProviderError::Auth("invalid key".into());
        assert_eq!(err.to_string(), "Auth: invalid key");
    }
}
