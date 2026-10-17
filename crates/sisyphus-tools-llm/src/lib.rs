//! LLM abstraction layer for the Sisyphus tool ecosystem.
//!
//! Provides LLM client traits, providers, model routing, and circuit breaker
//! for managing LLM interactions across the Sisyphus toolbelt.

// `double_must_use`：`#[async_trait]` 会给 trait 的 async 方法加 `#[must_use]`，
// 而其返回类型（boxed Future）本身已带 `#[must_use]`，clippy 1.99.0 起判为冗余。
// 本 crate 的 provider / client trait 全部经由该宏生成，无法逐个方法挂 allow。
#![allow(clippy::double_must_use)]

pub mod circuit_breaker {
    pub use sisyphus_types::circuit_breaker::*;
}
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
    use std::sync::Mutex;

    /// **全 crate 唯一**的进程级 env 测试锁。
    ///
    /// 进程内所有线程共享同一份环境变量，所以只要两个用例中有一个在写 env，
    /// 另一个在读 env，就必须互斥——**与它们是否碰同一个变量名无关**。
    ///
    /// 此前 `llm.rs` / `provider.rs` / `http_client.rs` 各有一把独立 `ENV_LOCK`，
    /// 其正确性依赖"这三者碰的变量名恰好不相交"这一**隐式约定**。任何一次
    /// 重构让它们产生交集，竞争就会静默回归且极难定位。合并成一把锁后，
    /// 新增 env 用例只需 `use crate::tests::ENV_LOCK;` 一行即可，
    /// 而"该去哪个模块找锁"这个决策点从三处收敛到了一处。
    ///
    /// 注意：这**不是**编译器强制的。`std::env::set_var` 本身仍可在任何地方被
    /// 调用而不触发任何警告；本锁靠约定与评审，而非类型系统。
    ///
    /// 特别提醒：`SharedHttpClientConfig::from_env()` 是**无条件全量读取**全部
    /// `SISYPHUS_HTTP_*`，不是按需读取。所以 `http_client` 里"只写一个变量、
    /// 只断言一个字段"的用例同样会被其他用例的写入污染——**不能**按变量名
    /// 划分锁的粒度。
    pub(crate) static ENV_LOCK: Mutex<()> = Mutex::new(());

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
