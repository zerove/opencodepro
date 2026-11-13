//! Dynamic LLM client that retrieves configuration from a shared config store.
//!
//! Supports per-agent model overrides, circuit breaker, model router, and budget tracking.

use async_trait::async_trait;
use parking_lot::RwLock as ParkingRwLock;
use serde_json::{json, Value};
use sisyphus_types::Config;
use std::collections::HashMap;
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
    /// Per-model fast-fail cooling: after a connection-level failure for a
    /// specific model, calls to that model are rejected immediately until the
    /// window expires. An unreachable model degrades quickly while others
    /// remain usable.
    fast_fail_until: Arc<ParkingRwLock<HashMap<String, std::time::Instant>>>,
    /// provider 上报的 token 用量（仅统计真实上报值，不混估算）。
    token_usage: Arc<TokenUsageCounters>,
    /// 检测到 provider **静默截断**了 prompt（上报的 prompt token 远小于我们
    /// 实际发出的量）。本地 provider（ollama 默认 num_ctx 较小）会保留尾部、
    /// 丢掉开头，且不报错 —— 模型因此可能在「没看到开头」的情况下作答。
    prompt_truncated: Arc<std::sync::atomic::AtomicBool>,
}

tokio::task_local! {
    /// 当前执行作用域的 token 累加器（可选）。存在时，`TokenUsageCounters::record`
    /// 会同时写入全局累计与该作用域，用于执行级确定性归因（不依赖全局差值/租约）。
    /// 需通过 `DynamicLlmClient::scope_usage` 在 async 作用域内激活；跨 `tokio::spawn`
    /// 的子任务不会自动继承，调用方需在需要归因的边界显式包裹。
    static EXECUTION_USAGE_SCOPE: Arc<TokenUsageCounters>;
}

/// provider 上报的 token 计数（原子累加）。
#[derive(Debug, Default)]
struct TokenUsageCounters {
    prompt_tokens: std::sync::atomic::AtomicU64,
    completion_tokens: std::sync::atomic::AtomicU64,
    calls: std::sync::atomic::AtomicU64,
}

impl TokenUsageCounters {
    fn record(&self, prompt: Option<u64>, completion: Option<u64>) {
        use std::sync::atomic::Ordering;
        // 只有一个字段上报时也算一次有效上报：宁少不少，但不伪造。
        if prompt.is_none() && completion.is_none() {
            return;
        }
        if let Some(v) = prompt {
            self.prompt_tokens.fetch_add(v, Ordering::Relaxed);
        }
        if let Some(v) = completion {
            self.completion_tokens.fetch_add(v, Ordering::Relaxed);
        }
        self.calls.fetch_add(1, Ordering::Relaxed);

        // 同步写入当前执行作用域（若已激活），实现执行级归因。
        let _ = EXECUTION_USAGE_SCOPE.try_with(|scope| {
            if let Some(v) = prompt {
                scope.prompt_tokens.fetch_add(v, Ordering::Relaxed);
            }
            if let Some(v) = completion {
                scope.completion_tokens.fetch_add(v, Ordering::Relaxed);
            }
            scope.calls.fetch_add(1, Ordering::Relaxed);
        });
    }

    fn snapshot(&self) -> sisyphus_types::LlmTokenUsage {
        use std::sync::atomic::Ordering;
        sisyphus_types::LlmTokenUsage {
            prompt_tokens: self.prompt_tokens.load(Ordering::Relaxed),
            completion_tokens: self.completion_tokens.load(Ordering::Relaxed),
            calls: self.calls.load(Ordering::Relaxed),
        }
    }

    /// 转为对外可见快照；从未收到 provider 上报 usage 时返回 `None`（未知 ≠ 0）。
    fn snapshot_option(&self) -> Option<sisyphus_types::LlmTokenUsage> {
        let snap = self.snapshot();
        (snap.calls > 0).then_some(snap)
    }
}

/// 粗略估算一段文本的 token 数。
///
/// CJK 字符约 1 字 1 token，其余约 4 字符 1 token。只用于「provider 是不是
/// 截断了我们的 prompt」这一相对比较，不需要精确。
fn estimate_tokens(text: &str) -> u64 {
    let mut cjk = 0u64;
    let mut other = 0u64;
    for c in text.chars() {
        let is_cjk = matches!(c as u32,
            0x3000..=0x303F | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF
            | 0xFF00..=0xFFEF | 0x20000..=0x2FA1F);
        if is_cjk {
            cjk += 1;
        } else {
            other += 1;
        }
    }
    cjk + other / 4
}

impl DynamicLlmClient {
    /// 本次运行中，provider 是否**静默截断**过 prompt。
    ///
    /// 典型场景：本地 ollama 的 `num_ctx` 默认较小（实测 37KB 的 prompt 被裁到
    /// ~2050 token，保留尾部、丢掉开头，且不报错）。此时模型可能根本没看到
    /// 任务的开头部分，却仍会给出自信的答案。
    pub fn prompt_truncated(&self) -> bool {
        self.prompt_truncated.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// 比较「我们发出的 token 估算」与「provider 上报的 prompt token」，
    /// 差距过大即判定 provider 截断了 prompt。
    fn note_prompt_truncation(&self, sent_text: &str, reported_prompt_tokens: Option<u64>) {
        use std::sync::atomic::Ordering;
        let Some(reported) = reported_prompt_tokens else {
            return;
        };
        let estimate = estimate_tokens(sent_text);
        // 容忍 30% 的估算误差；低于 70% 视为被裁剪。
        if estimate > 512 && reported * 10 < estimate * 7 {
            self.prompt_truncated.store(true, Ordering::Relaxed);
            tracing::warn!(
                target: "llm",
                sent_estimate = estimate,
                reported_prompt_tokens = reported,
                "provider 截断了 prompt：模型可能没有看到上下文开头"
            );
        }
    }

    pub fn new(config: Arc<RwLock<Config>>) -> Self {
        // 连接超时在建 client 时固定（配置在构造前已加载）；**请求超时必须在每次
        // 调用时按配置下发**。
        //
        // 旧实现把 `.timeout(5s)` 钉死在 client 上，而 `chat()` 只是在外层再套一个
        // `tokio::time::timeout(config.request_timeout_secs)`。两者取小 —— 于是
        // `request_timeout_secs` 调到 60s 也仍会被 client 的 5s 掐断：本地模型
        // （ollama 3B 生成 100 token 约 9s）和任何慢速 provider 100% 失败，且调配置
        // 无法自救。连接阶段仍按配置收紧，避免 DNS/防火墙黑洞。
        let connect_timeout_secs =
            config.read().map(|c| c.llm.connect_timeout_secs).unwrap_or(2).max(1);
        // 自定义构建器失败时降级为默认 Client::new()（无失败路径，避免 panic）。
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(connect_timeout_secs))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let provider_registry =
            config.read().ok().and_then(|cfg| build_provider_registry(&cfg.llm.providers).ok());
        Self {
            config,
            client,
            breaker: CircuitBreaker::default(),
            router: Arc::new(ModelRouter::new(0.0)),
            budget_provider: None,
            current_context: Arc::new(RwLock::new(None)),
            current_category: Arc::new(RwLock::new(None)),
            provider_registry,
            fast_fail_until: Arc::new(ParkingRwLock::new(HashMap::new())),
            token_usage: Arc::new(TokenUsageCounters::default()),
            prompt_truncated: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Record a connection-level failure for a specific model and open the
    /// fast-fail cooling window so subsequent calls to that model are rejected
    /// without touching the network.
    fn trip_fast_fail(&self, model: &str) {
        let cooling_secs = {
            let cfg = self.config.read().unwrap_or_else(std::sync::PoisonError::into_inner);
            cfg.llm.fast_fail_cooling_secs
        };
        self.fast_fail_until.write().insert(
            model.to_string(),
            std::time::Instant::now() + Duration::from_secs(cooling_secs),
        );
    }

    /// Clear the cooling window for a specific model after a successful call.
    fn clear_fast_fail(&self, model: &str) {
        self.fast_fail_until.write().remove(model);
    }

    /// True when the fast-fail cooling window for a model is active.
    fn fast_fail_active(&self, model: &str) -> bool {
        let cooling_secs = {
            let cfg = self.config.read().unwrap_or_else(std::sync::PoisonError::into_inner);
            cfg.llm.fast_fail_cooling_secs
        };
        if cooling_secs == 0 {
            return false;
        }
        let until = self.fast_fail_until.read().get(model).copied();
        matches!(until, Some(t) if t > std::time::Instant::now())
    }

    /// Get the current request/connect timeouts from config.
    fn timeouts(&self) -> (Duration, Duration) {
        let cfg = self.config.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        (
            Duration::from_secs(cfg.llm.connect_timeout_secs),
            Duration::from_secs(cfg.llm.request_timeout_secs),
        )
    }

    /// With provider registry.
    pub fn with_provider_registry(mut self, registry: ProviderRegistry) -> Self {
        self.provider_registry = Some(registry);
        self
    }

    /// With budget provider.
    pub fn with_budget_provider(mut self, provider: Arc<dyn BudgetProvider>) -> Self {
        self.budget_provider = Some(provider);
        self
    }

    /// Set context.
    pub fn set_context(&self, project_id: &str, goal_id: &str) {
        let mut ctx =
            self.current_context.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        *ctx = Some(RequestContext {
            project_id: project_id.to_string(),
            goal_id: goal_id.to_string(),
        });
    }

    /// Clear context.
    pub fn clear_context(&self) {
        let mut ctx =
            self.current_context.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        *ctx = None;
    }

    /// Set category.
    pub fn set_category(&self, category: &str) {
        let mut cat =
            self.current_category.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        *cat = Some(category.to_string());
    }

    /// Clear category.
    pub fn clear_category(&self) {
        let mut cat =
            self.current_category.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        *cat = None;
    }

    /// With breaker.
    pub fn with_breaker(mut self, breaker: CircuitBreaker) -> Self {
        self.breaker = breaker;
        self
    }

    /// With router.
    pub fn with_router(mut self, router: Arc<ModelRouter>) -> Self {
        self.router = router;
        self
    }

    /// Breaker.
    pub fn breaker(&self) -> &CircuitBreaker {
        &self.breaker
    }

    /// Router.
    pub fn router(&self) -> &Arc<ModelRouter> {
        &self.router
    }

    fn resolve_api_key(cfg_api_key: &str) -> String {
        std::env::var("GOOGLE_GENERATIVE_AI_API_KEY").unwrap_or_else(|_| cfg_api_key.to_string())
    }

    /// Get base (default) LLM parameters.
    pub(super) fn get_current_params(&self) -> (String, String, String) {
        let cfg = self.config.read().unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let cfg = self.config.read().unwrap_or_else(std::sync::PoisonError::into_inner);

        if let Some(override_) = cfg.llm.categories.get(category) {
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

    /// 在“执行级 token 作用域”内运行 future，并返回作用域内累计的 provider 上报 usage。
    ///
    /// - `TokenUsageCounters::record` 会在更新全局累计的同时写入当前作用域（若存在），
    ///   因而在作用域覆盖到所有相关 LLM 调用时可实现确定性归因（不依赖全局差值/租约）。
    /// - `tokio::spawn` 的子任务不会自动继承 task-local；若执行在子任务中发起 LLM 调用，
    ///   需在相应边界显式传播/包裹作用域，否则该部分 usage 无法归因到本作用域（返回 `None`
    ///   或部分计数，按实际可见调用而定；调用方不得臆造）。
    /// - 作用域内从未收到 provider 上报 usage 时返回 `None`（未知 ≠ 0）。
    pub async fn scope_usage<Fut, T>(fut: Fut) -> (T, Option<sisyphus_types::LlmTokenUsage>)
    where
        Fut: std::future::Future<Output = T>,
    {
        let scope = Arc::new(TokenUsageCounters::default());
        let result = EXECUTION_USAGE_SCOPE.scope(scope.clone(), fut).await;
        (result, scope.snapshot_option())
    }

    /// 返回当前执行作用域的累计 usage 快照（若处于 `scope_usage` 作用域内且有上报）。
    /// 不在作用域内或无上报时返回 `None`。
    pub fn current_scope_usage_snapshot() -> Option<sisyphus_types::LlmTokenUsage> {
        EXECUTION_USAGE_SCOPE.try_with(|scope| scope.snapshot_option()).ok().flatten()
    }
}

#[async_trait]
impl LlmClient for DynamicLlmClient {
    async fn generate(&self, prompt: &str) -> Result<String, sisyphus_types::TypesError> {
        self.chat("", prompt, false).await
    }

    async fn chat(
        &self,
        system: &str,
        user: &str,
        json_mode: bool,
    ) -> Result<String, sisyphus_types::TypesError> {
        let start_time = std::time::Instant::now();

        // We'll need the model name for per-model fast-fail. Resolve early.
        let (api_key, endpoint, model) = {
            let category = self
                .current_category
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            if let Some(cat_name) = category {
                let cfg = self.config.read().unwrap_or_else(std::sync::PoisonError::into_inner);
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

        // ── Per-model fast-fail cooling ───────────────────────────────
        // If a recent connection-level failure was recorded for this model,
        // reject the call immediately (no network attempt) so an unreachable
        // model degrades the cognitive loop in ~ms instead of stalling.
        if self.fast_fail_active(&model) {
            return Err(sisyphus_types::TypesError::InternalError(format!(
                "Model {model} unavailable (fast-fail cooling, previous call failed)"
            )));
        }

        // ── Try provider registry fallback chain first ─────────────
        if let Some(ref registry) = self.provider_registry {
            if !registry.fallback_order().is_empty() {
                let chat_messages = vec![ChatMessage {
                    role: ChatRole::User,
                    content: user.to_string(),
                }];
                match registry.chat_with_fallback(system, &chat_messages, json_mode).await {
                    Ok(response) => {
                        // Record cost via router
                        let token_estimate = (response.content.len() / 4).max(1) as u64;
                        self.router.record_usage(&response.model, 0.0, token_estimate);
                        self.token_usage.record(response.input_tokens, response.output_tokens);
                        self.note_prompt_truncation(
                            &format!("{system}\n{user}"),
                            response.input_tokens,
                        );
                        self.clear_fast_fail(&model);
                        return Ok(response.content);
                    },
                    Err(e) => {
                        // All providers failed — log warning and fall through to default endpoint
                        tracing::warn!(target: "llm", "All providers failed, falling back to default endpoint: {e}");
                    },
                }
            }
        }

        // 1. Budget Pre-check
        let context =
            self.current_context.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        if let (Some(provider), Some(ctx)) = (&self.budget_provider, context.clone()) {
            let estimated_cost = 0.01;
            if let Err(e) =
                provider.check_budget(&ctx.project_id, &ctx.goal_id, estimated_cost).await
            {
                let cfg = self.config.read().unwrap_or_else(std::sync::PoisonError::into_inner);
                if cfg.llm.budget_mode == sisyphus_types::BudgetMode::Strict {
                    return Err(sisyphus_types::TypesError::InternalError(format!(
                        "Budget exceeded (Strict Mode): {e}"
                    )));
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
        self.breaker.check_state(&model).map_err(|e| {
            sisyphus_types::TypesError::InternalError(format!(
                "LLM call blocked by circuit breaker ({model}): {e}"
            ))
        })?;

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

        let request_timeout = self.timeouts().1;

        let res = match tokio::time::timeout(
            request_timeout,
            self.client
                .post(&endpoint)
                // 每次调用按当前配置下发请求超时（client 级不再钉死任何请求超时）。
                .timeout(request_timeout)
                .header("Authorization", format!("Bearer {api_key}"))
                .json(&body)
                .send(),
        )
        .await
        {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => {
                self.breaker.on_failure(&model);
                self.trip_fast_fail(&model);
                // 连接/请求超时与「连不上」是两类不同问题，混在 "HTTP request failed"
                // 里用户无法判断该改超时还是改端点 —— 这里显式区分。
                if e.is_timeout() {
                    // 连接阶段与「已连上但响应慢」是两类问题：前者是端点不可达，
                    // 调大超时毫无用处；后者才是慢模型/慢网关。混为一谈会把用户
                    // 引向错误的修复方向。
                    return Err(sisyphus_types::TypesError::InternalError(
                        if e.is_connect() {
                            format!(
                            "LLM connect failed for {model} at {endpoint} (endpoint unreachable or port not listening)"
                        )
                        } else {
                            format!(
                            "LLM request timed out after {}s for {model} (raise `request_timeout_secs` for slow/local providers)",
                            request_timeout.as_secs()
                        )
                        },
                    ));
                }
                return Err(sisyphus_types::TypesError::InternalError(format!(
                    "HTTP request failed: {e}"
                )));
            },
            Err(_) => {
                self.breaker.on_failure(&model);
                self.trip_fast_fail(&model);
                return Err(sisyphus_types::TypesError::InternalError(format!(
                    "HTTP request timed out for {model}"
                )));
            },
        };

        let status = res.status();
        let response_json: Value = match res.json().await {
            Ok(j) => j,
            Err(e) => {
                self.breaker.on_failure(&model);
                return Err(sisyphus_types::TypesError::InternalError(format!(
                    "Failed to parse response JSON: {e}"
                )));
            },
        };

        if !status.is_success() {
            self.breaker.on_failure(&model);
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

        let content = response_json["choices"][0]["message"]["content"]
            .as_str()
            .map(std::string::ToString::to_string);

        let duration_secs = start_time.elapsed().as_secs_f64();

        if let Some(text) = content {
            self.breaker.on_success(&model);
            self.clear_fast_fail(&model);
            // provider 上报的 usage 才是真实值；缺失时保持「未知」而不是补 0。
            self.token_usage.record(
                response_json["usage"]["prompt_tokens"].as_u64(),
                response_json["usage"]["completion_tokens"].as_u64(),
            );
            self.note_prompt_truncation(
                &format!("{system}\n{user}"),
                response_json["usage"]["prompt_tokens"].as_u64(),
            );
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
            Err(sisyphus_types::TypesError::InternalError(
                "LLM response missing content field".into(),
            ))
        }
    }

    fn get_params_for(&self, agent_id: &str) -> (String, String, String) {
        let cfg = self.config.read().unwrap_or_else(std::sync::PoisonError::into_inner);

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

    fn breaker(&self) -> crate::circuit_breaker::CircuitBreaker {
        self.breaker.clone()
    }

    async fn shutdown(&self, wait: std::time::Duration) -> Result<(), String> {
        // 等待 provider registry 共享 HTTP 池的 in-flight 请求完成。
        // None 表示配置未加载；直接返回 Ok。
        if let Some(reg) = self.provider_registry.as_ref() {
            if let Some(shared) = reg.shared_http() {
                return shared.shutdown(wait).await;
            }
        }
        Ok(())
    }

    /// 暴露底层 shared_http（用于 /api/metrics 等指标端点）。
    /// 返回 None 表示 ProviderRegistry 未配置（开发模式）。
    fn http_stats(&self) -> Option<sisyphus_types::HttpClientStats> {
        self.provider_registry.as_ref().and_then(|reg| reg.shared_http()).map(|c| c.stats())
    }

    /// provider 上报的 token 用量；从未上报过则返回 `None`（未知 ≠ 0）。
    fn token_usage(&self) -> Option<sisyphus_types::LlmTokenUsage> {
        let snapshot = self.token_usage.snapshot();
        (snapshot.calls > 0).then_some(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    fn client() -> DynamicLlmClient {
        let config = Arc::new(RwLock::new(Config::default()));
        DynamicLlmClient::new(config)
    }

    #[test]
    fn fast_fail_cooling_blocks_after_trip() {
        let c = client();
        assert!(
            !c.fast_fail_active("test-model"),
            "cooling should start inactive"
        );
        c.trip_fast_fail("test-model");
        assert!(
            c.fast_fail_active("test-model"),
            "cooling should activate after trip"
        );
        c.clear_fast_fail("test-model");
        assert!(
            !c.fast_fail_active("test-model"),
            "cooling should clear on success"
        );
        // Other models unaffected
        assert!(
            !c.fast_fail_active("other-model"),
            "other model should not be cooled"
        );
    }

    #[test]
    fn fast_fail_cooling_expires() {
        let c = client();
        c.trip_fast_fail("test-model");
        // Manually rewind the expiry so we don't sleep 30s in the test.
        c.fast_fail_until.write().insert(
            "test-model".to_string(),
            std::time::Instant::now() - Duration::from_secs(1),
        );
        assert!(
            !c.fast_fail_active("test-model"),
            "cooling should expire after window"
        );
    }

    #[tokio::test]
    async fn chat_rejected_during_cooling_without_network() {
        let c = client();
        c.trip_fast_fail("gpt-4o");
        let err = c.chat("sys", "user", false).await.unwrap_err();
        assert!(
            err.to_string().contains("fast-fail cooling"),
            "expected fast-fail error, got: {err}"
        );
    }
    /// 回归：`request_timeout_secs` 必须真正生效，不能被 client 级硬编码超时钳制。
    ///
    /// 旧实现把 `.timeout(5s)` 钉在 reqwest client 上，本用例的慢响应（6s）必然
    /// 超时；修好后按配置 10s 放行。响应延迟 6s 是刻意越过 5s 的旧硬编码。
    #[tokio::test]
    async fn request_timeout_secs_is_not_clamped() {
        use std::io::{Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("绑定回环端口");
        let addr = listener.local_addr().expect("读取本地地址");
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                std::thread::sleep(Duration::from_secs(6));
                let body = r#"{"choices":[{"message":{"content":"PONG"}}]}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });

        let mut cfg = Config::default();
        cfg.llm.endpoint = format!("http://{addr}/v1/chat/completions");
        cfg.llm.request_timeout_secs = 10;
        let client = DynamicLlmClient::new(Arc::new(RwLock::new(cfg)));

        let out = client.chat("", "hi", false).await.expect("6s 慢响应应在 10s 超时内成功");
        assert_eq!(out, "PONG");
    }

    /// 起一个只回一次响应的本地 mock server，返回给定 body。
    fn mock_server_once(body: &'static str) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("绑定回环端口");
        let addr = listener.local_addr().expect("读取本地地址");
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let resp = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        format!("http://{addr}/v1/chat/completions")
    }

    #[tokio::test]
    async fn token_usage_records_provider_reported_tokens() {
        let endpoint = mock_server_once(
            r#"{"model":"m","choices":[{"message":{"content":"ok"}}],"usage":{"prompt_tokens":120,"completion_tokens":34}}"#,
        );
        let mut cfg = Config::default();
        cfg.llm.endpoint = endpoint;
        let client = DynamicLlmClient::new(Arc::new(RwLock::new(cfg)));

        client.chat("", "hi", false).await.expect("mock 响应应成功");

        let usage = client.token_usage().expect("provider 上报了 usage");
        assert_eq!(usage.prompt_tokens, 120);
        assert_eq!(usage.completion_tokens, 34);
        assert_eq!(usage.calls, 1);
        assert_eq!(usage.total_tokens(), 154);
    }

    #[tokio::test]
    async fn prompt_truncation_is_detected_when_provider_reports_fewer_tokens() {
        // provider 静默裁剪 prompt（本地 ollama 的 num_ctx 较小时会保留尾部、
        // 丢掉开头且不报错）→ 引擎必须能察觉并提示。
        let endpoint = mock_server_once(
            r#"{"model":"m","choices":[{"message":{"content":"ok"}}],"usage":{"prompt_tokens":20,"completion_tokens":5}}"#,
        );
        let mut cfg = Config::default();
        cfg.llm.endpoint = endpoint;
        let client = DynamicLlmClient::new(Arc::new(RwLock::new(cfg)));

        let long_prompt = "x".repeat(8000);
        client.chat("sys", &long_prompt, false).await.expect("mock 响应应成功");

        assert!(
            client.prompt_truncated(),
            "上报 token 远小于实际发送量时应判定为截断"
        );
    }

    #[tokio::test]
    async fn prompt_truncation_not_flagged_when_tokens_match() {
        let endpoint = mock_server_once(
            r#"{"model":"m","choices":[{"message":{"content":"ok"}}],"usage":{"prompt_tokens":20,"completion_tokens":5}}"#,
        );
        let mut cfg = Config::default();
        cfg.llm.endpoint = endpoint;
        let client = DynamicLlmClient::new(Arc::new(RwLock::new(cfg)));

        client.chat("sys", "hi", false).await.expect("mock 响应应成功");

        assert!(!client.prompt_truncated(), "正常长度不应判定为截断");
    }

    #[test]
    fn estimate_tokens_counts_cjk_and_ascii_differently() {
        // 100 个汉字 ≈ 100 token；400 个 ASCII ≈ 100 token。
        let cjk = "字".repeat(100);
        let ascii = "a".repeat(400);
        assert_eq!(estimate_tokens(&cjk), 100);
        assert_eq!(estimate_tokens(&ascii), 100);
    }

    #[tokio::test]
    async fn token_usage_is_unknown_when_provider_omits_usage() {
        // 没有 usage 字段时必须报「未知」（None），而不是补 0 ——
        // 补 0 会把「未上报」混进成本均值里。
        let endpoint =
            mock_server_once(r#"{"model":"m","choices":[{"message":{"content":"ok"}}]}"#);
        let mut cfg = Config::default();
        cfg.llm.endpoint = endpoint;
        let client = DynamicLlmClient::new(Arc::new(RwLock::new(cfg)));

        client.chat("", "hi", false).await.expect("mock 响应应成功");

        assert!(client.token_usage().is_none(), "未上报 usage 时应为 None");
    }

    #[tokio::test]
    async fn scope_usage_captures_reported_tokens_deterministically() {
        let endpoint = mock_server_once(
            r#"{"model":"m","choices":[{"message":{"content":"ok"}}],"usage":{"prompt_tokens":11,"completion_tokens":7}}"#,
        );
        let mut cfg = Config::default();
        cfg.llm.endpoint = endpoint;
        let client = Arc::new(DynamicLlmClient::new(Arc::new(RwLock::new(cfg))));
        let c = client.clone();

        let (res, usage) =
            DynamicLlmClient::scope_usage(async move { c.chat("", "hi", false).await }).await;
        res.expect("mock 响应应成功");
        let usage = usage.expect("作用域内应捕获上报 usage");
        assert_eq!(usage.prompt_tokens, 11);
        assert_eq!(usage.completion_tokens, 7);
        assert_eq!(usage.calls, 1);
        // 全局累计仍应可见
        let global = client.token_usage().expect("全局也应有 usage");
        assert_eq!(global.prompt_tokens, 11);
        assert_eq!(global.completion_tokens, 7);
    }

    #[tokio::test]
    async fn scope_usage_is_unknown_when_provider_omits_usage() {
        let endpoint =
            mock_server_once(r#"{"model":"m","choices":[{"message":{"content":"ok"}}]}"#);
        let mut cfg = Config::default();
        cfg.llm.endpoint = endpoint;
        let client = Arc::new(DynamicLlmClient::new(Arc::new(RwLock::new(cfg))));
        let c = client.clone();

        let (res, usage) =
            DynamicLlmClient::scope_usage(async move { c.chat("", "hi", false).await }).await;
        res.expect("mock 响应应成功");
        assert!(
            usage.is_none(),
            "未上报 usage 时作用域应为 None（未知 ≠ 0）"
        );
        assert!(DynamicLlmClient::current_scope_usage_snapshot().is_none());
    }
}
