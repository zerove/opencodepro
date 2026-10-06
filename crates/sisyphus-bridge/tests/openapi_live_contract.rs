//! OpenAPI 文档的对外契约验收：真实起 axum HTTP 服务，用真实 HTTP 客户端
//! 拉取 /openapi.json，验证文档确实能被外部消费者使用。
//!
//! 为什么需要这一层：单元测试直接调用 `openapi_json().into_response()`，
//! 绕过了 axum 的序列化与中间件链路。真实服务曾经返回
//! `200 OK` + `content-length: 0`（空文档），而所有单元测试都是绿的。

use sisyphus_bridge::http::build_router;
use sisyphus_types::Config;
use std::sync::{Arc, RwLock};

/// 起一个真实的 axum 服务，绑定随机端口，返回 base url 与后台任务句柄。
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

    let data_dir = std::path::PathBuf::from("target/test-data-openapi");
    let state = sisyphus_bridge::http::RestApiState::new(bridge, data_dir);
    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("绑定失败");
    let addr = listener.local_addr().expect("取地址失败");
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("服务异常退出");
    });
    (format!("http://{addr}"), handle)
}

/// 真实 HTTP 拉取 /openapi.json，返回 (状态码, 响应体)。
async fn fetch_openapi(base: &str) -> (u16, String) {
    let client = reqwest::Client::new();
    let resp =
        client.get(format!("{base}/openapi.json")).send().await.expect("请求 /openapi.json 失败");
    let status = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();
    (status, body)
}

/// 端到端验收：真实服务必须返回可解析、内容非空的 OpenAPI 文档。
#[tokio::test]
async fn live_service_serves_non_empty_openapi_document() {
    let (base, server) = serve_http().await;

    let (status, body) = fetch_openapi(&base).await;

    assert_eq!(status, 200, "真实服务应返回 200");
    assert!(
        !body.trim().is_empty(),
        "真实服务返回了空文档（content-length: 0），外部消费者无法使用"
    );

    let doc: serde_json::Value = serde_json::from_str(&body).unwrap_or_else(|e| {
        panic!(
            "文档不是合法 JSON: {e}\n前 200 字节: {}",
            &body[..body.len().min(200)]
        )
    });

    assert_eq!(
        doc["openapi"].as_str().map(|s| s[..3].to_string()),
        Some("3.1".to_string())
    );
    let paths = doc["paths"].as_object().expect("文档缺少 paths 对象");
    assert!(
        paths.len() >= 60,
        "真实服务只返回了 {} 条路径，文档被截断",
        paths.len()
    );

    server.abort();
}

/// 本次补全的 21 条端点必须出现在真实服务返回的文档里。
#[tokio::test]
async fn live_service_documents_all_previously_missing_routes() {
    let (base, server) = serve_http().await;
    let (status, body) = fetch_openapi(&base).await;
    assert_eq!(status, 200);

    let doc: serde_json::Value = serde_json::from_str(&body).expect("文档不是合法 JSON");
    let paths = doc["paths"].as_object().expect("文档缺少 paths 对象");

    let expected = [
        ("/api/anomaly", "get"),
        ("/api/benchmarks", "get"),
        ("/api/breakers", "get"),
        ("/api/chaos/start", "post"),
        ("/api/chaos/status", "get"),
        ("/api/chaos/stop", "post"),
        ("/api/cognitive/map/{sop_id}/intervene", "post"),
        ("/api/cognitive/map/{sop_id}/stream", "get"),
        ("/api/experience/capture", "post"),
        ("/api/feedback", "post"),
        ("/api/insights", "get"),
        ("/api/jcode/call", "post"),
        ("/api/plugins", "get"),
        ("/api/plugins/load", "post"),
        ("/api/plugins/unload", "post"),
        ("/api/roadmap/bottlenecks", "post"),
        ("/api/roadmap/predict", "post"),
        ("/api/roadmap/project-costs", "post"),
        ("/api/skills/{id}", "get"),
        ("/api/tools/execute", "post"),
        ("/api/workspaces/{id}", "delete"),
    ];

    let mut missing = Vec::new();
    for (path, method) in expected {
        match paths.get(path).and_then(|v| v.get(method)) {
            Some(_) => {},
            None => missing.push(format!("{method} {path}")),
        }
    }
    assert!(
        missing.is_empty(),
        "真实服务返回的文档缺少这些端点: {missing:?}"
    );

    server.abort();
}

/// 真实返回的文档里，所有 $ref 都必须能在 components.schemas 中解析。
#[tokio::test]
async fn live_service_document_has_no_dangling_refs() {
    let (base, server) = serve_http().await;
    let (status, body) = fetch_openapi(&base).await;
    assert_eq!(status, 200);

    let doc: serde_json::Value = serde_json::from_str(&body).expect("文档不是合法 JSON");
    let schemas = doc["components"]["schemas"].as_object().expect("缺少 components.schemas");

    let mut dangling = Vec::new();
    fn walk(
        v: &serde_json::Value,
        schemas: &serde_json::Map<String, serde_json::Value>,
        out: &mut Vec<String>,
    ) {
        match v {
            serde_json::Value::Object(map) => {
                for (k, val) in map {
                    if k == "$ref" {
                        if let Some(r) = val.as_str() {
                            if let Some(name) = r.strip_prefix("#/components/schemas/") {
                                if !schemas.contains_key(name) {
                                    out.push(r.to_string());
                                }
                            }
                        }
                    } else {
                        walk(val, schemas, out);
                    }
                }
            },
            serde_json::Value::Array(arr) => {
                for item in arr {
                    walk(item, schemas, out);
                }
            },
            _ => {},
        }
    }
    walk(&doc, schemas, &mut dangling);

    assert!(dangling.is_empty(), "真实文档存在悬空 $ref: {dangling:?}");
    server.abort();
}
