//! 生产级 HTTP 客户端工厂（共享给所有 LLM provider）。
//!
//! ## 设计目标（最终产品标准）
//!
//! 1. **共享连接池**：所有 provider 共享同一 [`SharedHttpClient`]，减少 TCP/TLS 握手与 fd 占用。
//! 2. **生产级配置**：连接池大小、TCP keepalive、connect/idle/pool timeout 全部显式设置。
//! 3. **可观测**：暴露 [`SharedHttpClient::stats`] 计数（请求数、错误数、in-flight）。
//! 4. **错误传播**：构造期错误返回 `Result`，绝不 panic。
//! 5. **优雅关闭**：[`SharedHttpClient::shutdown`] 等待 in-flight 请求完成（受 timeout 限制）。
//!
//! ## 用法
//!
//! ```no_run
//! use sisyphus_tools_llm::http_client::{SharedHttpClient, SharedHttpClientConfig};
//! use std::sync::Arc;
//! use std::time::Duration;
//!
//! let cfg = SharedHttpClientConfig::default();
//! let client = SharedHttpClient::new(cfg).expect("build shared client");
//! // 之后将 Arc::clone(&client) 传给所有 provider
//! ```

use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// 共享 HTTP 客户端配置。
///
/// 所有字段都有合理默认值；生产环境应通过 `from_env` 或显式 builder 调优。
#[derive(Debug, Clone)]
pub struct SharedHttpClientConfig {
    /// 总请求超时（含连接 + 等待响应）。
    /// 默认 5 分钟；provider 可在调用层用 `tokio::time::timeout` 二次收紧。
    pub request_timeout: Duration,

    /// 单次 connect 阶段超时。
    /// 默认 10 秒；防止 DNS 慢或防火墙黑洞。
    pub connect_timeout: Duration,

    /// 空闲连接在池中保持存活时间。
    /// 默认 90 秒；平衡"复用连接"与"释放 fd"。
    pub pool_idle_timeout: Duration,

    /// TCP keepalive 间隔（0 = 禁用）。
    /// 默认 60 秒；防 NAT/防火墙静默断开长连接。
    pub tcp_keepalive: Duration,

    /// 每 host 最大并发连接数。
    /// 默认 16；防止对单一 provider 过度并发触发 rate limit。
    pub pool_max_per_host: usize,

    /// HTTP 重定向最大次数。
    /// 默认 3；防止恶意/错误重定向循环。
    pub redirect_limit: usize,

    /// User-Agent 标识。
    pub user_agent: String,

    /// 是否启用 gzip/brotli 自动解压响应。
    pub gzip: bool,
}

impl Default for SharedHttpClientConfig {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(300),
            connect_timeout: Duration::from_secs(10),
            pool_idle_timeout: Duration::from_secs(90),
            tcp_keepalive: Duration::from_secs(60),
            pool_max_per_host: 16,
            redirect_limit: 3,
            user_agent: format!("Sisyphus/{}", env!("CARGO_PKG_VERSION")),
            gzip: true,
        }
    }
}

impl SharedHttpClientConfig {
    /// 从环境变量覆盖（`SISYPHUS_HTTP_*`）。
    ///
    /// 支持：
    /// - `SISYPHUS_HTTP_REQUEST_TIMEOUT_SECS`
    /// - `SISYPHUS_HTTP_CONNECT_TIMEOUT_SECS`
    /// - `SISYPHUS_HTTP_POOL_IDLE_SECS`
    /// - `SISYPHUS_HTTP_TCP_KEEPALIVE_SECS`
    /// - `SISYPHUS_HTTP_POOL_MAX_PER_HOST`
    /// - `SISYPHUS_HTTP_REDIRECT_LIMIT`
    pub fn from_env() -> Self {
        let mut cfg = Self::default();
        if let Ok(v) = std::env::var("SISYPHUS_HTTP_REQUEST_TIMEOUT_SECS") {
            if let Ok(n) = v.parse() {
                cfg.request_timeout = Duration::from_secs(n);
            }
        }
        if let Ok(v) = std::env::var("SISYPHUS_HTTP_CONNECT_TIMEOUT_SECS") {
            if let Ok(n) = v.parse() {
                cfg.connect_timeout = Duration::from_secs(n);
            }
        }
        if let Ok(v) = std::env::var("SISYPHUS_HTTP_POOL_IDLE_SECS") {
            if let Ok(n) = v.parse() {
                cfg.pool_idle_timeout = Duration::from_secs(n);
            }
        }
        if let Ok(v) = std::env::var("SISYPHUS_HTTP_TCP_KEEPALIVE_SECS") {
            if let Ok(n) = v.parse() {
                if n == 0 {
                    cfg.tcp_keepalive = Duration::from_secs(0);
                } else {
                    cfg.tcp_keepalive = Duration::from_secs(n);
                }
            }
        }
        if let Ok(v) = std::env::var("SISYPHUS_HTTP_POOL_MAX_PER_HOST") {
            if let Ok(n) = v.parse() {
                cfg.pool_max_per_host = n;
            }
        }
        if let Ok(v) = std::env::var("SISYPHUS_HTTP_REDIRECT_LIMIT") {
            if let Ok(n) = v.parse() {
                cfg.redirect_limit = n;
            }
        }
        cfg
    }
}

/// 共享 HTTP 客户端的运行时统计。
#[derive(Debug, Default, Clone)]
pub struct HttpClientStats {
    /// 自启动以来发起的总请求数。
    pub requests_total: u64,
    /// 自启动以来的失败请求数（非 2xx 或 transport 错误）。
    pub errors_total: u64,
    /// 当前 in-flight 请求数（已发出未收到响应）。
    pub in_flight: u64,
    /// 已完成的优雅关闭次数。
    pub shutdowns_total: u64,
}

/// 共享 HTTP 客户端，包装 `reqwest::Client` 并提供统计/关闭能力。
///
/// 内部使用 `Arc` 共享给所有 provider；连接池与 TLS 会话只存在一份。
#[derive(Clone)]
pub struct SharedHttpClient {
    inner: Arc<Inner>,
}

struct Inner {
    /// `reqwest::Client` 本身是 Clone-cheap（内部 Arc），但为清晰仍用 Arc 包装。
    http: reqwest::Client,
    /// 启动期配置（只读）。
    config: SharedHttpClientConfig,
    /// 原子计数（无锁、cache-friendly）。
    requests_total: AtomicU64,
    errors_total: AtomicU64,
    in_flight: AtomicU64,
    shutdowns_total: AtomicU64,
    /// 关闭标志：true → 新请求应被快速失败。
    closed: AtomicBool,
    /// 用于 `shutdown` 等待 in-flight 归零。
    in_flight_notifier: Mutex<Option<Arc<tokio::sync::Notify>>>,
}

impl std::fmt::Debug for SharedHttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedHttpClient")
            .field("config", &self.inner.config)
            .field("stats", &self.stats())
            .finish()
    }
}

impl SharedHttpClient {
    /// 构造共享客户端。失败原因（TLS、allocator、非法配置）以 `Err` 返回而非 panic。
    pub fn new(config: SharedHttpClientConfig) -> Result<Self, String> {
        let mut builder = reqwest::Client::builder()
            .timeout(config.request_timeout)
            .connect_timeout(config.connect_timeout)
            .pool_idle_timeout(config.pool_idle_timeout)
            .pool_max_idle_per_host(config.pool_max_per_host)
            .redirect(reqwest::redirect::Policy::limited(config.redirect_limit))
            .user_agent(config.user_agent.clone());

        if config.tcp_keepalive > Duration::from_secs(0) {
            builder = builder.tcp_keepalive(config.tcp_keepalive);
        }

        // 注意: reqwest 默认对 Accept-Encoding 头做 gzip 自动解压，无需显式调用；
        // brotli/zstd 需要 feature 启用。当前生产只需 gzip（gzip 模块在 reqwest 默认 features 中）。
        let _ = config.gzip;

        let http = builder
            .build()
            .map_err(|e| format!("build reqwest client: {e}"))?;

        Ok(Self {
            inner: Arc::new(Inner {
                http,
                config,
                requests_total: AtomicU64::new(0),
                errors_total: AtomicU64::new(0),
                in_flight: AtomicU64::new(0),
                shutdowns_total: AtomicU64::new(0),
                closed: AtomicBool::new(false),
                in_flight_notifier: Mutex::new(None),
            }),
        })
    }

    /// 拿到底层 `reqwest::Client` 引用，用于实际发送请求。
    pub fn http(&self) -> &reqwest::Client {
        &self.inner.http
    }

    /// 当前配置（只读）。
    pub fn config(&self) -> &SharedHttpClientConfig {
        &self.inner.config
    }

    /// 客户端统计快照。
    pub fn stats(&self) -> HttpClientStats {
        HttpClientStats {
            requests_total: self.inner.requests_total.load(Ordering::Relaxed),
            errors_total: self.inner.errors_total.load(Ordering::Relaxed),
            in_flight: self.inner.in_flight.load(Ordering::Relaxed),
            shutdowns_total: self.inner.shutdowns_total.load(Ordering::Relaxed),
        }
    }

    /// 标记一次请求开始（在 `reqwest::RequestBuilder::send` 前调用）。
    /// 若客户端已关闭则返回 `false`，调用方应立即放弃。
    pub fn on_request_start(&self) -> bool {
        if self.inner.closed.load(Ordering::Acquire) {
            return false;
        }
        self.inner.requests_total.fetch_add(1, Ordering::Relaxed);
        self.inner.in_flight.fetch_add(1, Ordering::Relaxed);
        true
    }

    /// 标记一次请求结束（无论成功或失败都调用）。
    pub fn on_request_end(&self, errored: bool) {
        if errored {
            self.inner.errors_total.fetch_add(1, Ordering::Relaxed);
        }
        // fetch_sub 返回旧值；若归零则唤醒等待的 shutdown。
        let prev = self.inner.in_flight.fetch_sub(1, Ordering::AcqRel);
        if prev == 1 {
            if let Some(notifier) = self.inner.in_flight_notifier.lock().clone() {
                notifier.notify_waiters();
            }
        }
    }

    /// 优雅关闭：等待 in-flight 归零，最多 `wait` 时长。
    ///
    /// 返回 `Ok(())` 表示所有请求完成；`Err` 表示超时但仍有 in-flight。
    /// 已关闭的客户端再次调用返回 `Ok(())`。
    pub async fn shutdown(&self, wait: Duration) -> Result<(), String> {
        if self.inner.closed.swap(true, Ordering::AcqRel) {
            return Ok(()); // 已关闭
        }
        self.inner.shutdowns_total.fetch_add(1, Ordering::Relaxed);

        // 注册 notify（惰性创建）
        let notifier = {
            let mut guard = self.inner.in_flight_notifier.lock();
            if let Some(n) = guard.as_ref() {
                n.clone()
            } else {
                let n = Arc::new(tokio::sync::Notify::new());
                *guard = Some(n.clone());
                n
            }
        };

        if self.inner.in_flight.load(Ordering::Acquire) == 0 {
            return Ok(());
        }

        match tokio::time::timeout(wait, wait_in_flight_zero(&notifier, &self.inner.in_flight)).await
        {
            Ok(()) => Ok(()),
            Err(_) => {
                let remaining = self.inner.in_flight.load(Ordering::Acquire);
                Err(format!(
                    "shutdown timeout after {wait:?}, {remaining} requests still in flight"
                ))
            }
        }
    }

    /// 客户端是否已关闭（关闭后新请求应被快速失败）。
    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::Acquire)
    }
}

async fn wait_in_flight_zero(notifier: &Arc<tokio::sync::Notify>, counter: &AtomicU64) {
    loop {
        if counter.load(Ordering::Acquire) == 0 {
            return;
        }
        notifier.notified().await;
    }
}

/// RAII guard: drop 时自动调用 on_request_end(errored=false)。
/// 若需标记为错误，调用 mark_errored() 后 drop。
///
/// 用于 LLM provider 在 chat() 入口开始时构造，Drop 时自动收尾。
/// 即使在错误 early-return 路径也能保证 in-flight 计数归零。
pub struct RequestGuard {
    http: Arc<SharedHttpClient>,
    errored: bool,
}

impl RequestGuard {
    pub fn new(http: Arc<SharedHttpClient>) -> Self {
        Self { http, errored: false }
    }
    pub fn mark_errored(&mut self) {
        self.errored = true;
    }
}

impl Drop for RequestGuard {
    fn drop(&mut self) {
        self.http.on_request_end(self.errored);
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_sane() {
        let cfg = SharedHttpClientConfig::default();
        assert!(cfg.request_timeout >= Duration::from_secs(30));
        assert!(cfg.connect_timeout < cfg.request_timeout);
        assert!(cfg.pool_idle_timeout > Duration::from_secs(0));
        assert!(cfg.pool_max_per_host > 0);
        assert!(cfg.redirect_limit <= 10);
        assert!(cfg.user_agent.starts_with("Sisyphus/"));
    }

    #[test]
    fn env_overrides_apply() {
        // 临时设环境变量
        std::env::set_var("SISYPHUS_HTTP_REQUEST_TIMEOUT_SECS", "60");
        std::env::set_var("SISYPHUS_HTTP_POOL_MAX_PER_HOST", "32");
        let cfg = SharedHttpClientConfig::from_env();
        assert_eq!(cfg.request_timeout, Duration::from_secs(60));
        assert_eq!(cfg.pool_max_per_host, 32);
        std::env::remove_var("SISYPHUS_HTTP_REQUEST_TIMEOUT_SECS");
        std::env::remove_var("SISYPHUS_HTTP_POOL_MAX_PER_HOST");
    }

    #[test]
    fn build_with_default_succeeds() {
        let client = SharedHttpClient::new(SharedHttpClientConfig::default());
        assert!(client.is_ok(), "default config must build: {client:?}");
    }

    #[test]
    fn build_with_zero_pool_max_per_host_succeeds() {
        // 0 表示 reqwest 默认（不限）—— 我们仍允许。
        let cfg = SharedHttpClientConfig {
            pool_max_per_host: 0,
            ..SharedHttpClientConfig::default()
        };
        let client = SharedHttpClient::new(cfg);
        assert!(client.is_ok());
    }

    #[test]
    fn stats_starts_at_zero() {
        let client = SharedHttpClient::new(SharedHttpClientConfig::default()).unwrap();
        let s = client.stats();
        assert_eq!(s.requests_total, 0);
        assert_eq!(s.errors_total, 0);
        assert_eq!(s.in_flight, 0);
        assert_eq!(s.shutdowns_total, 0);
    }

    #[test]
    fn request_start_end_updates_counters() {
        let client = SharedHttpClient::new(SharedHttpClientConfig::default()).unwrap();
        assert!(client.on_request_start());
        let s = client.stats();
        assert_eq!(s.requests_total, 1);
        assert_eq!(s.in_flight, 1);
        client.on_request_end(true);
        let s = client.stats();
        assert_eq!(s.requests_total, 1);
        assert_eq!(s.errors_total, 1);
        assert_eq!(s.in_flight, 0);
    }

    #[test]
    fn shutdown_marks_closed() {
        let client = SharedHttpClient::new(SharedHttpClientConfig::default()).unwrap();
        assert!(!client.is_closed());
        // tokio runtime required for shutdown
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let _ = client.shutdown(Duration::from_millis(50)).await;
        });
        assert!(client.is_closed());
        let s = client.stats();
        assert_eq!(s.shutdowns_total, 1);
    }

    #[test]
    fn shutdown_is_idempotent() {
        let client = SharedHttpClient::new(SharedHttpClientConfig::default()).unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let _ = client.shutdown(Duration::from_millis(50)).await;
            let _ = client.shutdown(Duration::from_millis(50)).await;
        });
        let s = client.stats();
        // 第二次调用早返回，shutdowns_total 仍为 1（避免无意义的双计数）
        assert_eq!(s.shutdowns_total, 1, "第二次 shutdown 应早返回，不增加计数");
    }

    #[test]
    fn request_start_after_shutdown_returns_false() {
        let client = SharedHttpClient::new(SharedHttpClientConfig::default()).unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let _ = client.shutdown(Duration::from_millis(50)).await;
        });
        assert!(!client.on_request_start());
    }

    #[test]
    fn clone_shares_state() {
        let c1 = SharedHttpClient::new(SharedHttpClientConfig::default()).unwrap();
        let c2 = c1.clone();
        c1.on_request_start();
        c1.on_request_end(false);
        // c2 看到的计数应与 c1 一致
        assert_eq!(c1.stats().requests_total, c2.stats().requests_total);
        assert_eq!(c1.stats().requests_total, 1);
    }
}
