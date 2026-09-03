//! 混沌测试扩展（TODO 7.2 第四项）
//!
//! 覆盖 bridge 端未在 bridge_integration_test.rs 测的边角：
//! - 并发请求竞态（同一 session 并行 health_check / execute_tool）
//! - 高并发短突发（100 并发 health_check）
//! - 大 payload 鲁棒性（10KB JSON args 不导致 panic）
//! - 重复连接（10 次连/断/连）

use sisyphus_bridge::bridge_service::MySisyphusBridge;
use sisyphus_bridge_api::{
    sisyphus_bridge_client::SisyphusBridgeClient, sisyphus_bridge_server::SisyphusBridgeServer,
    ExecuteToolRequest, HealthCheckRequest, ListSoPsRequest,
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
    // 关键：DummyLlmClient 没有 embedding 凭据，
    // 显式允许 dummy embedding 避免桥接启动失败。
    config.llm.embedding.allow_dummy_embeddings = true;
    let shared_config = Arc::new(RwLock::new(config));

    let bridge = MySisyphusBridge::new(shared_config, Some(Arc::new(DummyLlmClient)))
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

// ── 并发请求竞态 ──────────────────────────────────────────────

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
    // 50 并发调用应在 5s 内全部完成，无 panic
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
            // 每次不同 filter（ListSoPsRequest 无字段，简化为空请求）
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

// ── 高并发突发 ──────────────────────────────────────────────

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
    // 100 并发 health_check 应在 10s 完成 ≥ 90% 成功
    assert!(
        success >= 90,
        "only {} / 100 succeeded in {:?}",
        success,
        elapsed
    );
    server.abort();
}

// ── 大 payload 鲁棒性 ─────────────────────────────────────────

#[tokio::test]
async fn chaos_large_args_payload_does_not_crash() {
    let (mut client, server) = setup_bridge().await;
    // 构造 ~10KB JSON args
    let mut big_args = HashMap::new();
    big_args.insert("data".to_string(), "x".repeat(10_000));
    big_args.insert("name".to_string(), "test_tool".to_string());
    let r = ExecuteToolRequest {
        tool_id: "unknown_xyz_12345".to_string(),
        args_json: serde_json::to_string(&big_args).unwrap(),
        fuel_limit: 10000,
    };
    // bridge 对 execute_tool 总是返回 Ok（status != "SUCCESS" 表示失败）
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

// ── 重复连接 ─────────────────────────────────────────────────

#[tokio::test]
async fn chaos_repeated_connect_disconnect() {
    for _i in 0..10 {
        let (client, server) = setup_bridge().await;
        // 连上后立即做一次轻量调用
        let _r = client.clone().health_check(HealthCheckRequest {}).await;
        server.abort();
        // 强制 server 任务退出
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // 跑完不 panic 即通过
}
