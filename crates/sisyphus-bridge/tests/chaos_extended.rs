//! 混沌测试扩展（TODO 7.2 第四项 + 阶段 8.2 健康度端点 + 指标端点）
//!
//! 覆盖 bridge 端未在 bridge_integration_test.rs 测的边角：
//! - 并发请求竞态（同一 session 并行 health_check / execute_tool）
//! - 高并发短突发（100 并发 health_check）
//! - 大 payload 鲁棒性（10KB JSON args 不导致 panic）
//! - 重复连接（10 次连/断/连）
//! - K8s 标准健康度端点（/health/live, /health/ready）
//! - Prometheus 指标端点 (/api/metrics)

use sisyphus_bridge::bridge_service::MySisyphusBridge;
use sisyphus_bridge_api::{
    sisyphus_bridge_client::SisyphusBridgeClient, sisyphus_bridge_server::SisyphusBridgeServer,
    ExecuteSopRequest, ExecuteToolRequest, HealthCheckRequest, ListSoPsRequest,
};
use sisyphus_tools_llm::metis::DummyLlmClient;
use sisyphus_types::Config;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;
use std::time::{Duration, Instant};
use tonic::transport::{Channel, Server};

async fn setup_bridge() -> (SisyphusBridgeClient<Channel>, tokio::task::JoinHandle<()>) {
    let mut config = Config::default();
    config.cache.database_url = "sqlite::memory:".to_string();
    config.sandbox.wasm_tools_dir = "tests/wasm_tools".to_string();
    config.llm.embedding.allow_dummy_embeddings = true;
    let shared_config = Arc::new(RwLock::new(config));

    let bridge = MySisyphusBridge::new(shared_config, Some(Arc::new(DummyLlmClient)), None)
        .await
        .expect("Failed to create bridge");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_addr = format!("http://{}", addr);

    let bridge_clone = bridge.clone();
    let server_handle = tokio::spawn(async move {
        Server::builder()
            .add_service(SisyphusBridgeServer::new(bridge_clone))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .expect("Server failed");
    });

    let client = SisyphusBridgeClient::connect(server_addr).await.unwrap();
    (client, server_handle)
}

#[tokio::test]
async fn chaos_concurrent_health_check_no_race() {
    let (client, server) = setup_bridge().await;
    let mut handles = vec![];
    for _ in 0..50 {
        let mut c = client.clone();
        handles.push(tokio::spawn(async move {
            let _ = c.health_check(HealthCheckRequest {}).await;
        }));
    }
    let start = Instant::now();
    for h in handles {
        let r = tokio::time::timeout(Duration::from_secs(5), h).await;
        assert!(r.is_ok(), "concurrent health_check timed out");
    }
    let elapsed = start.elapsed();
    assert!(elapsed < Duration::from_secs(5), "took {:?}", elapsed);
    server.abort();
}

#[tokio::test]
async fn chaos_concurrent_list_sops_no_race() {
    let (client, server) = setup_bridge().await;
    let mut handles = vec![];
    for _ in 0..30 {
        let mut c = client.clone();
        handles.push(tokio::spawn(async move {
            let _ = c.list_so_ps(ListSoPsRequest {}).await;
        }));
    }
    let start = Instant::now();
    for h in handles {
        let r = tokio::time::timeout(Duration::from_secs(5), h).await;
        assert!(r.is_ok(), "concurrent list_sops timed out");
    }
    assert!(start.elapsed() < Duration::from_secs(5));
    server.abort();
}

#[tokio::test]
async fn chaos_burst_100_health_checks() {
    let (client, server) = setup_bridge().await;
    let start = Instant::now();
    let mut handles = vec![];
    for _ in 0..100 {
        let mut c = client.clone();
        handles.push(tokio::spawn(async move {
            c.health_check(HealthCheckRequest {}).await
        }));
    }
    let mut success = 0;
    for h in handles {
        if let Ok(Ok(_)) = tokio::time::timeout(Duration::from_secs(10), h).await {
            success += 1;
        }
    }
    let elapsed = start.elapsed();
    assert!(
        success >= 90,
        "only {} / 100 succeeded in {:?}",
        success,
        elapsed
    );
    server.abort();
}

#[tokio::test]
async fn chaos_large_args_payload_does_not_crash() {
    let (mut client, server) = setup_bridge().await;
    let mut big_args = HashMap::new();
    big_args.insert("data".to_string(), "x".repeat(10_000));
    big_args.insert("name".to_string(), "test_tool".to_string());
    let r = ExecuteToolRequest {
        tool_id: "unknown_xyz_12345".to_string(),
        args_json: serde_json::to_string(&big_args).unwrap(),
        fuel_limit: 10000,
    };
    let resp = client.execute_tool(r).await.expect("ExecuteTool transport failed");
    let inner = resp.into_inner();
    let msg = format!("{inner:?}");
    assert!(
        inner.status != "SUCCESS",
        "large payload must not succeed silently"
    );
    assert!(
        !msg.to_lowercase().contains("panic"),
        "unexpected panic: {msg}"
    );
    server.abort();
}

#[tokio::test]
async fn chaos_repeated_connect_disconnect() {
    for _i in 0..10 {
        let (client, server) = setup_bridge().await;
        let _r = client.clone().health_check(HealthCheckRequest {}).await;
        server.abort();
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

// ── K8s 健康度端点（阶段 8.2）─────────────────────────────
//
// 端点 /health/live 和 /health/ready 由 axum 路由提供，区别于 gRPC 端点。
// 这里在测试中通过 spawn axum server 后用 reqwest (HTTP/1.1) 直接测。
// 关键修正（两处）：
//   1. axum 0.7 默认只接 HTTP/1.1；reqwest 必须 .http1_only() 否则用
//      HTTP/2 prior knowledge 升级，axum 返回 502 Bad Gateway（connection-level）。
//   2. 在 Claude Code / Cursor 等开发环境下 HTTP_PROXY=127.0.0.1:7890 会干扰本地
//      请求；reqwest 默认 no_proxy() 解析不支持 "127.*" 这种通配。
//      显式 .no_proxy() 强制直连本进程 spawn 的 axum。

use sisyphus_bridge::http::{build_router, RestApiState};

async fn setup_bridge_with_rest() -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let mut config = Config::default();
    config.cache.database_url = "sqlite::memory:".to_string();
    config.sandbox.wasm_tools_dir = "tests/wasm_tools".to_string();
    config.llm.embedding.allow_dummy_embeddings = true;
    let shared_config = Arc::new(RwLock::new(config));

    let bridge = Arc::new(
        MySisyphusBridge::new(shared_config, Some(Arc::new(DummyLlmClient)), None)
            .await
            .expect("Failed to create bridge"),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let data_dir = std::env::temp_dir();
    let rest_state = RestApiState::new(bridge.clone(), data_dir);
    let http_app = build_router(rest_state);

    let server = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, http_app).await {
            eprintln!("axum serve error: {e}");
        }
    });

    tokio::time::sleep(Duration::from_millis(500)).await;
    (addr, server)
}

#[tokio::test]
async fn health_live_always_ok() {
    let (addr, server) = setup_bridge_with_rest().await;
    let url = format!("http://{}/health/live", addr);
    // 关键: 在某些开发环境下 (如 Claude Code / Cursor) HTTP_PROXY 会被设置,
    // 且 reqwest 的默认 no_proxy 解析不支持 wildcard (e.g. "127.*")。
    // 显式 no_proxy() 强制 reqwest 不走系统代理, 直连本地 axum。
    let client = reqwest::Client::builder()
        .http1_only()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("build client");
    let resp = client.get(&url).send().await.expect("HTTP request failed");
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    eprintln!("[health_live] status={} body={}", status, text);
    assert_eq!(
        status, 200,
        "liveness must be 200, got {} body={}",
        status, text
    );
    let body: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["kind"], "liveness");
    server.abort();
}

#[tokio::test]
async fn health_ready_reports_components() {
    let (addr, server) = setup_bridge_with_rest().await;
    let url = format!("http://{}/health/ready", addr);
    let client = reqwest::Client::builder()
        .http1_only()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("build client");
    let resp = client.get(&url).send().await.expect("HTTP request failed");
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    eprintln!("[health_ready] status={} body={}", status, text);
    assert_eq!(
        status, 200,
        "readiness should be 200 when deps OK, got {} body={}",
        status, text
    );
    let body: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["kind"], "readiness");
    assert!(body["components"].is_object());
    server.abort();
}

// ── Prometheus 指标端点（阶段 8.2）─────────────────────────
//
// 端点 /api/metrics 由 axum 路由提供 (Prometheus 0.0.4 文本格式)。
// 验证：(1) 200 响应；(2) Content-Type 是 text/plain; version=0.0.4；
// (3) 关键指标名都在 (counters + gauges)；(4) 多线程安全（5 并发请求都 200）。

#[tokio::test]
async fn prometheus_metrics_exposes_standard_names() {
    let (addr, server) = setup_bridge_with_rest().await;
    let url = format!("http://{}/api/metrics", addr);
    let client = reqwest::Client::builder()
        .http1_only()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("build client");
    let resp = client.get(&url).send().await.expect("HTTP request failed");
    let status = resp.status();
    let ct = resp
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .map(|v| v.to_str().unwrap_or("").to_string())
        .unwrap_or_default();
    let text = resp.text().await.unwrap_or_default();
    eprintln!(
        "[metrics] status={} ct={} body_bytes={}",
        status,
        ct,
        text.len()
    );
    assert_eq!(status, 200, "metrics must be 200");
    assert!(
        ct.starts_with("text/plain"),
        "Content-Type must be Prometheus exposition, got {}",
        ct
    );
    assert!(
        ct.contains("version=0.0.4"),
        "missing Prometheus version, got {}",
        ct
    );

    // 关键指标必须出现 (counters + gauges)
    for name in [
        "sisyphus_http_requests_total",
        "sisyphus_http_errors_total",
        "sisyphus_http_in_flight",
        "sisyphus_http_shutdowns_total",
        "sisyphus_uptime_seconds",
        "sisyphus_sse_connections_active",
        "sisyphus_chaos_faults_total",
    ] {
        assert!(
            text.contains(name),
            "missing metric {} in body:\n{}",
            name,
            text
        );
    }

    // # HELP 和 # TYPE 行至少出现 1 次
    assert!(text.contains("# HELP"), "missing # HELP comments");
    assert!(text.contains("# TYPE"), "missing # TYPE comments");
    server.abort();
}

#[tokio::test]
async fn prometheus_metrics_concurrent_scrape_safe() {
    // 5 个并发 scrape 必须都返回 200；handler 内部读 shared_http + RwLock<u64>，
    // 验证并发读无 panic / 无 race。
    let (addr, server) = setup_bridge_with_rest().await;
    let url = format!("http://{}/api/metrics", addr);
    let mut handles = vec![];
    for _ in 0..5 {
        let url = url.clone();
        handles.push(tokio::spawn(async move {
            let client = reqwest::Client::builder()
                .http1_only()
                .no_proxy()
                .timeout(Duration::from_secs(5))
                .build()
                .expect("build client");
            let r = client.get(&url).send().await.expect("HTTP request failed");
            (r.status(), r.text().await.unwrap_or_default())
        }));
    }
    let mut ok = 0;
    for h in handles {
        let (status, body) = tokio::time::timeout(Duration::from_secs(5), h)
            .await
            .expect("metrics scrape timed out")
            .expect("scrape task panicked");
        assert_eq!(status, 200, "concurrent scrape got {}", status);
        assert!(body.contains("sisyphus_uptime_seconds"), "missing uptime");
        ok += 1;
    }
    assert_eq!(ok, 5, "all 5 concurrent scrapes must succeed");
    server.abort();
}

// ── tracing span 覆盖率（阶段 8.2）──────────────────────────
//
// 关键路径（execute_goal / execute_sop / token_budget 检查）必须 emit
// 命名的 OpenTelemetry-style span + 业务字段（sop_id / tool_id / project_id）。
//
// span 内容验证（subscriber 捕获）放在 sisyphus-intel 单元测试里 (tracing
// subscriber 不在 chaos_extended dev-dep 范围内，且 Layer 实现复杂)。
//
// 这里做路由可达性 sanity 检查：确认 #[instrument] 不破坏 axum 路由 + handler
// 仍能正常响应。

#[tokio::test]
async fn tracing_http_handlers_compile_with_instrument() {
    let (addr, server) = setup_bridge_with_rest().await;
    let client = reqwest::Client::builder()
        .http1_only()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("build client");
    // 触发带 #[instrument] 的 handler 路径（health_live 是健康度端点，
    // 不带 instrument；用 health_ready + /api/metrics 触发带 instrument 的路径）。
    // /api/metrics 经 metrics_handler (sse_handlers::metrics_handler)。
    let resp = client
        .get(format!("http://{}/api/metrics", addr))
        .send()
        .await
        .expect("HTTP request failed");
    assert_eq!(resp.status(), 200, "metrics must be 200");
    server.abort();
}

#[tokio::test]
async fn tracing_grpc_handlers_compile_with_instrument() {
    // 调用 execute_sop（带 #[instrument] gRPC handler），验证 tonic 调用链无 panic
    let (mut client, server) = setup_bridge().await;
    let req = ExecuteSopRequest {
        sop_id: "trace_sanity".to_string(),
        args: HashMap::new(),
    };
    let _r = client.execute_sop(req).await; // 不管结果
    server.abort();
}
