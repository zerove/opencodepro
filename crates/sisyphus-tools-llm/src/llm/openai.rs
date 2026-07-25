//! A production-ready OpenAI-compatible LLM client.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::time::Duration;

use crate::metis::LlmClient;

/// A production-ready LLM client compatible with OpenAI-style APIs (e.g., vLLM, Ollama).
pub struct OpenAiLlmClient {
    pub api_key: String,
    pub endpoint: String,
    pub model: String,
    client: reqwest::Client,
}

impl OpenAiLlmClient {
    pub fn new(
        api_key: String,
        endpoint: String,
        model: String,
    ) -> Result<Self, sisyphus_types::TypesError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_mins(1))
            .build()
            .map_err(|e| {
                sisyphus_types::TypesError::InternalError(format!(
                    "Failed to build reqwest::Client: {e}"
                ))
            })?;
        Ok(Self {
            api_key,
            endpoint,
            model,
            client,
        })
    }
}

#[async_trait]
impl LlmClient for OpenAiLlmClient {
    async fn generate(&self, prompt: &str) -> Result<String, sisyphus_types::TypesError> {
        self.chat("", prompt, false).await
    }

    async fn chat(
        &self,
        system: &str,
        user: &str,
        json_mode: bool,
    ) -> Result<String, sisyphus_types::TypesError> {
        let client = &self.client;

        let messages = vec![
            json!({ "role": "system", "content": system }),
            json!({ "role": "user", "content": user }),
        ];

        let mut body = json!({
            "model": self.model,
            "messages": messages,
            "temperature": 0.1,
        });

        if json_mode {
            body["response_format"] = json!({ "type": "json_object" });
        }

        let res = client
            .post(&self.endpoint)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {e}"))?;

        let status = res.status();
        let response_json: Value = res
            .json()
            .await
            .map_err(|e| format!("Failed to parse response JSON: {e}"))?;

        if !status.is_success() {
            let error_obj = if response_json.is_array() {
                &response_json[0]["error"]
            } else {
                &response_json["error"]
            };
            let error_msg = error_obj["message"].as_str().unwrap_or("unknown error");
            return Err(sisyphus_types::TypesError::InternalError(format!(
                "LLM API error ({}): {}",
                status.as_u16(),
                error_msg
            )));
        }

        response_json["choices"][0]["message"]["content"]
            .as_str()
            .map(std::string::ToString::to_string)
            .ok_or_else(|| {
                sisyphus_types::TypesError::InternalError(
                    "LLM response missing content field".into(),
                )
            })
    }

    fn get_params_for(&self, _agent_id: &str) -> (String, String, String) {
        (
            self.api_key.clone(),
            self.endpoint.clone(),
            self.model.clone(),
        )
    }

    fn breaker(&self) -> crate::circuit_breaker::CircuitBreaker {
        crate::circuit_breaker::CircuitBreaker::default()
    }
}

pub struct OpenAiEmbeddingClient {
    pub api_key: String,
    pub endpoint: String,
    pub model: String,
    client: reqwest::Client,
}

impl OpenAiEmbeddingClient {
    pub fn new(
        api_key: String,
        endpoint: String,
        model: String,
    ) -> Result<Self, sisyphus_types::TypesError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| {
                sisyphus_types::TypesError::InternalError(format!(
                    "Failed to build reqwest::Client: {e}"
                ))
            })?;
        Ok(Self {
            api_key,
            endpoint,
            model,
            client,
        })
    }
}

#[async_trait]
impl crate::llm::EmbeddingClient for OpenAiEmbeddingClient {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, sisyphus_types::TypesError> {
        let body = json!({
            "input": text,
            "model": self.model,
        });

        let res = self
            .client
            .post(format!(
                "{}/embeddings",
                self.endpoint.trim_end_matches('/')
            ))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {e}"))?;

        let status = res.status();
        let response_json: Value = res
            .json()
            .await
            .map_err(|e| format!("Failed to parse response JSON: {e}"))?;

        if !status.is_success() {
            let error_msg = response_json["error"]["message"]
                .as_str()
                .unwrap_or("unknown error");
            return Err(sisyphus_types::TypesError::InternalError(format!(
                "Embedding API error ({}): {}",
                status.as_u16(),
                error_msg
            )));
        }

        let embedding = response_json["data"][0]["embedding"]
            .as_array()
            .ok_or_else(|| {
                sisyphus_types::TypesError::InternalError(
                    "Embedding response missing data[0].embedding array".into(),
                )
            })?
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect();

        Ok(embedding)
    }
}
