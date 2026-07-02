//! LLM abstraction layer for the Sisyphus tool ecosystem.
//!
//! Provides LLM client traits, providers, model routing, and circuit breaker
//! for managing LLM interactions across the Sisyphus toolbelt.

pub mod circuit_breaker;
pub mod llm;
pub mod metis;
pub mod model_router;
pub mod provider;

pub use circuit_breaker::*;
pub use llm::*;
pub use metis::*;
pub use model_router::*;
pub use provider::{
    AnthropicProvider, build_provider_registry, ChatMessage, ChatRole, GoogleGeminiProvider,
    LlmProvider, OpenAiCompatibleProvider, ProviderError, ProviderRegistry, ProviderResponse,
    RetryConfig,
};
