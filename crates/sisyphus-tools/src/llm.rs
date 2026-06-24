use async_trait::async_trait;
use serde_json::{json, Value};
use crate::metis::LlmClient;
use std::sync::{Arc, RwLock};
use sisyphus_types::Config;

/// A production-ready LLM client compatible with OpenAI-style APIs (e.g., vLLM, Ollama).
pub struct OpenAiLlmClient {
    pub api_key: String,
    pub endpoint: String,
    pub model: String,
}

impl OpenAiLlmClient {
    pub fn new(api_key: String, endpoint: String, model: String) -> Self {
        Self {
            api_key,
            endpoint,
            model,
        }
    }
}

#[async_trait]
impl LlmClient for OpenAiLlmClient {
    async fn chat(&self, system: &str, user: &str, json_mode: bool) -> Result<String, String> {
        let client = reqwest::Client::new();
        
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
pub struct DynamicLlmClient {
    config: Arc<RwLock<Config>>,
}

impl DynamicLlmClient {
    pub fn new(config: Arc<RwLock<Config>>) -> Self {
        Self { config }
    }

    fn get_current_params(&self) -> (String, String, String) {
        let cfg = self.config.read().unwrap();
        
        let api_key = std::env::var("GOOGLE_GENERATIVE_AI_API_KEY")
            .unwrap_or_else(|_| cfg.llm.api_key.clone());
            
        (api_key, cfg.llm.endpoint.clone(), cfg.llm.model.clone())
    }
}

#[async_trait]
impl LlmClient for DynamicLlmClient {
    async fn chat(&self, system: &str, user: &str, json_mode: bool) -> Result<String, String> {
        let (api_key, endpoint, model) = self.get_current_params();
        
        // Delegate to OpenAiLlmClient for the actual HTTP call
        let client = OpenAiLlmClient::new(api_key, endpoint, model);
        client.chat(system, user, json_mode).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(api_key: &str, endpoint: &str, model: &str) -> Arc<RwLock<Config>> {
        Arc::new(RwLock::new(Config {
            llm: sisyphus_types::LlmConfig {
                api_key: api_key.to_string(),
                endpoint: endpoint.to_string(),
                model: model.to_string(),
            },
            ..Config::default()
        }))
    }

    #[test]
    fn test_params_from_config_fallback() {
        // Ensure the env var is NOT set for this test
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
        std::env::remove_var("GOOGLE_GENERATIVE_AI_API_KEY");

        let config = make_config("", "", "");
        let client = DynamicLlmClient::new(config);

        let (api_key, endpoint, model) = client.get_current_params();

        assert_eq!(api_key, "", "should return empty string from config when no env");
        assert_eq!(endpoint, "", "should return empty endpoint from config");
        assert_eq!(model, "", "should return empty model from config");
    }
}

