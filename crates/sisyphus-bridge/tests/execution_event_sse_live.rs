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
    let client = reqwest::Client::builder().no_proxy().build().expect("构造 HTTP 客户端失败");

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
    let client = reqwest::Client::builder().no_proxy().build().expect("构造 HTTP 客户端失败");

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
    let client = reqwest::Client::builder().no_proxy().build().expect("构造 HTTP 客户端失败");

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

/// 收集 SSE 流的完整事件载荷（含 event_type 字段），直到超时。
async fn collect_payloads(
    stream: &mut (impl futures_util::Stream<Item = reqwest::Result<String>> + Unpin),
    max_events: usize,
    window: Duration,
) -> Vec<serde_json::Value> {
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
                    if let Some(data) = line.strip_prefix("data:") {
                        let t = data.trim();
                        if t.is_empty() {
                            continue;
                        }
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(t) {
                            out.push(v);
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

/// 真实执行后，事件类型必须是内核变体的 snake_case 名，而不是 Rust 变体名。
///
/// `execution_event_type` 把 8 个 `ExecutionEvent` 变体映射为
/// node_started / cognitive_step / micro_reflection 等 snake_case。
/// 客户端按这些名字做分支，所以经真实 HTTP 路径拿到的名字必须正确。
#[tokio::test]
async fn live_execution_event_names_are_snake_case_kernel_variants() {
    let (base, server) = serve_http().await;
    let client = reqwest::Client::builder().no_proxy().build().expect("构造 HTTP 客户端失败");

    let resp =
        client.get(format!("{base}/api/events?heartbeat=0")).send().await.expect("SSE 连接失败");
    let mut stream =
        resp.bytes_stream().map(|r| r.map(|b| String::from_utf8_lossy(&b).to_string()));
    tokio::time::sleep(Duration::from_millis(400)).await;

    let body = serde_json::json!({
        "goal": "naming check",
        "sop_id": "live-exec-naming",
        "event_tx": true,
    });
    let _ = client
        .post(format!("{base}/api/execute/goal"))
        .json(&body)
        .send()
        .await
        .expect("执行请求失败");

    let payloads = collect_payloads(&mut stream, 10, Duration::from_secs(10)).await;
    assert!(
        !payloads.is_empty(),
        "真实执行后没有收到任何事件，无法校验事件名"
    );

    // 允许的 8 个 snake_case 名，取自 execution_event_type 的映射表
    const EXPECTED: [&str; 8] = [
        "node_started",
        "node_completed",
        "node_failed",
        "execution_complete",
        "cognitive_plan",
        "cognitive_step",
        "micro_reflection",
        "cognitive_complete",
    ];

    let mut bad = Vec::new();
    for p in &payloads {
        let ty = p.get("event_type").and_then(|v| v.as_str()).unwrap_or("");
        if !EXPECTED.contains(&ty) {
            bad.push(ty.to_string());
        }
    }
    assert!(
        bad.is_empty(),
        "收到非 snake_case 或未知的事件名 {bad:?}，实际载荷: {payloads:?}"
    );

    // 每个事件都必须带 sop_id，否则客户端无法按 SOP 过滤。
    // 注意 wire 格式：sop_id 位于 payload 内层（Custom 变体的 payload 里），
    // 这与 SseEvent::sop_id() 的读取路径一致。
    for p in &payloads {
        assert!(
            p.pointer("/payload/sop_id").and_then(|v| v.as_str()).is_some(),
            "事件 payload 缺少 sop_id 维度，客户端无法过滤: {p:?}"
        );
    }

    server.abort();
}

/// composite 路径的完整客户端闭环：用响应里的 `sse_sop_id` 订阅事件。
///
/// `/api/execute/composite` 内部生成 `composite-<uuid>` 作为事件归属，
/// 并在响应中回传 `sse_sop_id` —— 客户端必须靠它才知道订阅哪个 sop_id。
/// 这是与 execute_goal 完全独立的代码路径（不同 handler、独立 sop_id 生成）。
///
/// 覆盖两个曾经同时存在的缺陷：
/// 1. handler 为每个 goal 塞 `SOPGraphConfig::default()`（空图），被
///    `SOPGraph::from_config` 拒绝 → 端点恒返回 500。
/// 2. 内核 `execute_goals` 硬编码 `None` 事件槽 → 即便执行成功，SSE 也全静默。
#[tokio::test]
async fn live_composite_execution_exposes_sse_sop_id_for_subscription() {
    let (base, server) = serve_http().await;
    let client = reqwest::Client::builder().no_proxy().build().expect("构造 HTTP 客户端失败");

    // 先建立不带 sop_id 过滤的订阅：composite 的 sop_id 由服务端生成，
    // 客户端事前无法得知，只能事后关联。
    let resp =
        client.get(format!("{base}/api/events?heartbeat=0")).send().await.expect("SSE 连接失败");
    assert_eq!(resp.status().as_u16(), 200, "SSE 订阅应成功");
    let mut stream =
        resp.bytes_stream().map(|r| r.map(|b| String::from_utf8_lossy(&b).to_string()));

    let body = serde_json::json!({
        "goals": ["first goal", "second goal"],
        "event_tx": true,
    });
    let post = client
        .post(format!("{base}/api/execute/composite"))
        .json(&body)
        .send()
        .await
        .expect("composite 请求失败");
    let status = post.status().as_u16();
    let resp_json: serde_json::Value = post.json().await.unwrap_or(serde_json::Value::Null);

    // 端点可用性：此前为每个 goal 塞空图，被 `SOPGraph::from_config`
    // 以 "Graph config must contain at least one node" 拒绝 → 恒 500。
    // 空图缺陷修好后请求会真正进入内核，因此错误（若有）必须来自执行
    // 本身，而不能再是构图失败。
    if status != 200 {
        let err = resp_json.get("error").and_then(|v| v.as_str()).unwrap_or_default();
        assert!(
            !err.contains("at least one node"),
            "composite 仍因空图构图失败而不可用（缺陷未修复）: {resp_json}"
        );
    }

    // 核心闭环：执行期间的事件必须真的流到 SSE（此前内核 `execute_goals`
    // 硬编码 `None` 事件槽，即便端点修好也会全静默）。
    let payloads = collect_payloads(&mut stream, 8, Duration::from_secs(8)).await;
    assert!(
        !payloads.is_empty(),
        "composite 执行期间 SSE 未收到任何事件（内核事件槽未透传）"
    );

    // 每个事件都必须带 composite- 前缀的 sop_id：handler 用它作为转发打标，
    // 客户端据此用 `/api/events?sop_id=...` 定向订阅。转发器用外部 sop_id
    // 覆盖内核事件自带的 `goal_N`，这里验证覆盖确实生效。
    for p in &payloads {
        let event_sop_id = p
            .pointer("/payload/sop_id")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("事件缺少 payload.sop_id，客户端无法定向订阅: {p}"));
        assert!(
            event_sop_id.starts_with("composite-"),
            "事件 sop_id 应被 handler 的 composite- 前缀覆盖（内核自带的 goal_N 客户端无法预知）: {p}"
        );
        assert!(
            p.get("event_type").and_then(|v| v.as_str()).is_some(),
            "事件缺少 event_type: {p}"
        );
    }

    // **无条件**断言 `sse_sop_id` 存在且与事件打标一致。
    //
    // 这里刻意不按 status 分支：DummyLlm 下内核几乎总以 StrategicPivot
    // 收场（HTTP 500），若把断言放进 `if status == 200`，整段就是死代码 ——
    // 实测确实从未执行过，也就从未验证过契约。composite 的 sop_id 由服务端
    // 生成，客户端无从推算，所以**无论执行成败都必须回传**：此刻事件已经
    // 广播出去了，客户端拿不到 id 就等于收到了却无法订阅。
    let sse_sop_id = resp_json.get("sse_sop_id").and_then(|v| v.as_str()).unwrap_or_else(|| {
        panic!(
            "composite 响应缺少 sse_sop_id（status={status}）。事件已广播，\
             客户端却无从得知该用哪个 sop_id 订阅: {resp_json}"
        )
    });
    assert!(
        sse_sop_id.starts_with("composite-"),
        "sse_sop_id 应带 composite- 前缀，实际: {sse_sop_id}"
    );
    for p in &payloads {
        assert_eq!(
            p.pointer("/payload/sop_id").and_then(|v| v.as_str()),
            Some(sse_sop_id),
            "事件 sop_id 与响应回传的 sse_sop_id 不一致，客户端按它过滤会收不到: {p}"
        );
    }

    // 拿到 id 去订阅必须能连上 —— 这是客户端拿到 id 后的第一步动作。
    let sub = client
        .get(format!("{base}/api/events?sop_id={sse_sop_id}&heartbeat=0"))
        .send()
        .await
        .expect("按 sse_sop_id 订阅失败");
    assert_eq!(
        sub.status().as_u16(),
        200,
        "按 composite 返回的 sse_sop_id 订阅应成功"
    );

    // 成功时还要带上真实执行结果，而非空壳。
    if status == 200 {
        let results = resp_json.get("results").and_then(|v| v.as_object()).expect("缺少 results");
        assert!(
            !results.is_empty(),
            "composite 成功响应却没有任何执行结果: {resp_json}"
        );
    }

    server.abort();
}
