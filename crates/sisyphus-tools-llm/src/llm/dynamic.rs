//! Dynamic LLM client that retrieves configuration from a shared config store.
//!
//! Supports per-agent model overrides, circuit breaker, model router, and budget tracking.

use async_trait::async_trait;
use serde_json::{json, Value};
use sisyphus_types::Config;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use crate::circuit_breaker::CircuitBreaker;
use crate::metis::LlmClient;
use crate::model_router::{ModelConfig, ModelRouter};
use crate::provider::{build_provider_registry, ChatMessage, ChatRole, ProviderRegistry};

use super::{BudgetProvider, RequestContext};

/// A proxy client that retrieves LLM configuration dynamically from a shared config store.
///
/// Supports per-agent model overrides via `Config.llm.agents`.
///
/// Integrates a [`CircuitBreaker`] to prevent cascading failures and a [`ModelRouter`]
/// for cost tracking (model selection can be wired on top of the existing config-based resolution).
pub struct DynamicLlmClient {
    config: Arc<RwLock<Config>>,
    client: reqwest::Client,
    breaker: CircuitBreaker,
    router: Arc<ModelRouter>,
    budget_provider: Option<Arc<dyn BudgetProvider>>,
    pub(super) current_context: Arc<RwLock<Option<RequestContext>>>,
    current_category: Arc<RwLock<Option<String>>>,
    provider_registry: Option<ProviderRegistry>,
}

impl DynamicLlmClient {
    pub fn new(config: Arc<RwLock<Config>>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_mins(1))
            .build()
            .expect("Failed to build reqwest::Client");
        let provider_registry = config
            .read()
            .ok()
            .map(|cfg| build_provider_registry(&cfg.llm.providers));
        Self {
            config,
            client,
            breaker: CircuitBreaker::default(),
            router: Arc::new(ModelRouter::new(0.0)),
            budget_provider: None,
            current_context: Arc::new(RwLock::new(None)),
            current_category: Arc::new(RwLock::new(None)),
            provider_registry,
        }
    }

    pub fn with_provider_registry(mut self, registry: ProviderRegistry) -> Self {
        self.provider_registry = Some(registry);
        self
    }

    pub fn with_budget_provider(mut self, provider: Arc<dyn BudgetProvider>) -> Self {
        self.budget_provider = Some(provider);
        self
    }

    pub fn set_context(&self, project_id: &str, goal_id: &str) {
        let mut ctx = self
            .current_context
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *ctx = Some(RequestContext {
            project_id: project_id.to_string(),
            goal_id: goal_id.to_string(),
        });
    }

    pub fn clear_context(&self) {
        let mut ctx = self
            .current_context
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *ctx = None;
    }

    pub fn set_category(&self, category: &str) {
        let mut cat = self
            .current_category
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *cat = Some(category.to_string());
    }

    pub fn clear_category(&self) {
        let mut cat = self
            .current_category
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *cat = None;
    }

    pub fn with_breaker(mut self, breaker: CircuitBreaker) -> Self {
        self.breaker = breaker;
        self
    }

    pub fn with_router(mut self, router: Arc<ModelRouter>) -> Self {
        self.router = router;
        self
    }

    pub fn breaker(&self) -> &CircuitBreaker {
        &self.breaker
    }

    pub fn router(&self) -> &Arc<ModelRouter> {
        &self.router
    }

    fn resolve_api_key(cfg_api_key: &str) -> String {
        std::env::var("GOOGLE_GENERATIVE_AI_API_KEY").unwrap_or_else(|_| cfg_api_key.to_string())
    }

    /// Get base (default) LLM parameters.
    pub(super) fn get_current_params(&self) -> (String, String, String) {
        let cfg = self
            .config
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let api_key = Self::resolve_api_key(&cfg.llm.api_key);
        (api_key, cfg.llm.endpoint.clone(), cfg.llm.model.clone())
    }

    /// Get LLM parameters for a specific agent.
    ///
    /// If the agent has a model override in `Config.llm.agents`, its
    /// endpoint / model / api_key take precedence.  Fallback fields
    /// come from the base config.
    /// Get LLM parameters for a specific category.
    ///
    /// If the category has an override in `Config.llm.categories`, its
    /// endpoint / model / api_key take precedence. Fallback fields
    /// come from the base config.
    pub fn get_params_for_category(&self, category: &str) -> (String, String, String) {
        let cfg = self
            .config
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        if let Some(override_) = cfg.llm.categories.get(category) {
            let endpoint = override_
                .endpoint
                .as_deref()
                .unwrap_or(&cfg.llm.endpoint)
                .to_string();
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
    async fn generate(&self, prompt: &str) -> Result<String, String> {
        self.chat("", prompt, false).await
    }

    async fn chat(&self, system: &str, user: &str, json_mode: bool) -> Result<String, String> {
        let start_time = std::time::Instant::now();

        // ── Try provider registry fallback chain first ─────────────
        if let Some(ref registry) = self.provider_registry {
            if !registry.fallback_order().is_empty() {
                let chat_messages = vec![ChatMessage {
                    role: ChatRole::User,
                    content: user.to_string(),
                }];
                match registry
                    .chat_with_fallback(system, &chat_messages, json_mode)
                    .await
                {
                    Ok(response) => {
                        // Record cost via router
                        let token_estimate = (response.content.len() / 4).max(1) as u64;
                        self.router
                            .record_usage(&response.model, 0.0, token_estimate);
                        return Ok(response.content);
                    }
                    Err(e) => {
                        // All providers failed — log warning and fall through to default endpoint
                        tracing::warn!(target: "llm", "All providers failed, falling back to default endpoint: {e}");
                    }
                }
            }
        }

        // ── Fallback: existing OpenAI-compatible HTTP call ─────────
        let (api_key, endpoint, model) = {
            let category = self
                .current_category
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            if let Some(cat_name) = category {
                let cfg = self
                    .config
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(cat_cfg) = cfg.llm.categories.get(&cat_name) {
                    if let Some(strategy) = &cat_cfg.strategy {
                        if let Ok(selected) = self.router.select(user, strategy) {
                            let (resolved_api, _, _) = self.get_params_for_category(&cat_name);
                            (resolved_api, selected.endpoint, selected.name)
                        } else {
                            self.get_params_for_category(&cat_name)
                        }
                    } else {
                        self.get_params_for_category(&cat_name)
                    }
                } else {
                    self.get_params_for_category(&cat_name)
                }
            } else {
                self.get_current_params()
            }
        };

        // 1. Budget Pre-check
        let context = self
            .current_context
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let (Some(provider), Some(ctx)) = (&self.budget_provider, context.clone()) {
            let estimated_cost = 0.01;
            if let Err(e) = provider
                .check_budget(&ctx.project_id, &ctx.goal_id, estimated_cost)
                .await
            {
                let cfg = self
                    .config
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if cfg.llm.budget_mode == sisyphus_types::BudgetMode::Strict {
                    return Err(format!("Budget exceeded (Strict Mode): {e}"));
                }
                tracing::warn!(target: "llm", "Budget warning for {}: {}", ctx.goal_id, e);
            }
        }

        // Auto-register the current model with the router for cost tracking
        self.router.register_model(ModelConfig {
            name: model.clone(),
            cost_per_1k_tokens: 0.01,
            provider: "default".into(),
            max_tokens: 128_000,
            endpoint: endpoint.clone(),
            complexity_ceiling: 100,
        });

        // Check circuit breaker before making the call
        self.breaker
            .check_state(&model)
            .map_err(|e| format!("LLM call blocked by circuit breaker ({model}): {e}"))?;

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

        let res = match self
            .client
            .post(&endpoint)
            .header("Authorization", format!("Bearer {api_key}"))
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
            let error_msg = error_obj["message"].as_str().unwrap_or("unknown error");
            return Err(format!(
                "LLM API error ({}): {}",
                status.as_u16(),
                error_msg
            ));
        }

        let content = response_json["choices"][0]["message"]["content"]
            .as_str()
            .map(std::string::ToString::to_string);

        let duration_secs = start_time.elapsed().as_secs_f64();

        if let Some(text) = content {
            self.breaker.on_success(&model);
            let token_estimate = (text.len() / 4).max(1) as u64;
            let actual_cost = (token_estimate as f64 / 1000.0) * 0.01;
            self.router.record_usage(&model, 0.0, token_estimate);

            if let (Some(provider), Some(ctx)) = (&self.budget_provider, context) {
                let _ = provider
                    .record_actual_cost(&ctx.project_id, &ctx.goal_id, actual_cost, duration_secs)
                    .await;
            }

            Ok(text)
        } else {
            self.breaker.on_failure(&model);
            Err("LLM response missing content field".to_string())
        }
    }

    fn get_params_for(&self, agent_id: &str) -> (String, String, String) {
        let cfg = self
            .config
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        if let Some(override_) = cfg.llm.agents.get(agent_id) {
            let endpoint = override_
                .endpoint
                .as_deref()
                .unwrap_or(&cfg.llm.endpoint)
                .to_string();
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

    fn breaker(&self) -> crate::circuit_breaker::CircuitBreaker {
        self.breaker.clone()
    }
}
