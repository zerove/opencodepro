//! REST API tests for the distributed skill market and experience sharing.
//!
//! Exercises the peer registry protocol (this instance serving skills to
//! other Sisyphus instances), the publish/search market endpoints, and the
//! multi-workspace experience sharing endpoints.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use sisyphus_bridge::bridge_service::MySisyphusBridge;
use sisyphus_bridge::http::{build_router, RestApiState};
use sisyphus_tools_llm::metis::DummyLlmClient;
use sisyphus_types::skill::{SkillBundle, SkillMetadata};
use sisyphus_types::Config;
use std::sync::{Arc, RwLock};
use tower::ServiceExt;

fn test_bundle(id: &str) -> SkillBundle {
    SkillBundle {
        metadata: SkillMetadata {
            id: id.to_string(),
            name: format!("skill-{id}"),
            version: "1.0.0".to_string(),
            author: "test".to_string(),
            description: "test skill".to_string(),
            tags: vec!["test".to_string()],
            created_at: 1,
        },
        main_sop: sisyphus_types::graph::SOPGraphConfig {
            nodes: vec![],
            edges: vec![],
        },
        sub_sops: Default::default(),
        required_tools: vec![],
        acceptance_criteria: String::new(),
    }
}

async fn setup() -> Router {
    let mut config = Config::default();
    config.cache.database_url = "sqlite::memory:".to_string();
    config.sandbox.wasm_tools_dir = "tests/wasm_tools".to_string();
    let shared_config = Arc::new(RwLock::new(config));

    let bridge = MySisyphusBridge::new(shared_config, Some(Arc::new(DummyLlmClient)))
        .await
        .expect("Failed to create bridge");
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

async fn post_json(
    app: &Router,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, axum::Json<serde_json::Value>) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .expect("request build failed"),
        )
        .await
        .expect("request failed");
    let status = response.status();
    let bytes =
        axum::body::to_bytes(response.into_body(), usize::MAX).await.expect("body read failed");
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, axum::Json(json))
}

// ── Peer registry protocol ───────────────────────────────────────────────

#[tokio::test]
async fn test_peer_list_skills_empty() {
    let app = setup().await;
    let (status, _) = get(&app, "/peer/v1/skills").await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn test_peer_publish_and_fetch_roundtrip() {
    let app = setup().await;
    let bundle = test_bundle("peer-test-1");

    // Publish a bundle to this instance (as if from a peer).
    let (status, _) = post_json(
        &app,
        "/peer/v1/skills",
        serde_json::to_value(&bundle).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "peer publish failed");

    // The skill is now listed.
    let (status, json) = get(&app, "/peer/v1/skills").await;
    assert_eq!(status, StatusCode::OK);
    let skills = json["skills"].as_array().expect("skills array");
    assert!(
        skills.iter().any(|s| s["id"] == "peer-test-1"),
        "published skill missing from peer list"
    );

    // Fetch the full bundle back.
    let (status, json) = get(&app, "/peer/v1/skills/peer-test-1").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["metadata"]["id"], "peer-test-1");

    // Unknown skill -> 404.
    let (status, _) = get(&app, "/peer/v1/skills/nope").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ── Skill market (publish / search) ──────────────────────────────────────

#[tokio::test]
async fn test_publish_missing_skill_returns_404() {
    let app = setup().await;
    let (status, json) = post_json(
        &app,
        "/api/skills/publish",
        serde_json::json!({
            "registry_url": "http://127.0.0.1:1",
            "skill_id": "not-installed"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json["success"], false);
}

#[tokio::test]
async fn test_search_skills_unreachable_registry_returns_empty() {
    // The default registry is unreachable; search must degrade to empty
    // results instead of failing hard.
    let app = setup().await;
    let (status, json) = get(&app, "/api/skills/search?q=foo").await;
    assert_eq!(status, StatusCode::OK);
    assert!(json["results"].is_array());
}

// ── Experience sharing (multi-workspace knowledge transfer) ──────────────

#[tokio::test]
async fn test_share_experience_flow() {
    let app = setup().await;

    // Sharing a missing experience must error.
    let (status, json) = post_json(
        &app,
        "/api/experience/share",
        serde_json::json!({ "exp_key": "exp:missing" }),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(json["success"], false);

    // Shared listing is available and empty.
    let (status, json) = get(&app, "/api/experience/shared").await;
    assert_eq!(status, StatusCode::OK);
    assert!(json["experiences"].is_array());

    // Unshare without project_id must be rejected.
    let (status, json) = post_json(
        &app,
        "/api/experience/unshare",
        serde_json::json!({ "exp_key": "exp:missing" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["success"], false);
}
