//! LLM abstraction layer for the Sisyphus tool ecosystem.
//!
//! Provides LLM client traits, providers, model routing, and circuit breaker
//! for managing LLM interactions across the Sisyphus toolbelt.

// `double_must_use`：`#[async_trait]` 会给 trait 的 async 方法加 `#[must_use]`，
// 而其返回类型（boxed Future）本身已带 `#[must_use]`，clippy 1.99.0 起判为冗余。
// 本 crate 的 provider / client trait 全部经由该宏生成，无法逐个方法挂 allow。
#![allow(clippy::double_must_use)]

pub mod circuit_breaker;
pub mod http_client;
pub mod llm;
pub mod metis;
pub mod model_router;
pub mod provider;

pub use llm::{
    BudgetProvider, DummyEmbeddingClient, DynamicLlmClient, EmbeddingClient, OpenAiEmbeddingClient,
    OpenAiLlmClient, RequestContext,
};
pub use metis::{
    Ambiguity, AmbiguitySeverity, DummyLlmClient, FixedResponseClient, ImplicitRequirement,
    LlmClient, MetisAnalysis, MetisError,
};
pub use provider::{
    build_provider_registry, AnthropicProvider, ChatMessage, ChatRole, GoogleGeminiProvider,
    LlmProvider, OpenAiCompatibleProvider, ProviderError, ProviderRegistry, ProviderResponse,
    RetryConfig,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_breaker_config_default() {
        let config = circuit_breaker::BreakerConfig::default();
        assert!(config.failure_threshold > 0);
    }

    #[test]
    fn test_model_config_creation() {
        let config = model_router::ModelConfig {
            name: "test-model".into(),
            cost_per_1k_tokens: 0.01,
            provider: "test-provider".into(),
            max_tokens: 1000,
            endpoint: "http://localhost".into(),
            complexity_ceiling: 50,
        };
        assert_eq!(config.name, "test-model");
    }
}
