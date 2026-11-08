use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use sisyphus_bridge::bridge_service::MySisyphusBridge;
use sisyphus_bridge::http::{build_router, RestApiState};
use sisyphus_types::Config;
use std::sync::{Arc, RwLock};
use tower::ServiceExt;

async fn setup() -> (Router, Arc<MySisyphusBridge>) {
    let mut config = Config::default();
    config.cache.database_url = "sqlite::memory:".to_string();
    config.sandbox.wasm_tools_dir = "tests/wasm_tools".to_string();
    config.llm.embedding.allow_dummy_embeddings = true;
    let shared_config = Arc::new(RwLock::new(config));

    let bridge =
        Arc::new(MySisyphusBridge::new(shared_config, None, None).await.expect("bridge 构造失败"));

    let app = build_router(RestApiState::new(bridge.clone(), std::env::temp_dir()));
    (app, bridge)
}

async fn get(app: &Router, uri: &str) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).expect("request build failed"))
        .await
        .expect("request failed");
    let status = response.status();
    let bytes =
        axum::body::to_bytes(response.into_body(), usize::MAX).await.expect("body read failed");
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

/// 单节点/集群未启用：端点必须返回 200 + 只读快照（不因无 peer manager 报错）。
#[tokio::test]
async fn cluster_status_endpoint_reports_single_node_view() {
    let (app, _bridge) = setup().await;

    let (status, json) = get(&app, "/api/cluster/status").await;
    assert_eq!(status, StatusCode::OK, "{json:?}");
    assert_eq!(json["enabled"], false);
    assert_eq!(json["config"]["control_plane_enabled"], false);
    assert!(
        json["cluster_name"].is_string(),
        "cluster_name 应为字符串: {json:?}"
    );
    assert_eq!(json["peer_manager"]["cluster_size"], 1);
    assert_eq!(json["peer_manager"]["is_leader"], false);
    assert_eq!(json["peer_manager"]["state_proposal_broadcasts"], 0);
    // Phase 6.2b：经验同步只读计数（单节点初始为 0）
    let exp = &json["peer_manager"]["experience_sync"];
    assert_eq!(exp["broadcasts"], 0, "{json:?}");
    assert_eq!(exp["inbound_delivered"], 0, "{json:?}");
    assert_eq!(exp["inbound_applied"], 0, "{json:?}");
    assert_eq!(exp["inbound_rejected"], 0, "{json:?}");
    assert_eq!(exp["inbound_rejected_deserialize"], 0, "{json:?}");
    assert_eq!(exp["inbound_rejected_quality"], 0, "{json:?}");
    assert_eq!(exp["inbound_rejected_apply"], 0, "{json:?}");
    assert!(json["peers"].is_array(), "peers 应为数组: {json:?}");
    assert!(json["peers"].as_array().expect("array").is_empty());
    // consensus trait view 始终存在（桥接持有 Arc<dyn Consensus>）
    assert!(json["consensus"]["leader_id"].is_string() || json["consensus"]["leader_id"].is_null());
}

/// ADR-019 只读 REST：门禁默认关闭时返回 DISABLED，不改变状态。
#[tokio::test]
async fn cluster_control_lease_get_read_only_gate_default() {
    let (app, _bridge) = setup().await;
    let (status, json) = get(&app, "/api/cluster/control/leases/lease%3Atest").await;
    assert_eq!(status, StatusCode::OK, "body: {json}");
    assert_eq!(json["found"], false);
    assert_eq!(json["status_code"], "DISABLED");
}
