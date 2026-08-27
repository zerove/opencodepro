//! Provider registry with fallback support.

use sisyphus_types::{ProviderConfig, ProviderKind};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use super::{
    AnthropicProvider, ChatMessage, GoogleGeminiProvider, LlmProvider, OpenAiCompatibleProvider,
    ProviderError, ProviderResponse, RetryConfig,
};

/// Manages multiple LLM providers and supports fallback chains.
pub struct ProviderRegistry {
    /// All registered providers, keyed by name.
    providers: HashMap<String, Arc<dyn LlmProvider>>,
    /// Ordered list of provider names for fallback (by priority).
    fallback_order: Vec<String>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
            fallback_order: Vec::new(),
        }
    }

    /// Register a provider. If a provider with the same name exists, it is replaced.
    pub fn register(&mut self, provider: Arc<dyn LlmProvider>) {
        let name = provider.name().to_string();
        self.providers.insert(name.clone(), provider);

        // Rebuild fallback order if not already present
        if !self.fallback_order.contains(&name) {
            self.fallback_order.push(name);
        }
    }

    /// Get a provider by name.
    pub fn get(&self, name: &str) -> Option<Arc<dyn LlmProvider>> {
        self.providers.get(name).cloned()
    }

    /// Get all registered provider names.
    pub fn list(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }

    /// Remove a provider by name.
    pub fn remove(&mut self, name: &str) -> bool {
        self.fallback_order.retain(|n| n != name);
        self.providers.remove(name).is_some()
    }

    /// Register a fallback chain: providers are tried in priority order.
    ///
    /// This replaces any existing fallback order.
    pub fn set_fallback_order(&mut self, order: Vec<String>) {
        self.fallback_order = order;
    }

    /// Get the current fallback order.
    pub fn fallback_order(&self) -> &[String] {
        &self.fallback_order
    }

    /// Call the first available provider with automatic fallback.
    ///
    /// On transient error or rate-limit, the next provider in the chain is tried.
    /// Permanent errors and auth errors are returned immediately.
    pub async fn chat_with_fallback(
        &self,
        system: &str,
        messages: &[ChatMessage],
        json_mode: bool,
    ) -> Result<ProviderResponse, ProviderError> {
        let mut last_error = None;
        let retry_cfg = RetryConfig::default();

        for name in &self.fallback_order {
            if let Some(provider) = self.providers.get(name) {
                let mut attempt = 0;
                loop {
                    match provider.chat(system, messages, json_mode).await {
                        Ok(response) => return Ok(response),
                        Err(e @ ProviderError::Permanent(_)) => return Err(e),
                        Err(e @ ProviderError::Auth(_)) => return Err(e),
                        Err(e) => {
                            if attempt >= retry_cfg.max_retries {
                                last_error = Some(ProviderError::Transient(format!(
                                    "{}: {e}",
                                    provider.name()
                                )));
                                break;
                            }

                            let base_delay = retry_cfg.base_delay_ms * 2u64.pow(attempt);
                            let jitter_range = (base_delay as f64 * 0.25) as i64;
                            let offset = if jitter_range > 0 {
                                let nanos = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_nanos();

                                (nanos % (2 * jitter_range as u128)) as i64 - jitter_range
                            } else {
                                0
                            };
                            let delay =
                                Duration::from_millis((base_delay as i64 + offset).max(0) as u64);

                            tracing::warn!(
                                target: "provider",
                                "Provider '{}' failed with retryable error: {e}. Retrying in {:?} (attempt {}/{})",
                                provider.name(),
                                delay,
                                attempt + 1,
                                retry_cfg.max_retries
                            );

                            tokio::time::sleep(delay).await;
                            attempt += 1;
                        }
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            ProviderError::Permanent("No providers available in fallback chain".to_string())
        }))
    }

    /// Call a specific provider by name.
    pub async fn chat_with_provider(
        &self,
        name: &str,
        system: &str,
        messages: &[ChatMessage],
        json_mode: bool,
    ) -> Result<ProviderResponse, ProviderError> {
        match self.providers.get(name) {
            Some(provider) => provider.chat(system, messages, json_mode).await,
            None => Err(ProviderError::Permanent(format!(
                "Provider '{name}' not found"
            ))),
        }
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Build the default provider registry from config.
pub fn build_provider_registry(providers_cfg: &[ProviderConfig]) -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();
    let mut fallback_order: Vec<String> = Vec::new();

    let mut sorted = providers_cfg.to_vec();
    sorted.sort_by_key(|p| p.priority);

    for cfg in sorted {
        if !cfg.enabled {
            continue;
        }

        let provider: Result<Arc<dyn LlmProvider>, String> = match cfg.kind {
            // OpenAI 兼容协议：OpenAI/Groq/Mistral/Together 共享 OpenAiCompatibleProvider
            ProviderKind::OpenAi
            | ProviderKind::Groq
            | ProviderKind::Mistral
            | ProviderKind::Together => OpenAiCompatibleProvider::new(cfg.clone())
                .map(|p| Arc::new(p) as Arc<dyn LlmProvider>),
            ProviderKind::Google => GoogleGeminiProvider::new(cfg.clone())
                .map(|p| Arc::new(p) as Arc<dyn LlmProvider>),
            ProviderKind::Anthropic => AnthropicProvider::new(cfg.clone())
                .map(|p| Arc::new(p) as Arc<dyn LlmProvider>),
            // Cohere 暂未实现，路由报错即可（上层可借此回退）
            ProviderKind::Cohere => Err(format!(
                "Cohere provider not yet implemented in this build"
            )),
        };

        match provider {
            Ok(p) => {
                let name = p.name().to_string();
                registry.register(p);
                fallback_order.push(name);
            }
            Err(e) => {
                tracing::error!(target: "provider", "Failed to build '{}': {e}", cfg.name);
            }
        }
    }

    registry.set_fallback_order(fallback_order);
    registry
}

#[cfg(test)]
mod tests {
    use super::*;
    use sisyphus_types::{ProviderConfig, ProviderKind};

    fn make_openai_compatible_cfg(kind: ProviderKind, name: &str, model: &str) -> ProviderConfig {
        ProviderConfig {
            name: name.to_string(),
            kind,
            endpoint: kind.default_endpoint().unwrap().to_string(),
            api_key: "test-key".to_string(),
            model: model.to_string(),
            base_url: None,
            priority: 10,
            enabled: true,
            max_tokens: Some(1024),
            temperature: Some(0.1),
        }
    }

    #[test]
    fn registry_registers_groq_via_openai_compatible() {
        let cfg = make_openai_compatible_cfg(ProviderKind::Groq, "groq-main", "llama-3.1-70b-versatile");
        let r = build_provider_registry(&[cfg]);
        assert!(r.get("groq-main").is_some());
        assert_eq!(r.list().len(), 1);
    }

    #[test]
    fn registry_registers_mistral_via_openai_compatible() {
        let cfg = make_openai_compatible_cfg(ProviderKind::Mistral, "mistral-main", "mistral-large-latest");
        let r = build_provider_registry(&[cfg]);
        assert!(r.get("mistral-main").is_some());
    }

    #[test]
    fn registry_registers_together_via_openai_compatible() {
        let cfg = make_openai_compatible_cfg(ProviderKind::Together, "together-main", "meta-llama/Llama-3.3-70B-Instruct-Turbo");
        let r = build_provider_registry(&[cfg]);
        assert!(r.get("together-main").is_some());
    }

    #[test]
    fn registry_registers_openai_unchanged() {
        let cfg = make_openai_compatible_cfg(ProviderKind::OpenAi, "openai-main", "gpt-4o-mini");
        let r = build_provider_registry(&[cfg]);
        assert!(r.get("openai-main").is_some());
    }

    #[test]
    fn registry_fallback_chain_orders_by_priority() {
        let mut high = make_openai_compatible_cfg(ProviderKind::OpenAi, "openai-priority-5", "gpt-4o-mini");
        high.priority = 5;
        let mut low = make_openai_compatible_cfg(ProviderKind::Groq, "groq-priority-20", "llama-3.1-70b-versatile");
        low.priority = 20;
        let r = build_provider_registry(&[low, high]); // 故意乱序
        let order = r.fallback_order();
        assert_eq!(order, &["openai-priority-5".to_string(), "groq-priority-20".to_string()]);
    }

    #[test]
    fn registry_skips_disabled_providers() {
        let mut cfg = make_openai_compatible_cfg(ProviderKind::Groq, "groq-disabled", "llama-3.1-70b-versatile");
        cfg.enabled = false;
        let r = build_provider_registry(&[cfg]);
        assert_eq!(r.list().len(), 0);
    }

    #[test]
    fn registry_cohere_returns_error() {
        // Cohere 暂未实现 — 路由报错
        // 不通过 make_openai_compatible_cfg（它会 unwrap default_endpoint()）
        let cfg = ProviderConfig {
            name: "cohere-main".to_string(),
            kind: ProviderKind::Cohere,
            endpoint: "https://api.cohere.ai/v1/chat".to_string(),
            api_key: "test-key".to_string(),
            model: "command-r-plus".to_string(),
            base_url: None,
            priority: 10,
            enabled: true,
            max_tokens: None,
            temperature: None,
        };
        let r = build_provider_registry(&[cfg]);
        // 构造失败 → registry 中不包含
        assert!(r.get("cohere-main").is_none());
    }

    #[test]
    fn registry_combines_openai_groq_anthropic() {
        let cfgs = vec![
            make_openai_compatible_cfg(ProviderKind::OpenAi, "openai-x", "gpt-4o-mini"),
            make_openai_compatible_cfg(ProviderKind::Groq, "groq-x", "llama-3.1-70b-versatile"),
            make_openai_compatible_cfg(ProviderKind::Mistral, "mistral-x", "mistral-large-latest"),
            make_openai_compatible_cfg(ProviderKind::Together, "together-x", "meta-llama/Llama-3.3-70B-Instruct-Turbo"),
        ];
        let r = build_provider_registry(&cfgs);
        assert_eq!(r.list().len(), 4);
        for name in ["openai-x", "groq-x", "mistral-x", "together-x"] {
            assert!(r.get(name).is_some(), "{name} not registered");
        }
    }
}
