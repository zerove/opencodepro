//! OpenAI-compatible provider implementation.
//!
//! Compatible with: OpenAI, vLLM, Ollama, Groq, Together AI, Fireworks, etc.

use async_trait::async_trait;
use metrics::counter;
use serde_json::{json, Value};
use sisyphus_types::{ProviderConfig, ProviderKind};
use std::sync::Arc;

use super::{ChatMessage, LlmProvider, ProviderError, ProviderResponse};
use crate::http_client::SharedHttpClient;

/// OpenRouter 归因 header：根据配置或默认值返回 referer。
/// `None` 表示不发送该 header。
fn openrouter_referer(cfg: &ProviderConfig) -> Option<String> {
    if cfg.kind != ProviderKind::OpenRouter {
        return None;
    }
    // 优先用 config.base_url 作为 referer（若配置者设置了一个站点），
    // 否则使用 OpenRouter 推荐的默认占位。
    Some(cfg.base_url.clone().unwrap_or_else(|| "https://sisyphus.dev".to_string()))
}

/// OpenRouter 归因 header：站点/应用名。
fn openrouter_title(cfg: &ProviderConfig) -> Option<String> {
    if cfg.kind != ProviderKind::OpenRouter {
        return None;
    }
    Some("Sisyphus Cognitive Engine".to_string())
}

/// RAII guard：drop 时自动调用 `on_request_end(errored=false)`。
/// 若需标记为错误，调用 `mark_errored()` 后 drop。
struct RequestGuard {
    http: Arc<SharedHttpClient>,
    errored: bool,
}

impl RequestGuard {
    fn new(http: Arc<SharedHttpClient>) -> Self {
        Self { http, errored: false }
    }
    fn mark_errored(&mut self) {
        self.errored = true;
    }
}

impl Drop for RequestGuard {
    fn drop(&mut self) {
        self.http.on_request_end(self.errored);
    }
}

/// Provider that speaks the OpenAI chat completions protocol.
///
/// 接收共享 [`SharedHttpClient`]——所有 provider 共享同一连接池。
pub struct OpenAiCompatibleProvider {
    config: ProviderConfig,
    http: Arc<SharedHttpClient>,
}

impl OpenAiCompatibleProvider {
    /// New.
    pub fn new(config: ProviderConfig, http: Arc<SharedHttpClient>) -> Result<Self, String> {
        Ok(Self { config, http })
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

        // 共享 client 关闭后快速失败，避免对已关闭系统继续打洞。
        if !self.http.on_request_start() {
            return Err(ProviderError::Transient(
                "shared http client is closed".into(),
            ));
        }
        // RAII 风格的 request 结束追踪：成功/失败/early-return 都会触发。
        let mut _req_guard = RequestGuard::new(self.http.clone());

        let mut builder = self
            .http
            .http()
            .post(&self.config.endpoint)
            .header("Authorization", format!("Bearer {api_key}"));
        // OpenRouter 推荐的归因 headers（用于在 openrouter.ai/rankings 展示）。
        if let Some(referer) = openrouter_referer(&self.config) {
            builder = builder.header("HTTP-Referer", referer);
        }
        if let Some(title) = openrouter_title(&self.config) {
            builder = builder.header("X-Title", title);
        }
        let res = match builder
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
            }
        };

        let status = res.status();
        let response_json: Value = res
            .json()
            .await
            .map_err(|e| ProviderError::Transient(format!("Failed to parse response: {e}")))?;

        if !status.is_success() {
            let error_msg = response_json["error"]["message"]
                .as_str()
                .unwrap_or("unknown error");
            let code = status.as_u16();
            // 5xx 视为可重试 transient；4xx 通常 permanent（除 429）
            if code == 429 {
                _req_guard.mark_errored();
                return Err(ProviderError::RateLimited(error_msg.to_string()));
            }
            if code == 401 || code == 403 {
                _req_guard.mark_errored();
                return Err(ProviderError::Auth(error_msg.to_string()));
            }
            if code >= 500 {
                _req_guard.mark_errored();
                return Err(ProviderError::Transient(format!(
                    "API error ({code}): {error_msg}"
                )));
            }
            // 其它 4xx：permanent（4xx 是客户端错误，重试无意义）
            return Err(ProviderError::Permanent(format!(
                "API error ({code}): {error_msg}"
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

        let input_tokens_val = input_tokens.unwrap_or(0);
        let output_tokens_val = output_tokens.unwrap_or(0);
        counter!(
            "sisyphus_llm_requests_total",
            "provider" => "openai",
            "model" => model.clone(),
        ).increment(1);
        counter!(
            "sisyphus_llm_tokens_total",
            "provider" => "openai",
            "model" => model.clone(),
            "kind" => "input",
        ).increment(input_tokens_val);
        counter!(
            "sisyphus_llm_tokens_total",
            "provider" => "openai",
            "model" => model.clone(),
            "kind" => "output",
        ).increment(output_tokens_val);

        Ok(ProviderResponse {
            content,
            model,
            input_tokens,
            output_tokens,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cfg(kind: ProviderKind) -> ProviderConfig {
        ProviderConfig {
            name: "test".into(),
            kind,
            endpoint: "https://example.com/v1/chat/completions".into(),
            api_key: "sk-test".into(),
            model: "test-model".into(),
            base_url: None,
            priority: 10,
            enabled: true,
            max_tokens: None,
            temperature: None,
        }
    }

    #[test]
    fn openrouter_headers_present() {
        let cfg = make_cfg(ProviderKind::OpenRouter);
        assert!(openrouter_referer(&cfg).is_some());
        assert!(openrouter_title(&cfg).is_some());
        let referer = openrouter_referer(&cfg).unwrap();
        let title = openrouter_title(&cfg).unwrap();
        assert!(!referer.is_empty());
        assert!(!title.is_empty());
    }

    #[test]
    fn non_openrouter_headers_absent() {
        for kind in [
            ProviderKind::OpenAi,
            ProviderKind::Groq,
            ProviderKind::Mistral,
            ProviderKind::Together,
            ProviderKind::Anthropic,
            ProviderKind::Google,
        ] {
            let cfg = make_cfg(kind);
            assert!(openrouter_referer(&cfg).is_none(), "{kind:?} should NOT set referer");
            assert!(openrouter_title(&cfg).is_none(), "{kind:?} should NOT set title");
        }
    }

    #[test]
    fn openrouter_referer_uses_base_url_when_set() {
        let mut cfg = make_cfg(ProviderKind::OpenRouter);
        cfg.base_url = Some("https://my-app.example.com".into());
        let referer = openrouter_referer(&cfg).unwrap();
        assert_eq!(referer, "https://my-app.example.com");
    }
}
