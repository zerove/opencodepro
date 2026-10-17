//! Transport-agnostic LLM abstractions.
//!
//! These traits define *what* the engine asks of an LLM, not how calls are
//! made. They live in `sisyphus-types` so that `sisyphus-core` can name them
//! without depending on an LLM implementation crate; `sisyphus-tools-llm`
//! re-exports them so existing paths keep resolving (ADR-014 phase 3).

use crate::{CircuitBreaker, HttpClientStats, TypesError};

/// Run chat completions.
///
/// Implementations are expected to honour the per-agent rate limit and circuit
/// breaker reachable via [`Self::breaker`].
#[async_trait::async_trait]
pub trait LlmClient: Send + Sync {
    /// Run a chat completion.
    ///
    /// * `system` — System prompt; prepended regardless of `json_mode`.
    /// * `user` — The user message; if `json_mode` is true the system prompt is
    ///   augmented to instruct the model to reply with strict JSON.
    /// * `json_mode` — Hint to enforce JSON output (e.g. tool/function calling).
    async fn chat(&self, system: &str, user: &str, json_mode: bool) -> Result<String, TypesError>;

    /// Run a single-prompt completion.
    async fn generate(&self, prompt: &str) -> Result<String, TypesError>;

    /// Resolve the provider/model/api_key triple for an agent id.
    fn get_params_for(&self, agent_id: &str) -> (String, String, String);

    /// Snapshot of the circuit breaker gating outbound calls.
    fn breaker(&self) -> CircuitBreaker;

    /// Close the client and any shared connection pool.
    ///
    /// Defaults to a no-op so test doubles need not implement it.
    async fn shutdown(&self, _wait: std::time::Duration) -> Result<(), String> {
        Ok(())
    }

    /// Transport counters for the shared outbound HTTP client, if any.
    ///
    /// Deliberately returns plain counters rather than the client handle: a
    /// client type wraps `reqwest`, and naming it here would pull that into
    /// every crate that only needs the trait. `None` when the implementation
    /// owns no shared client (mocks, hand-built registries).
    fn http_stats(&self) -> Option<HttpClientStats> {
        None
    }
}

/// Turn text into a vector.
#[async_trait::async_trait]
pub trait EmbeddingClient: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, TypesError>;
}

/// Link LLM usage to project budgets.
#[async_trait::async_trait]
pub trait BudgetProvider: Send + Sync {
    /// Verify an estimated cost fits the projected budget.
    async fn check_budget(
        &self,
        project_id: &str,
        goal_id: &str,
        estimated_cost: f64,
    ) -> Result<(), TypesError>;

    /// Record actual cost and duration for a goal.
    async fn record_actual_cost(
        &self,
        project_id: &str,
        goal_id: &str,
        actual_cost: f64,
        duration_secs: f64,
    ) -> Result<(), TypesError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Stub;

    #[async_trait::async_trait]
    impl LlmClient for Stub {
        async fn chat(&self, _s: &str, _u: &str, _j: bool) -> Result<String, TypesError> {
            Ok(String::new())
        }
        async fn generate(&self, _p: &str) -> Result<String, TypesError> {
            Ok(String::new())
        }
        fn get_params_for(&self, _a: &str) -> (String, String, String) {
            (String::new(), String::new(), String::new())
        }
        fn breaker(&self) -> CircuitBreaker {
            CircuitBreaker::default()
        }
    }

    #[tokio::test]
    async fn default_http_stats_is_none() {
        let c: Box<dyn LlmClient> = Box::new(Stub);
        assert!(
            c.http_stats().is_none(),
            "implementations without a shared client report None"
        );
    }

    #[tokio::test]
    async fn default_shutdown_is_ok() {
        let c: Box<dyn LlmClient> = Box::new(Stub);
        assert!(c.shutdown(std::time::Duration::from_millis(1)).await.is_ok());
    }
}
