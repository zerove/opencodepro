//! Multi-provider LLM abstraction.
//!
//! Defines a common [`LlmProvider`] trait that each provider (OpenAI-compatible,
//! Google Gemini, Anthropic Claude, etc.) implements, plus a [`ProviderRegistry`]
//! that manages active providers and supports automatic fallback chaining.
//!
//! ## Sub-modules
//! - [`types`]: Shared types (ProviderResponse, ProviderError, ChatMessage, ChatRole).
//! - [`openai`]: OpenAI-compatible provider (OpenAI, vLLM, Ollama, Groq, etc.).
//! - [`gemini`]: Google Gemini provider.
//! - [`anthropic`]: Anthropic Claude provider.
//! - [`registry`]: Provider registry with fallback support.

pub mod anthropic;
pub mod gemini;
pub mod openai;
pub mod registry;
pub mod types;

pub use anthropic::AnthropicProvider;
pub use gemini::GoogleGeminiProvider;
pub use openai::OpenAiCompatibleProvider;
pub use registry::{build_provider_registry, ProviderRegistry};
pub use types::{ChatMessage, ChatRole, ProviderError, ProviderResponse, RetryConfig};

use async_trait::async_trait;
use sisyphus_types::ProviderKind;

// ── Provider trait ─────────────────────────────────────────────────────────

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

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sisyphus_types::ProviderConfig;
    use std::sync::{Arc, Mutex};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct MockProvider {
        name: String,
        fail_until: usize,
        current_attempt: std::sync::atomic::AtomicUsize,
        error_to_return: ProviderError,
        success_response: ProviderResponse,
    }

    #[async_trait]
    impl LlmProvider for MockProvider {
        fn kind(&self) -> ProviderKind { ProviderKind::OpenAi }
        fn name(&self) -> &str { &self.name }
        fn model(&self) -> &str { "mock-model" }
        async fn chat(&self, _s: &str, _m: &[ChatMessage], _j: bool) -> Result<ProviderResponse, ProviderError> {
            let attempt = self.current_attempt.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if attempt < self.fail_until {
                Err(self.error_to_return.clone())
            } else {
                Ok(self.success_response.clone())
            }
        }
    }

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
        let _guard = ENV_LOCK.lock().unwrap();
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

    #[tokio::test]
    async fn test_chat_with_fallback_retries_transient() {
        let mut registry = ProviderRegistry::new();
        let mock = Arc::new(MockProvider {
            name: "mock-transient".into(),
            fail_until: 2,
            current_attempt: std::sync::atomic::AtomicUsize::new(0),
            error_to_return: ProviderError::Transient("temporary failure".into()),
            success_response: ProviderResponse {
                content: "success".into(),
                model: "mock-model".into(),
                input_tokens: None,
                output_tokens: None,
            },
        });
        registry.register(mock.clone());

        let res = registry.chat_with_fallback("sys", &[], false).await;
        assert!(res.is_ok());
        assert_eq!(res.unwrap().content, "success");
        assert_eq!(mock.current_attempt.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_chat_with_fallback_retries_rate_limited() {
        let mut registry = ProviderRegistry::new();
        let mock = Arc::new(MockProvider {
            name: "mock-rate".into(),
            fail_until: 2,
            current_attempt: std::sync::atomic::AtomicUsize::new(0),
            error_to_return: ProviderError::RateLimited("too many requests".into()),
            success_response: ProviderResponse {
                content: "success".into(),
                model: "mock-model".into(),
                input_tokens: None,
                output_tokens: None,
            },
        });
        registry.register(mock.clone());

        let res = registry.chat_with_fallback("sys", &[], false).await;
        assert!(res.is_ok());
        assert_eq!(res.unwrap().content, "success");
        assert_eq!(mock.current_attempt.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_chat_with_fallback_no_retry_permanent() {
        let mut registry = ProviderRegistry::new();
        let mock = Arc::new(MockProvider {
            name: "mock-perm".into(),
            fail_until: 2,
            current_attempt: std::sync::atomic::AtomicUsize::new(0),
            error_to_return: ProviderError::Permanent("fatal error".into()),
            success_response: ProviderResponse {
                content: "success".into(),
                model: "mock-model".into(),
                input_tokens: None,
                output_tokens: None,
            },
        });
        registry.register(mock.clone());

        let res = registry.chat_with_fallback("sys", &[], false).await;
        assert!(res.is_err());
        match res.unwrap_err() {
            ProviderError::Permanent(e) => assert!(e.contains("fatal error")),
            _ => panic!("Expected Permanent error"),
        }
        assert_eq!(mock.current_attempt.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_chat_with_fallback_exhausts_retries_and_falls_back() {
        let mut registry = ProviderRegistry::new();

        let mock1 = Arc::new(MockProvider {
            name: "mock-fail".into(),
            fail_until: 10,
            current_attempt: std::sync::atomic::AtomicUsize::new(0),
            error_to_return: ProviderError::Transient("fail".into()),
            success_response: ProviderResponse {
                content: "never".into(),
                model: "mock-model".into(),
                input_tokens: None,
                output_tokens: None,
            },
        });

        let mock2 = Arc::new(MockProvider {
            name: "mock-success".into(),
            fail_until: 0,
            current_attempt: std::sync::atomic::AtomicUsize::new(0),
            error_to_return: ProviderError::Transient("fail".into()),
            success_response: ProviderResponse {
                content: "success".into(),
                model: "mock-model".into(),
                input_tokens: None,
                output_tokens: None,
            },
        });

        registry.register(mock1.clone());
        registry.register(mock2.clone());
        registry.set_fallback_order(vec!["mock-fail".into(), "mock-success".into()]);

        let res = registry.chat_with_fallback("sys", &[], false).await;
        assert!(res.is_ok());
        assert_eq!(res.unwrap().content, "success");

        assert_eq!(mock1.current_attempt.load(std::sync::atomic::Ordering::SeqCst), 4);
        assert_eq!(mock2.current_attempt.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
