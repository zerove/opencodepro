//! Anthropic Claude provider implementation.
//!
//! Translates requests to Anthropic Messages API format.

use async_trait::async_trait;
use serde_json::{json, Value};
use sisyphus_types::{ProviderConfig, ProviderKind};
use std::sync::Arc;

use super::{ChatMessage, ChatRole, LlmProvider, ProviderError, ProviderResponse};
use crate::http_client::{RequestGuard, SharedHttpClient};

/// Provider that translates requests to Anthropic Messages API format。
pub struct AnthropicProvider {
    config: ProviderConfig,
    http: Arc<SharedHttpClient>,
}

impl AnthropicProvider {
    /// New.
    pub fn new(config: ProviderConfig, http: Arc<SharedHttpClient>) -> Result<Self, String> {
        Ok(Self { config, http })
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

        if !self.http.on_request_start() {
            return Err(ProviderError::Transient(
                "shared http client is closed".into(),
            ));
        }
        let mut _req_guard = RequestGuard::new(self.http.clone());

        let res = match self
            .http
            .http()
            .post(&self.config.endpoint)
            .header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                _req_guard.mark_errored();
                return Err(ProviderError::Transient(format!(
                    "HTTP request failed: {e}"
                )));
            },
        };

        let status = res.status();
        let response_json: Value = match res.json().await {
            Ok(v) => v,
            Err(e) => {
                _req_guard.mark_errored();
                return Err(ProviderError::Transient(format!(
                    "Failed to parse response: {e}"
                )));
            },
        };

        if !status.is_success() {
            let error_msg = response_json["error"]["message"].as_str().unwrap_or("unknown error");
            let code = status.as_u16();
            if code == 429 {
                _req_guard.mark_errored();
                return Err(ProviderError::RateLimited(error_msg.to_string()));
            }
            if code == 401 {
                _req_guard.mark_errored();
                return Err(ProviderError::Auth(error_msg.to_string()));
            }
            if code >= 500 {
                _req_guard.mark_errored();
                return Err(ProviderError::Transient(format!(
                    "API error ({code}): {error_msg}"
                )));
            }
            return Err(ProviderError::Permanent(format!(
                "API error ({code}): {error_msg}"
            )));
        }

        // Anthropic response format: { content: [{ type: "text", text: "..." }] }
        let content = response_json["content"]
            .as_array()
            .and_then(|arr| {
                arr.iter().find(|c| c["type"] == "text").and_then(|c| c["text"].as_str())
            })
            .map(std::string::ToString::to_string)
            .ok_or_else(|| ProviderError::Permanent("Response missing text content".to_string()))?;

        let model = response_json["model"].as_str().unwrap_or(&self.config.model).to_string();

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
