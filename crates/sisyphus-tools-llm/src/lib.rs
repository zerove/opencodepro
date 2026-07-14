//! LLM abstraction layer for the Sisyphus tool ecosystem.
//!
//! Provides LLM client traits, providers, model routing, and circuit breaker
//! for managing LLM interactions across the Sisyphus toolbelt.

pub mod circuit_breaker;
pub mod llm;
pub mod metis;
pub mod model_router;
pub mod provider;

pub use llm::{
    BudgetProvider, DynamicLlmClient, OpenAiLlmClient, RequestContext,
    EmbeddingClient, DummyEmbeddingClient, OpenAiEmbeddingClient,
};
pub use metis::{
    Ambiguity, AmbiguitySeverity, DummyLlmClient, FixedResponseClient, ImplicitRequirement,
    LlmClient, MetisAnalysis, MetisError,
};
pub use provider::{
    AnthropicProvider, build_provider_registry, ChatMessage, ChatRole, GoogleGeminiProvider,
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

