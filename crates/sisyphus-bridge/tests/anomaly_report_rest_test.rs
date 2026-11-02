//! REST `/api/anomaly` 数据链路测试。
//!
//! 回归背景：该端点通常依赖真实的认知循环运行来产生异常；这里通过直接写入
//! `StrategicManager.anomaly_log`（与 production 的 `spawn_trace_recorder`
//! 写入点是同一来源）来验证处理器的「取数 → 作用域过滤 → JSON 映射」链路，
//! 避免测试只是验证永远为空的数组。

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use sisyphus_bridge::bridge_service::MySisyphusBridge;
use sisyphus_bridge::http::{build_router, RestApiState};
use sisyphus_core::cognitive_loop_v2::{AnomalyLog, AnomalySeverity, NodeAnomaly};
use sisyphus_tools_llm::metis::DummyLlmClient;
use sisyphus_types::Config;
use std::sync::{Arc, RwLock};
use tower::ServiceExt;

async fn setup_with_log_fill() -> Router {
    let mut config = Config::default();
    config.cache.database_url = "sqlite::memory:".to_string();
    config.sandbox.wasm_tools_dir = "tests/wasm_tools".to_string();
    config.llm.embedding.allow_dummy_embeddings = true;
    let shared_config = Arc::new(RwLock::new(config));

    let bridge = MySisyphusBridge::new(shared_config, Some(Arc::new(DummyLlmClient)), None)
        .await
        .expect("Failed to create bridge");

    // 直接构造两条异常观测（与 spawn_trace_recorder 的写入路径一致）。
    let log: &AnomalyLog = &bridge.kernel().strategic_manager.anomaly_log;
    let mk = |name: &str, metric: &str, sev: AnomalySeverity| NodeAnomaly {
        node_name: name.into(),
        severity: sev,
        metric: metric.into(),
        observed: 42.0,
        expected: 10.0,
        description: format!("{name} {metric} deviation"),
    };
    log.record(&[mk("node_a", "fuel", AnomalySeverity::Medium)], "sop-live");
    log.record(
        &[mk("node_b", "duration", AnomalySeverity::High)],
        "sop-other",
    );

    build_router(RestApiState::new(Arc::new(bridge), std::env::temp_dir()))
}

async fn get(app: &Router, uri: &str) -> (StatusCode, axum::Json<serde_json::Value>) {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).expect("request build failed"))
        .await
        .expect("request failed");
    let status = response.status();
    let bytes =
        axum::body::to_bytes(response.into_body(), usize::MAX).await.expect("body read failed");
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, axum::Json(json))
}

#[tokio::test]
async fn anomaly_endpoint_scopes_reports_by_sop_id() {
    let app = setup_with_log_fill().await;

    // 只返回请求作用域内的异常。
    let (status, json) = get(&app, "/api/anomaly?sop_id=sop-live").await;
    assert_eq!(status, StatusCode::OK);
    let rows = json.as_array().expect("array body");
    assert_eq!(rows.len(), 1, "only sop-live anomalies: {json:?}");
    assert_eq!(rows[0]["node_name"], "node_a");
    assert_eq!(rows[0]["metric"], "fuel");
    assert_eq!(rows[0]["severity"], "medium");
    assert_eq!(rows[0]["observed"], 42.0);
    assert_eq!(rows[0]["expected"], 10.0);
}

#[tokio::test]
async fn anomaly_endpoint_unknown_scope_is_empty_not_error() {
    let app = setup_with_log_fill().await;

    let (status, json) = get(&app, "/api/anomaly?sop_id=missing").await;
    assert_eq!(status, StatusCode::OK);
    assert!(json.as_array().is_some_and(|rows| rows.is_empty()));
}

#[tokio::test]
async fn anomaly_endpoint_without_records_returns_empty_array() {
    // 干净 bridge（未写入任何异常）：200 + 空数组，而不是错误。
    let mut config = Config::default();
    config.cache.database_url = "sqlite::memory:".to_string();
    config.sandbox.wasm_tools_dir = "tests/wasm_tools".to_string();
    config.llm.embedding.allow_dummy_embeddings = true;
    let shared_config = Arc::new(RwLock::new(config));
    let bridge = MySisyphusBridge::new(shared_config, Some(Arc::new(DummyLlmClient)), None)
        .await
        .expect("Failed to create bridge");
    let app = build_router(RestApiState::new(Arc::new(bridge), std::env::temp_dir()));

    let (status, json) = get(&app, "/api/anomaly?sop_id=sop-nothing").await;
    assert_eq!(status, StatusCode::OK);
    assert!(json.as_array().is_some_and(|rows| rows.is_empty()));
}
