//! OpenAI-compatible provider implementation.
//!
//! Compatible with: OpenAI, vLLM, Ollama, Groq, Together AI, Fireworks, etc.

use async_trait::async_trait;
use serde_json::{json, Value};
use sisyphus_types::{ProviderConfig, ProviderKind};
use std::time::Duration;

use super::{ChatMessage, LlmProvider, ProviderError, ProviderResponse};

/// Provider that speaks the OpenAI chat completions protocol.
pub struct OpenAiCompatibleProvider {
    config: ProviderConfig,
    client: reqwest::Client,
}

impl OpenAiCompatibleProvider {
    pub fn new(config: ProviderConfig) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_mins(2))
            .build()
            .map_err(|e| format!("Failed to build HTTP client: {e}"))?;
        Ok(Self { config, client })
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatibleProvider {
    fn kind(&self) -> ProviderKind {
        self.config.kind
    }

    fn name(&self) -> &str {
        &self.config.name
    }

    fn model(&self) -> &str {
        &self.config.model
    }

    async fn chat(
        &self,
        system: &str,
        messages: &[ChatMessage],
        json_mode: bool,
    ) -> Result<ProviderResponse, ProviderError> {
        let api_key = self.config.resolve_api_key();

        let mut formatted_messages: Vec<Value> = Vec::new();

        if !system.is_empty() {
            formatted_messages.push(json!({
                "role": "system",
                "content": system
            }));
        }

        for msg in messages {
            formatted_messages.push(json!({
                "role": msg.role.as_str(),
                "content": msg.content
            }));
        }

        let mut body = json!({
            "model": self.config.model,
            "messages": formatted_messages,
            "temperature": self.config.temperature.unwrap_or(0.1),
        });

        if let Some(maxt) = self.config.max_tokens {
            body["max_tokens"] = json!(maxt);
        }

        if json_mode {
            body["response_format"] = json!({ "type": "json_object" });
        }

        let res = self
            .client
            .post(&self.config.endpoint)
            .header("Authorization", format!("Bearer {api_key}"))
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Transient(format!("HTTP request failed: {e}")))?;

        let status = res.status();
        let response_json: Value = res
            .json()
            .await
            .map_err(|e| ProviderError::Transient(format!("Failed to parse response: {e}")))?;

        if !status.is_success() {
            let error_msg = response_json["error"]["message"]
                .as_str()
                .unwrap_or("unknown error");
            if status.as_u16() == 429 {
                return Err(ProviderError::RateLimited(error_msg.to_string()));
            }
            if status.as_u16() == 401 || status.as_u16() == 403 {
                return Err(ProviderError::Auth(error_msg.to_string()));
            }
            return Err(ProviderError::Transient(format!(
                "API error ({}): {}",
                status.as_u16(),
                error_msg
            )));
        }

        let content = response_json["choices"][0]["message"]["content"]
            .as_str()
            .map(std::string::ToString::to_string)
            .ok_or_else(|| ProviderError::Permanent("Response missing content".to_string()))?;

        let model = response_json["model"]
            .as_str()
            .unwrap_or(&self.config.model)
            .to_string();

        let input_tokens = response_json["usage"]["prompt_tokens"].as_u64();
        let output_tokens = response_json["usage"]["completion_tokens"].as_u64();

        Ok(ProviderResponse {
            content,
            model,
            input_tokens,
            output_tokens,
        })
    }
}
