//! SSE 端点的对外契约验收：真实起 axum 服务，用真实 HTTP 客户端建立 SSE 连接，
//! 再通过真实的 `POST /api/events/broadcast` 端点投递事件，观察订阅端实际收到什么。
//!
//! 为什么需要这一层：单元测试直接调用内部函数并注入 broadcast channel，
//! 绕过了 HTTP 路由、查询参数解析、跨请求的广播通道共享这些真实环节。
//!
//! 注意：客户端一律用 `.no_proxy()` 构造。这些用例起的是 **127.0.0.1** 上的
//! 真实服务，而 `reqwest::Client::new()` 会读取系统代理环境变量
//! （`HTTP_PROXY` / `http_proxy` 等）。开发机上设了
//! `HTTP_PROXY=http://127.0.0.1:7890` 时，本机请求会被转发给代理，
//! 代理无法访问测试进程自己的临时端口，于是**每一页**都拿到 HTTP 502，
//! 3 条用例全红（实测：开代理 0 passed/3 failed，清掉代理 3 passed）。
//! 这类失败与被测代码无关，但会把真实回归淹没在噪声里。

use futures_util::StreamExt;
use sisyphus_bridge::http::build_router;
use sisyphus_types::Config;
use std::sync::{Arc, RwLock};
use std::time::Duration;

async fn serve_http() -> (String, tokio::task::JoinHandle<()>) {
    let mut config = Config::default();
    config.cache.database_url = "sqlite::memory:".to_string();
    config.llm.embedding.allow_dummy_embeddings = true;
    config.llm.embedding.provider = "dummy".into();
    let shared_config = Arc::new(RwLock::new(config));

    let bridge = Arc::new(
        sisyphus_bridge::bridge_service::MySisyphusBridge::new(shared_config, None, None)
            .await
            .expect("bridge 构造失败"),
    );

    let data_dir = std::path::PathBuf::from("target/test-data-sse");
    let state = sisyphus_bridge::http::RestApiState::new(bridge, data_dir);
    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("绑定失败");
    let addr = listener.local_addr().expect("取地址失败");
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}"), handle)
}

/// 从 SSE 流中读取下一个 `data:` 行的内容，超时则返回 None。
async fn next_sse_data(
    stream: &mut (impl futures_util::Stream<Item = reqwest::Result<String>> + Unpin),
) -> Option<String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout_at(deadline, stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                for line in chunk.lines() {
                    if let Some(data) = line.strip_prefix("data:") {
                        let trimmed = data.trim();
                        if !trimmed.is_empty() {
                            return Some(trimmed.to_string());
                        }
                    }
                }
            },
            Ok(Some(Err(_))) | Ok(None) => return None,
            Err(_) => return None,
        }
    }
    None
}

/// 端到端验收：真实 SSE 订阅者必须收到真实广播端点投递的事件。
#[tokio::test]
async fn live_sse_subscriber_receives_broadcast_event() {
    let (base, server) = serve_http().await;
    let client = reqwest::Client::builder().no_proxy().build().expect("构造 HTTP 客户端失败");

    // 建立真实 SSE 连接
    let resp =
        client.get(format!("{base}/api/events?heartbeat=0")).send().await.expect("SSE 连接失败");
    assert_eq!(resp.status().as_u16(), 200);
    let mut stream =
        resp.bytes_stream().map(|r| r.map(|b| String::from_utf8_lossy(&b).to_string()));

    // 给订阅者一点时间注册到广播通道
    tokio::time::sleep(Duration::from_millis(400)).await;

    // 通过真实 HTTP 端点投递事件
    let body = serde_json::json!({
        "event_type": "custom_test_event",
        "payload": {"hello": "world"},
    });
    let post = client
        .post(format!("{base}/api/events/broadcast"))
        .json(&body)
        .send()
        .await
        .expect("广播请求失败");
    assert!(post.status().is_success(), "广播端点返回 {}", post.status());

    let received = next_sse_data(&mut stream).await;
    assert!(
        received.is_some(),
        "真实 SSE 订阅者在 8 秒内没有收到任何事件"
    );
    let payload: serde_json::Value =
        serde_json::from_str(received.as_ref().unwrap()).expect("事件负载不是合法 JSON");
    assert_eq!(
        payload["event_type"], "custom_test_event",
        "收到的不是我们广播的那个事件"
    );
    assert_eq!(payload["payload"]["hello"], "world");

    server.abort();
}

/// 端到端验收：`?project_id=` 过滤必须真正生效，跨项目订阅者不得收到事件。
#[tokio::test]
async fn live_sse_project_id_filter_isolates_subscribers() {
    let (base, server) = serve_http().await;
    let client = reqwest::Client::builder().no_proxy().build().expect("构造 HTTP 客户端失败");

    // 订阅者只关心 project-a
    let resp = client
        .get(format!(
            "{base}/api/events?project_id=project-a&heartbeat=0"
        ))
        .send()
        .await
        .expect("SSE 连接失败");
    assert_eq!(resp.status().as_u16(), 200);
    let mut stream =
        resp.bytes_stream().map(|r| r.map(|b| String::from_utf8_lossy(&b).to_string()));
    tokio::time::sleep(Duration::from_millis(400)).await;

    // 广播一个属于 project-b 的事件 —— project-a 订阅者不应收到
    let body = serde_json::json!({
        "event_type": "other_project_event",
        "payload": {"secret": "should_not_leak"},
        "project_id": "project-b",
    });
    let post = client
        .post(format!("{base}/api/events/broadcast"))
        .json(&body)
        .send()
        .await
        .expect("广播请求失败");
    assert!(post.status().is_success(), "广播返回 {}", post.status());

    let leaked = next_sse_data(&mut stream).await;
    assert!(
        leaked.is_none(),
        "project-a 的订阅者收到了 project-b 的事件，项目隔离失效: {leaked:?}"
    );

    server.abort();
}

/// 端到端验收：同项目订阅者必须能收到自己项目的事件（过滤不能过度拦截）。
#[tokio::test]
async fn live_sse_same_project_event_is_delivered() {
    let (base, server) = serve_http().await;
    let client = reqwest::Client::builder().no_proxy().build().expect("构造 HTTP 客户端失败");

    let resp = client
        .get(format!(
            "{base}/api/events?project_id=project-a&heartbeat=0"
        ))
        .send()
        .await
        .expect("SSE 连接失败");
    let mut stream =
        resp.bytes_stream().map(|r| r.map(|b| String::from_utf8_lossy(&b).to_string()));
    tokio::time::sleep(Duration::from_millis(400)).await;

    let body = serde_json::json!({
        "event_type": "own_project_event",
        "payload": {"ok": true},
        "project_id": "project-a",
    });
    let status = client
        .post(format!("{base}/api/events/broadcast"))
        .json(&body)
        .send()
        .await
        .expect("广播请求失败")
        .status();
    assert!(status.is_success(), "广播返回 {status}");

    let received = next_sse_data(&mut stream).await;
    assert!(
        received.is_some(),
        "project-a 订阅者没有收到 project-a 自己的事件，过滤过度拦截了"
    );

    server.abort();
}
