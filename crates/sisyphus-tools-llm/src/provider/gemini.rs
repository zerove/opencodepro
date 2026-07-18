//! Google Gemini provider implementation.
//!
//! Uses the OpenAI-compatible endpoint at
//! `https://generativelanguage.googleapis.com/v1beta/openai/chat/completions`
//! which Gemini supports natively. Falls back to the native API if needed.

use async_trait::async_trait;
use serde_json::{json, Value};
use sisyphus_types::{ProviderConfig, ProviderKind};
use std::time::Duration;

use super::{ChatMessage, LlmProvider, ProviderError, ProviderResponse};

/// Provider that translates requests to Google Generative Language API format.
pub struct GoogleGeminiProvider {
    config: ProviderConfig,
    client: reqwest::Client,
}

impl GoogleGeminiProvider {
    pub fn new(config: ProviderConfig) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_mins(2))
            .build()
            .map_err(|e| format!("Failed to build HTTP client: {e}"))?;
        Ok(Self { config, client })
    }
}

#[async_trait]
impl LlmProvider for GoogleGeminiProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Google
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

        // Try OpenAI-compatible endpoint first (Gemini supports it natively)
        let endpoint = self.config.endpoint.clone();
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
            .post(&endpoint)
            .header("x-goog-api-key", &api_key)
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
                .or_else(|| response_json.get("error").and_then(|e| e.as_str()))
                .unwrap_or("unknown error");
            if status.as_u16() == 429 {
                return Err(ProviderError::RateLimited(error_msg.to_string()));
            }
            return Err(ProviderError::Transient(format!(
                "API error ({}): {}",
                status.as_u16(),
                error_msg
            )));
        }

        // Parse both OpenAI-style and Gemini-native response formats
        let content = response_json["choices"][0]["message"]["content"]
            .as_str()
            .or_else(|| {
                response_json["candidates"][0]["content"]["parts"]
                    .as_array()
                    .and_then(|parts| parts.first())
                    .and_then(|p| p["text"].as_str())
            })
            .map(std::string::ToString::to_string)
            .ok_or_else(|| {
                ProviderError::Permanent("Response missing content field".to_string())
            })?;

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
