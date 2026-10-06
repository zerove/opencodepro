//! ExecutionEvent → SSE 转发的对外契约验收：真实起 axum 服务，建立真实 SSE
//! 连接，然后通过真实的 `POST /api/execute/goal` 触发一次真实内核执行，
//! 观察订阅端实际收到哪些执行事件。
//!
//! 为什么需要这一层：`1e70363` 修复的缺陷是「只取 sender 丢弃 receiver，
//! 内核 send 立即返回 Err，事件被静默吞掉」。单元测试通过直接调用
//! `wire_execution_events` 并手工注入 channel 来验证，这绕过了
//! 「handler 是否真的调用它」以及「内核是否真的产生了事件」两件事。

use futures_util::StreamExt;
use sisyphus_bridge::http::build_router;
use sisyphus_tools_llm::metis::DummyLlmClient;
use sisyphus_types::Config;
use std::sync::{Arc, RwLock};
use std::time::Duration;

async fn serve_http() -> (String, tokio::task::JoinHandle<()>) {
    let mut config = Config::default();
    config.cache.database_url = "sqlite::memory:".to_string();
    config.llm.embedding.allow_dummy_embeddings = true;
    config.llm.embedding.provider = "dummy".into();
    config.sandbox.wasm_tools_dir = "tests/wasm_tools".to_string();
    let shared_config = Arc::new(RwLock::new(config));

    let bridge = Arc::new(
        sisyphus_bridge::bridge_service::MySisyphusBridge::new(
            shared_config,
            Some(Arc::new(DummyLlmClient)),
            None,
        )
        .await
        .expect("bridge 构造失败"),
    );

    let data_dir = std::path::PathBuf::from("target/test-data-exec");
    let state = sisyphus_bridge::http::RestApiState::new(bridge, data_dir);
    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("绑定失败");
    let addr = listener.local_addr().expect("取地址失败");
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}"), handle)
}

/// 收集 SSE 流中的事件类型，直到超时或达到 max_events 个。
async fn collect_event_types(
    stream: &mut (impl futures_util::Stream<Item = reqwest::Result<String>> + Unpin),
    max_events: usize,
    window: Duration,
) -> Vec<String> {
    let deadline = tokio::time::Instant::now() + window;
    let mut out = Vec::new();
    let mut buf = String::new();
    while tokio::time::Instant::now() < deadline && out.len() < max_events {
        match tokio::time::timeout_at(deadline, stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                buf.push_str(&chunk);
                while let Some(idx) = buf.find('\n') {
                    let line: String = buf.drain(..=idx).collect();
                    let line = line.trim();
                    if line.is_empty() || line.starts_with(':') || line.starts_with("event:") {
                        continue;
                    }
                    if let Some(data) = line.strip_prefix("data:") {
                        let t = data.trim();
                        if t.is_empty() {
                            continue;
                        }
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(t) {
                            let ty = v
                                .get("type")
                                .and_then(|x| x.as_str())
                                .unwrap_or("unknown")
                                .to_string();
                            out.push(ty);
                        }
                    }
                }
            },
            Ok(Some(Err(_))) | Ok(None) => break,
            Err(_) => break,
        }
    }
    out
}

/// 端到端验收：真实执行一个 goal，SSE 订阅者必须收到内核产生的执行事件。
///
/// 这是 `1e70363` 修复的核心断言：修复前事件被静默吞掉，客户端
/// 即使传了 `event_tx: true` 也收不到任何东西。
#[tokio::test]
async fn live_execution_emits_events_to_sse_subscribers() {
    let (base, server) = serve_http().await;
    let client = reqwest::Client::new();

    // 建立真实 SSE 连接，不加任何过滤，接收全部事件
    let resp =
        client.get(format!("{base}/api/events?heartbeat=0")).send().await.expect("SSE 连接失败");
    assert_eq!(resp.status().as_u16(), 200);
    let mut stream =
        resp.bytes_stream().map(|r| r.map(|b| String::from_utf8_lossy(&b).to_string()));
    tokio::time::sleep(Duration::from_millis(400)).await;

    // 通过真实 HTTP 端点触发一次真实内核执行，显式要求事件转发
    let body = serde_json::json!({
        "goal": "say hello",
        "sop_id": "live-exec-sop",
        "event_tx": true,
    });
    let post = client
        .post(format!("{base}/api/execute/goal"))
        .json(&body)
        .send()
        .await
        .expect("执行请求失败");
    let status = post.status().as_u16();
    let resp_body: serde_json::Value = post.json().await.unwrap_or(serde_json::Value::Null);

    // 执行本身可能失败（DummyLlm 下认知循环常以 StrategicPivot 收场并返回 500），
    // 这不影响事件是否流出 —— 失败路径同样会产生 NodeFailed / CognitiveComplete 等
    // 事件。我们验证的是「订阅端收到了什么」，而不是「执行成功与否」，
    // 因此不把 500 当作失败。500 本身在此证明执行确实跑起来了。
    assert!(
        status == 200 || status == 500,
        "执行端点返回了意外状态 {status}: {resp_body}"
    );

    let types = collect_event_types(&mut stream, 8, Duration::from_secs(10)).await;

    assert!(
        !types.is_empty(),
        "真实执行后 SSE 订阅者没有收到任何执行事件（status={status}, body={resp_body}）。\
         这正是 1e70363 修复的缺陷模式：事件被静默吞掉。"
    );
    // 执行事件应映射为 Custom 事件（wire 走 SseEvent::from_execution_event）
    assert!(
        types.iter().any(|t| t == "custom"),
        "收到的执行事件类型不符合预期（应包含 custom），实际: {types:?}"
    );

    server.abort();
}

/// `event_tx: false` 是调用方明确不要事件，订阅端不应收到执行事件。
#[tokio::test]
async fn live_execution_respects_event_tx_false() {
    let (base, server) = serve_http().await;
    let client = reqwest::Client::new();

    let resp =
        client.get(format!("{base}/api/events?heartbeat=0")).send().await.expect("SSE 连接失败");
    let mut stream =
        resp.bytes_stream().map(|r| r.map(|b| String::from_utf8_lossy(&b).to_string()));
    tokio::time::sleep(Duration::from_millis(400)).await;

    let body = serde_json::json!({
        "goal": "quiet run",
        "sop_id": "live-exec-quiet",
        "event_tx": false,
    });
    let _ = client
        .post(format!("{base}/api/execute/goal"))
        .json(&body)
        .send()
        .await
        .expect("执行请求失败");

    let types = collect_event_types(&mut stream, 8, Duration::from_secs(6)).await;
    assert!(
        types.is_empty(),
        "event_tx=false 时仍收到执行事件，禁用语义失效: {types:?}"
    );

    server.abort();
}

/// `?sop_id=` 过滤必须作用于执行事件：只订阅 A 就收不到 B 的执行事件。
#[tokio::test]
async fn live_execution_events_respect_sop_id_filter() {
    let (base, server) = serve_http().await;
    let client = reqwest::Client::new();

    // 只订阅 sop-watch
    let resp = client
        .get(format!(
            "{base}/api/events?sop_id=live-sop-watch&heartbeat=0"
        ))
        .send()
        .await
        .expect("SSE 连接失败");
    let mut stream =
        resp.bytes_stream().map(|r| r.map(|b| String::from_utf8_lossy(&b).to_string()));
    tokio::time::sleep(Duration::from_millis(400)).await;

    // 执行另一个 sop
    let body = serde_json::json!({
        "goal": "other sop",
        "sop_id": "live-sop-other",
        "event_tx": true,
    });
    let _ = client
        .post(format!("{base}/api/execute/goal"))
        .json(&body)
        .send()
        .await
        .expect("执行请求失败");

    let types = collect_event_types(&mut stream, 8, Duration::from_secs(6)).await;
    assert!(
        types.is_empty(),
        "订阅 live-sop-watch 却收到了 live-sop-other 的执行事件: {types:?}"
    );

    server.abort();
}
