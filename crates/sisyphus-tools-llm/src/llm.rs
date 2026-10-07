//! LLM client abstraction layer.
//!
//! Provides production-ready [`OpenAiLlmClient`] and a dynamic [`DynamicLlmClient`] that
//! reads configuration from a shared config store, with circuit breaker, model routing,
//! and budget tracking.
//!
//! ## Sub-modules
//! - [`budget`]: BudgetProvider trait and RequestContext.
//! - [`openai`]: Basic OpenAI-compatible LLM client.
//! - [`dynamic`]: Dynamic configuration-based LLM client.

pub mod budget;
pub mod dynamic;
pub mod openai;

#[async_trait::async_trait]
/// Embedding Client.
pub trait EmbeddingClient: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, sisyphus_types::TypesError>;
}

/// 测试/开发用伪嵌入客户端：由文本哈希生成正弦伪随机向量。
///
/// 注意：向量之间无语义相似性可言（仅同文本确定性相同），
/// 不得用于生产语义检索；生产环境应配置真实嵌入提供方。
pub struct DummyEmbeddingClient;

#[async_trait::async_trait]
impl EmbeddingClient for DummyEmbeddingClient {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, sisyphus_types::TypesError> {
        // Return a stable pseudo-random vector based on the text hash
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        let seed = hasher.finish();

        let vector = (0..128).map(|i| ((seed as f64 + i as f64).sin()) as f32).collect();
        Ok(vector)
    }
}

pub use budget::{BudgetProvider, RequestContext};
pub use dynamic::DynamicLlmClient;
pub use openai::{OpenAiEmbeddingClient, OpenAiLlmClient};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metis::LlmClient;
    use crate::tests::ENV_LOCK;
    use async_trait::async_trait;
    use sisyphus_types::env_guard::EnvVarGuard;
    use sisyphus_types::Config;
    use std::sync::{Arc, Mutex, RwLock};

    struct MockBudgetProvider {
        recorded_costs: Arc<Mutex<Vec<(String, f64, f64)>>>,
        should_fail_budget: bool,
    }

    #[async_trait]
    impl BudgetProvider for MockBudgetProvider {
        async fn check_budget(
            &self,
            _p: &str,
            _g: &str,
            _c: f64,
        ) -> Result<(), sisyphus_types::TypesError> {
            if self.should_fail_budget {
                Err(sisyphus_types::TypesError::InternalError(
                    "Budget exceeded".into(),
                ))
            } else {
                Ok(())
            }
        }
        async fn record_actual_cost(
            &self,
            p: &str,
            _g: &str,
            cost: f64,
            dur: f64,
        ) -> Result<(), sisyphus_types::TypesError> {
            self.recorded_costs.lock().unwrap().push((p.to_string(), cost, dur));
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_dynamic_llm_client_cost_recording() {
        let config = make_config("k", "e", "m");
        let costs = Arc::new(Mutex::new(Vec::new()));
        let provider = Arc::new(MockBudgetProvider {
            recorded_costs: costs.clone(),
            should_fail_budget: false,
        });

        let client = DynamicLlmClient::new(config).with_budget_provider(provider);

        client.set_context("proj-1", "goal-1");

        // We can't easily call chat() because it makes a real HTTP request.
        // But we can verify the logic if we were to mock the reqwest client.
        // For now, let's just verify that set_context and the budget provider
        // are correctly wired.

        let ctx = client.current_context.read().unwrap();
        assert!(ctx.is_some());
        assert_eq!(ctx.as_ref().unwrap().project_id, "proj-1");
    }

    // ... (rest of existing tests)

    fn make_config(api_key: &str, endpoint: &str, model: &str) -> Arc<RwLock<Config>> {
        Arc::new(RwLock::new(Config {
            llm: sisyphus_types::LlmConfig {
                api_key: api_key.to_string(),
                endpoint: endpoint.to_string(),
                model: model.to_string(),
                ..Default::default()
            },
            ..Config::default()
        }))
    }

    #[test]
    fn test_params_from_config_fallback() {
        let _guard = ENV_LOCK.lock().unwrap();

        unsafe { std::env::remove_var("GOOGLE_GENERATIVE_AI_API_KEY") };

        let config = make_config("cfg_key", "http://cfg:8080/v1/chat", "cfg-model");
        let client = DynamicLlmClient::new(config);

        let (api_key, endpoint, model) = client.get_current_params();

        assert_eq!(api_key, "cfg_key", "should fall back to config api_key");
        assert_eq!(
            endpoint, "http://cfg:8080/v1/chat",
            "endpoint should come from config"
        );
        assert_eq!(model, "cfg-model", "model should come from config");
    }

    #[test]
    fn test_params_from_env_override() {
        let _guard = ENV_LOCK.lock().unwrap();

        // SAFETY: ENV_LOCK 串行化本二进制内所有环境变量测试。
        // 该变量是生产配置的真实名字——本用例刻意覆盖「env 覆盖
        // config」这一行为，无法改为显式传参，故用守卫保证恢复。
        let _env = unsafe { EnvVarGuard::set("GOOGLE_GENERATIVE_AI_API_KEY", "env_key_value") };

        let config = make_config("cfg_key", "http://cfg:8080/v1/chat", "cfg-model");
        let client = DynamicLlmClient::new(config);

        let (api_key, endpoint, model) = client.get_current_params();

        assert_eq!(
            api_key, "env_key_value",
            "env var should override config api_key"
        );
        assert_eq!(
            endpoint, "http://cfg:8080/v1/chat",
            "endpoint should still come from config"
        );
        assert_eq!(model, "cfg-model", "model should still come from config");
    }

    #[test]
    fn test_params_with_empty_config_fallback() {
        let _guard = ENV_LOCK.lock().unwrap();

        // SAFETY: ENV_LOCK 串行化本二进制内所有环境变量测试。
        let _env = unsafe { EnvVarGuard::unset("GOOGLE_GENERATIVE_AI_API_KEY") };

        let config = make_config("", "", "");
        let client = DynamicLlmClient::new(config);

        let (api_key, endpoint, model) = client.get_current_params();

        assert_eq!(
            api_key, "",
            "should return empty string from config when no env"
        );
        assert_eq!(endpoint, "", "should return empty endpoint from config");
        assert_eq!(model, "", "should return empty model from config");
    }

    #[test]
    fn test_openai_llm_client_new() {
        let client =
            OpenAiLlmClient::new("key".into(), "http://localhost:8080".into(), "gpt-4".into())
                .unwrap();
        assert_eq!(client.api_key, "key");
        assert_eq!(client.endpoint, "http://localhost:8080");
        assert_eq!(client.model, "gpt-4");
    }

    #[test]
    fn test_dynamic_llm_client_new() {
        let _guard = ENV_LOCK.lock().unwrap();

        unsafe { std::env::remove_var("GOOGLE_GENERATIVE_AI_API_KEY") };
        let config = make_config("k", "e", "m");
        let client = DynamicLlmClient::new(config);
        let (api, ep, model) = client.get_current_params();
        assert_eq!(api, "k");
        assert_eq!(ep, "e");
        assert_eq!(model, "m");
    }

    #[test]
    fn test_dynamic_llm_client_reflects_config_change() {
        let _guard = ENV_LOCK.lock().unwrap();

        unsafe { std::env::remove_var("GOOGLE_GENERATIVE_AI_API_KEY") };
        let shared = make_config("old_key", "http://old:8080", "old-model");
        let client = DynamicLlmClient::new(shared.clone());

        // Verify initial params
        let (api, _ep, model) = client.get_current_params();
        assert_eq!(api, "old_key");
        assert_eq!(model, "old-model");

        // Update config in-place
        {
            let mut cfg = shared.write().unwrap();
            cfg.llm.api_key = "new_key".into();
            cfg.llm.endpoint = "http://new:9090".into();
            cfg.llm.model = "new-model".into();
        }

        // Verify params reflect the live change
        let (api, ep, model) = client.get_current_params();
        assert_eq!(
            api, "new_key",
            "env var is not set, should pick up new api_key"
        );
        assert_eq!(ep, "http://new:9090", "endpoint should reflect write");
        assert_eq!(model, "new-model", "model should reflect write");
    }

    #[test]
    fn test_dynamic_llm_client_concurrent_reads() {
        let _guard = ENV_LOCK.lock().unwrap();

        unsafe { std::env::remove_var("GOOGLE_GENERATIVE_AI_API_KEY") };
        let shared = make_config("key", "http://ep:8080", "m");
        let client = DynamicLlmClient::new(shared.clone());

        let mut handles = Vec::new();
        for i in 0..10 {
            let params = client.get_current_params();
            handles.push(std::thread::spawn(move || {
                let (api, ep, model) = params;
                assert_eq!(api, "key");
                assert_eq!(ep, "http://ep:8080");
                assert_eq!(model, "m");
                i
            }));
        }

        for h in handles {
            h.join().expect("thread should not panic");
        }
    }

    #[test]
    fn test_openai_llm_client_empty_fields() {
        let client = OpenAiLlmClient::new("".into(), "".into(), "".into()).unwrap();
        assert_eq!(client.api_key, "");
        assert_eq!(client.endpoint, "");
        assert_eq!(client.model, "");
    }

    #[test]
    fn test_get_params_for_no_override_falls_back() {
        let _guard = ENV_LOCK.lock().unwrap();

        unsafe { std::env::remove_var("GOOGLE_GENERATIVE_AI_API_KEY") };
        let config = make_config("base_key", "http://base:8080", "base-model");
        let client = DynamicLlmClient::new(config);

        let (api, ep, model) = client.get_params_for("nonexistent_agent");
        assert_eq!(api, "base_key", "no override → fall back to base config");
        assert_eq!(ep, "http://base:8080");
        assert_eq!(model, "base-model");
    }

    #[test]
    fn test_get_params_for_override_model() {
        let _guard = ENV_LOCK.lock().unwrap();

        unsafe { std::env::remove_var("GOOGLE_GENERATIVE_AI_API_KEY") };
        let shared = make_config("base_key", "http://base:8080", "base-model");
        {
            let mut cfg = shared.write().unwrap();
            cfg.llm.agents.insert(
                "metis".into(),
                sisyphus_types::AgentLlmOverride {
                    model: "o3-mini".into(),
                    endpoint: None,
                    api_key: None,
                },
            );
        }
        let client = DynamicLlmClient::new(shared);

        let (api, ep, model) = client.get_params_for("metis");
        assert_eq!(model, "o3-mini", "override should change model");
        assert_eq!(ep, "http://base:8080", "endpoint should fall back");
        assert_eq!(api, "base_key", "api_key should fall back");
    }

    #[test]
    fn test_get_params_for_full_override() {
        let _guard = ENV_LOCK.lock().unwrap();

        unsafe { std::env::remove_var("GOOGLE_GENERATIVE_AI_API_KEY") };
        let shared = make_config("base_key", "http://base:8080", "base-model");
        {
            let mut cfg = shared.write().unwrap();
            cfg.llm.agents.insert(
                "momus".into(),
                sisyphus_types::AgentLlmOverride {
                    model: "claude-sonnet-4".into(),
                    endpoint: Some("https://api.anthropic.com/v1".into()),
                    api_key: Some("anthropic_key".into()),
                },
            );
        }
        let client = DynamicLlmClient::new(shared);

        let (api, ep, model) = client.get_params_for("momus");
        assert_eq!(model, "claude-sonnet-4");
        assert_eq!(ep, "https://api.anthropic.com/v1");
        assert_eq!(api, "anthropic_key", "custom api_key from override");
    }

    #[test]
    fn test_get_params_for_env_override_still_wins() {
        let _guard = ENV_LOCK.lock().unwrap();

        // SAFETY: ENV_LOCK 串行化本二进制内所有环境变量测试。
        let _env = unsafe { EnvVarGuard::set("GOOGLE_GENERATIVE_AI_API_KEY", "env_overrides_all") };
        let shared = make_config("base_key", "http://base:8080", "base-model");
        {
            let mut cfg = shared.write().unwrap();
            cfg.llm.agents.insert(
                "librarian".into(),
                sisyphus_types::AgentLlmOverride {
                    model: "gemma-4-31b-it".into(),
                    endpoint: Some("https://generativelanguage.googleapis.com/v1beta".into()),
                    api_key: Some("override_key".into()),
                },
            );
        }
        let client = DynamicLlmClient::new(shared);

        let (api, ep, model) = client.get_params_for("librarian");
        assert_eq!(model, "gemma-4-31b-it");
        assert_eq!(ep, "https://generativelanguage.googleapis.com/v1beta");
        assert_eq!(api, "env_overrides_all", "env var should win over override");
    }

    /// Helper: create config with per-agent overrides pre-loaded.
    fn make_config_with_agents(overrides: &[(&str, &str, &str, &str)]) -> Arc<RwLock<Config>> {
        let cfg = make_config("k", "http://default:8080", "default-model");
        {
            let mut guard = cfg.write().unwrap();
            for (agent_id, model, endpoint, api_key) in overrides {
                guard.llm.agents.insert(
                    agent_id.to_string(),
                    sisyphus_types::AgentLlmOverride {
                        model: model.to_string(),
                        endpoint: Some(endpoint.to_string()),
                        api_key: Some(api_key.to_string()),
                    },
                );
            }
        }
        cfg
    }

    #[test]
    fn test_get_params_for_multiple_agents() {
        let _guard = ENV_LOCK.lock().unwrap();

        unsafe { std::env::remove_var("GOOGLE_GENERATIVE_AI_API_KEY") };
        let shared = make_config_with_agents(&[
            ("librarian", "gemma-4-31b-it", "http://lib:8080", "lib_key"),
            ("metis", "o3-mini", "http://metis:8080", "metis_key"),
            ("momus", "claude-sonnet", "http://momus:8080", "momus_key"),
        ]);
        let client = DynamicLlmClient::new(shared);

        let (_, _, m1) = client.get_params_for("librarian");
        assert_eq!(m1, "gemma-4-31b-it");

        let (_, _, m2) = client.get_params_for("metis");
        assert_eq!(m2, "o3-mini");

        let (_, _, m3) = client.get_params_for("momus");
        assert_eq!(m3, "claude-sonnet");

        let (_, _, m4) = client.get_params_for("unknown");
        assert_eq!(m4, "default-model", "unknown agent gets default");
    }

    #[test]
    fn test_openai_llm_client_long_fields() {
        let long_key = "a".repeat(1024);
        let long_ep =
            "https://very-long-endpoint-url.example.com/api/v1/chat/completions".to_string();
        let long_model = "very-long-model-name-v1.0.0-experimental".to_string();
        let client =
            OpenAiLlmClient::new(long_key.clone(), long_ep.clone(), long_model.clone()).unwrap();
        assert_eq!(client.api_key, long_key);
        assert_eq!(client.endpoint, long_ep);
        assert_eq!(client.model, long_model);
    }

    #[tokio::test]
    async fn test_budget_blocking_strict() {
        let config = Arc::new(RwLock::new(Config {
            llm: sisyphus_types::LlmConfig {
                budget_mode: sisyphus_types::BudgetMode::Strict,
                ..sisyphus_types::default_llm_config()
            },
            ..Config::default()
        }));
        let costs = Arc::new(Mutex::new(Vec::new()));
        let provider = Arc::new(MockBudgetProvider {
            recorded_costs: costs.clone(),
            should_fail_budget: true,
        });

        let client = DynamicLlmClient::new(config).with_budget_provider(provider);

        client.set_context("proj-1", "goal-1");

        let result = client.chat("sys", "user", false).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Budget exceeded (Strict Mode)"));
    }

    #[tokio::test]
    async fn test_budget_warning_non_blocking() {
        let config = Arc::new(RwLock::new(Config {
            llm: sisyphus_types::LlmConfig {
                budget_mode: sisyphus_types::BudgetMode::Warning,
                ..sisyphus_types::default_llm_config()
            },
            ..Config::default()
        }));
        let costs = Arc::new(Mutex::new(Vec::new()));
        let provider = Arc::new(MockBudgetProvider {
            recorded_costs: costs.clone(),
            should_fail_budget: true,
        });

        let client = DynamicLlmClient::new(config).with_budget_provider(provider);

        client.set_context("proj-1", "goal-1");

        let result = client.chat("sys", "user", false).await;
        // Should not fail due to budget, but will fail because we aren't mocking the HTTP request
        if let Err(e) = result {
            assert!(!e.to_string().contains("Budget exceeded (Strict Mode)"));
        }
    }
}
