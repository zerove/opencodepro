use async_trait::async_trait;
use serde_json::{json, Value};
use crate::metis::LlmClient;
use crate::circuit_breaker::CircuitBreaker;
use crate::model_router::{ModelRouter, ModelConfig};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use sisyphus_types::Config;

/// A production-ready LLM client compatible with OpenAI-style APIs (e.g., vLLM, Ollama).
pub struct OpenAiLlmClient {
    pub api_key: String,
    pub endpoint: String,
    pub model: String,
    client: reqwest::Client,
}

impl OpenAiLlmClient {
    pub fn new(api_key: String, endpoint: String, model: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("Failed to build reqwest::Client");
        Self {
            api_key,
            endpoint,
            model,
            client,
        }
    }
}

#[async_trait]
impl LlmClient for OpenAiLlmClient {
    async fn chat(&self, system: &str, user: &str, json_mode: bool) -> Result<String, String> {
        let client = &self.client;
        
        let messages = vec![
            json!({ "role": "system", "content": system }),
            json!({ "role": "user", "content": user }),
        ];

        let mut body = json!({
            "model": self.model,
            "messages": messages,
            "temperature": 0.1, // Set to very low for high-precision tools
        });

        if json_mode {
            body["response_format"] = json!({ "type": "json_object" });
        }

        let res = client.post(&self.endpoint)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {e}"))?;

        let status = res.status();
        let response_json: Value = res.json().await.map_err(|e| format!("Failed to parse response JSON: {e}"))?;

        if !status.is_success() {
            // Google AI Studio wraps errors in an array: [{ "error": { "message": "...", ... } }]
            // Standard OpenAI returns: { "error": { "message": "...", ... } }
            let error_obj = if response_json.is_array() {
                &response_json[0]["error"]
            } else {
                &response_json["error"]
            };
            let error_msg = error_obj["message"]
                .as_str()
                .unwrap_or("unknown error");
            return Err(format!("LLM API error ({}): {}", status.as_u16(), error_msg));
        }

        response_json["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "LLM response missing content field".to_string())
    }
}

/// A proxy client that retrieves LLM configuration dynamically from a shared config store.
///
/// Supports per-agent model overrides via `Config.llm.agents`.
///
/// Integrates a [`CircuitBreaker`] to prevent cascading failures and a [`ModelRouter`]
/// for cost tracking (model selection can be wired on top of the existing config-based resolution).
pub struct DynamicLlmClient {
    config: Arc<RwLock<Config>>,
    client: reqwest::Client,
    breaker: Arc<CircuitBreaker>,
    router: Arc<ModelRouter>,
}

impl DynamicLlmClient {
    pub fn new(config: Arc<RwLock<Config>>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("Failed to build reqwest::Client");
        Self {
            config,
            client,
            breaker: Arc::new(CircuitBreaker::default()),
            router: Arc::new(ModelRouter::new(0.0)),
        }
    }

    pub fn with_breaker(mut self, breaker: Arc<CircuitBreaker>) -> Self {
        self.breaker = breaker;
        self
    }

    pub fn with_router(mut self, router: Arc<ModelRouter>) -> Self {
        self.router = router;
        self
    }

    pub fn breaker(&self) -> &Arc<CircuitBreaker> {
        &self.breaker
    }

    pub fn router(&self) -> &Arc<ModelRouter> {
        &self.router
    }

    fn resolve_api_key(cfg_api_key: &str) -> String {
        std::env::var("GOOGLE_GENERATIVE_AI_API_KEY")
            .unwrap_or_else(|_| cfg_api_key.to_string())
    }

    /// Get base (default) LLM parameters.
    fn get_current_params(&self) -> (String, String, String) {
        let cfg = self.config.read().unwrap_or_else(|e| e.into_inner());
        let api_key = Self::resolve_api_key(&cfg.llm.api_key);
        (api_key, cfg.llm.endpoint.clone(), cfg.llm.model.clone())
    }

    /// Get LLM parameters for a specific agent.
    ///
    /// If the agent has a model override in `Config.llm.agents`, its
    /// endpoint / model / api_key take precedence.  Fallback fields
    /// come from the base config.
    pub fn get_params_for(&self, agent_id: &str) -> (String, String, String) {
        let cfg = self.config.read().unwrap_or_else(|e| e.into_inner());

        if let Some(override_) = cfg.llm.agents.get(agent_id) {
            let endpoint = override_.endpoint.as_deref().unwrap_or(&cfg.llm.endpoint).to_string();
            let api_key = match &override_.api_key {
                Some(k) => Self::resolve_api_key(k),
                None => Self::resolve_api_key(&cfg.llm.api_key),
            };
            let model = override_.model.clone();
            (api_key, endpoint, model)
        } else {
            let api_key = Self::resolve_api_key(&cfg.llm.api_key);
            (api_key, cfg.llm.endpoint.clone(), cfg.llm.model.clone())
        }
    }
}

#[async_trait]
impl LlmClient for DynamicLlmClient {
    async fn chat(&self, system: &str, user: &str, json_mode: bool) -> Result<String, String> {
        let (api_key, endpoint, model) = self.get_current_params();

        // Auto-register the current model with the router for cost tracking
        self.router.register_model(ModelConfig {
            name: model.clone(),
            cost_per_1k_tokens: 0.01, // sensible default; overridable via with_router()
            provider: "default".into(),
            max_tokens: 128_000,
            endpoint: endpoint.clone(),
            complexity_ceiling: 100,
        });

        // Check circuit breaker before making the call
        self.breaker.check_state(&model)
            .map_err(|e| format!("LLM call blocked by circuit breaker ({}): {e}", model))?;

        let messages = vec![
            json!({ "role": "system", "content": system }),
            json!({ "role": "user", "content": user }),
        ];

        let mut body = json!({
            "model": model,
            "messages": messages,
            "temperature": 0.1,
        });

        if json_mode {
            body["response_format"] = json!({ "type": "json_object" });
        }

        let res = match self.client.post(&endpoint)
            .header("Authorization", format!("Bearer {}", api_key))
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                self.breaker.on_failure(&model);
                return Err(format!("HTTP request failed: {e}"));
            }
        };

        let status = res.status();
        let response_json: Value = match res.json().await {
            Ok(j) => j,
            Err(e) => {
                self.breaker.on_failure(&model);
                return Err(format!("Failed to parse response JSON: {e}"));
            }
        };

        if !status.is_success() {
            self.breaker.on_failure(&model);
            let error_obj = if response_json.is_array() {
                &response_json[0]["error"]
            } else {
                &response_json["error"]
            };
            let error_msg = error_obj["message"]
                .as_str()
                .unwrap_or("unknown error");
            return Err(format!("LLM API error ({}): {}", status.as_u16(), error_msg));
        }

        let content = response_json["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string());

        match content {
            Some(text) => {
                self.breaker.on_success(&model);
                let token_estimate = (text.len() / 4).max(1) as u64;
                self.router.record_usage(&model, 0.0, token_estimate);
                Ok(text)
            }
            None => {
                self.breaker.on_failure(&model);
                Err("LLM response missing content field".to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

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
        std::env::remove_var("GOOGLE_GENERATIVE_AI_API_KEY");

        let config = make_config("cfg_key", "http://cfg:8080/v1/chat", "cfg-model");
        let client = DynamicLlmClient::new(config);

        let (api_key, endpoint, model) = client.get_current_params();

        assert_eq!(api_key, "cfg_key", "should fall back to config api_key");
        assert_eq!(endpoint, "http://cfg:8080/v1/chat", "endpoint should come from config");
        assert_eq!(model, "cfg-model", "model should come from config");
    }

    #[test]
    fn test_params_from_env_override() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("GOOGLE_GENERATIVE_AI_API_KEY", "env_key_value");

        let config = make_config("cfg_key", "http://cfg:8080/v1/chat", "cfg-model");
        let client = DynamicLlmClient::new(config);

        let (api_key, endpoint, model) = client.get_current_params();

        assert_eq!(api_key, "env_key_value", "env var should override config api_key");
        assert_eq!(endpoint, "http://cfg:8080/v1/chat", "endpoint should still come from config");
        assert_eq!(model, "cfg-model", "model should still come from config");

        std::env::remove_var("GOOGLE_GENERATIVE_AI_API_KEY");
    }

    #[test]
    fn test_params_with_empty_config_fallback() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("GOOGLE_GENERATIVE_AI_API_KEY");

        let config = make_config("", "", "");
        let client = DynamicLlmClient::new(config);

        let (api_key, endpoint, model) = client.get_current_params();

        assert_eq!(api_key, "", "should return empty string from config when no env");
        assert_eq!(endpoint, "", "should return empty endpoint from config");
        assert_eq!(model, "", "should return empty model from config");
    }

    #[test]
    fn test_openai_llm_client_new() {
        let client = OpenAiLlmClient::new(
            "key".into(),
            "http://localhost:8080".into(),
            "gpt-4".into(),
        );
        assert_eq!(client.api_key, "key");
        assert_eq!(client.endpoint, "http://localhost:8080");
        assert_eq!(client.model, "gpt-4");
    }

    #[test]
    fn test_dynamic_llm_client_new() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("GOOGLE_GENERATIVE_AI_API_KEY");
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
        std::env::remove_var("GOOGLE_GENERATIVE_AI_API_KEY");
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
        assert_eq!(api, "new_key", "env var is not set, should pick up new api_key");
        assert_eq!(ep, "http://new:9090", "endpoint should reflect write");
        assert_eq!(model, "new-model", "model should reflect write");
    }

    #[test]
    fn test_dynamic_llm_client_concurrent_reads() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("GOOGLE_GENERATIVE_AI_API_KEY");
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
        let client = OpenAiLlmClient::new("".into(), "".into(), "".into());
        assert_eq!(client.api_key, "");
        assert_eq!(client.endpoint, "");
        assert_eq!(client.model, "");
    }

    #[test]
    fn test_get_params_for_no_override_falls_back() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("GOOGLE_GENERATIVE_AI_API_KEY");
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
        std::env::remove_var("GOOGLE_GENERATIVE_AI_API_KEY");
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
        std::env::remove_var("GOOGLE_GENERATIVE_AI_API_KEY");
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
        std::env::set_var("GOOGLE_GENERATIVE_AI_API_KEY", "env_overrides_all");
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

        std::env::remove_var("GOOGLE_GENERATIVE_AI_API_KEY");
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
        std::env::remove_var("GOOGLE_GENERATIVE_AI_API_KEY");
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
        let long_ep = "https://very-long-endpoint-url.example.com/api/v1/chat/completions".to_string();
        let long_model = "very-long-model-name-v1.0.0-experimental".to_string();
        let client = OpenAiLlmClient::new(long_key.clone(), long_ep.clone(), long_model.clone());
        assert_eq!(client.api_key, long_key);
        assert_eq!(client.endpoint, long_ep);
        assert_eq!(client.model, long_model);
    }
}

