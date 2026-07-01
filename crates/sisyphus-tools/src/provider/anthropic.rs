//! Anthropic Claude provider implementation.
//!
//! Translates requests to Anthropic Messages API format.

use async_trait::async_trait;
use serde_json::{json, Value};
use sisyphus_types::{ProviderConfig, ProviderKind};
use std::time::Duration;

use super::{ChatMessage, ChatRole, LlmProvider, ProviderError, ProviderResponse};

/// Provider that translates requests to Anthropic Messages API format.
pub struct AnthropicProvider {
    config: ProviderConfig,
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(config: ProviderConfig) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_mins(3))
            .build()
            .map_err(|e| format!("Failed to build HTTP client: {e}"))?;
        Ok(Self { config, client })
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Anthropic
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
        _json_mode: bool,
    ) -> Result<ProviderResponse, ProviderError> {
        let api_key = self.config.resolve_api_key();

        // Anthropic uses a different message format
        let mut anthropic_messages: Vec<Value> = Vec::new();
        for msg in messages {
            anthropic_messages.push(json!({
                "role": if msg.role == ChatRole::Assistant { "assistant" } else { "user" },
                "content": msg.content
            }));
        }

        let mut body = json!({
            "model": self.config.model,
            "messages": anthropic_messages,
            "max_tokens": self.config.max_tokens.unwrap_or(4096),
            "temperature": self.config.temperature.unwrap_or(0.1),
        });

        // Anthropic puts system prompt at top level, not in messages array
        if !system.is_empty() {
            body["system"] = json!(system);
        }

        let res = self
            .client
            .post(&self.config.endpoint)
            .header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01")
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
            if status.as_u16() == 401 {
                return Err(ProviderError::Auth(error_msg.to_string()));
            }
            return Err(ProviderError::Transient(format!(
                "API error ({}): {}",
                status.as_u16(),
                error_msg
            )));
        }

        // Anthropic response format: { content: [{ type: "text", text: "..." }] }
        let content = response_json["content"]
            .as_array()
            .and_then(|arr| {
                arr.iter()
                    .find(|c| c["type"] == "text")
                    .and_then(|c| c["text"].as_str())
            })
            .map(std::string::ToString::to_string)
            .ok_or_else(|| {
                ProviderError::Permanent("Response missing text content".to_string())
            })?;

        let model = response_json["model"]
            .as_str()
            .unwrap_or(&self.config.model)
            .to_string();

        let input_tokens = response_json["usage"]["input_tokens"].as_u64();
        let output_tokens = response_json["usage"]["output_tokens"].as_u64();

        Ok(ProviderResponse {
            content,
            model,
            input_tokens,
            output_tokens,
        })
    }
}
