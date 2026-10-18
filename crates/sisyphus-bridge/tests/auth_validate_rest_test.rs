//! /api/auth/validate REST 契约测试。
//!
//! 修复前缺陷（⑨）：该端点注册在受保护路由且 handler 在 JWT 模式恒返回
//! `valid=true`（`state.auth_token` 为 None 的分支）。SDK 探测坏凭据会被
//! 告知"可用"，且受保护路由使探测必须携带有效 token（循环依赖）。
//!
//! 修复后：
//! - 路由公开可达（探测无需先有凭据）；
//! - JWT 模式按 access token 校验 api_key（空 secret 与中间件一致视为未启用）；
//! - 静态 token 模式比较 api_key；开发模式（无认证配置）恒接受。

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use sisyphus_bridge::auth_jwt::JwtService;
use sisyphus_bridge::bridge_service::MySisyphusBridge;
use sisyphus_bridge::http::{build_router, RestApiState};
use sisyphus_tools_llm::metis::DummyLlmClient;
use std::sync::{Arc, RwLock};
use tower::ServiceExt;

fn jwt_config(secret: &str) -> sisyphus_types::config::JwtConfig {
    sisyphus_types::config::JwtConfig {
        secret: secret.into(),
        access_ttl_secs: 900,
        issuer: "sisyphus-bridge".into(),
    }
}

async fn setup(
    auth_token: Option<String>,
    jwt: Option<sisyphus_types::config::JwtConfig>,
) -> Router {
    let mut config = sisyphus_types::Config::default();
    config.cache.database_url = "sqlite::memory:".to_string();
    config.sandbox.wasm_tools_dir = "tests/wasm_tools".to_string();
    config.llm.embedding.allow_dummy_embeddings = true;
    let shared_config = Arc::new(RwLock::new(config));

    let bridge = Arc::new(
        MySisyphusBridge::new(shared_config, Some(Arc::new(DummyLlmClient)), None)
            .await
            .expect("Failed to create bridge"),
    );

    let mut state = RestApiState::new(bridge, std::env::temp_dir());
    state.auth_token = auth_token;
    state.jwt_config = jwt;
    build_router(state)
}

async fn post_validate(app: &Router, api_key: &str) -> (StatusCode, serde_json::Value) {
    let request = Request::builder()
        .method("POST")
        .uri("/api/auth/validate")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({ "api_key": api_key }).to_string(),
        ))
        .expect("request build failed");
    let response = app.clone().oneshot(request).await.expect("request failed");
    let status = response.status();
    let bytes =
        axum::body::to_bytes(response.into_body(), usize::MAX).await.expect("body read failed");
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

#[tokio::test]
async fn jwt_mode_rejects_garbage_key_without_authorization_header() {
    let app = setup(None, Some(jwt_config("p1-test-secret"))).await;
    let (status, json) = post_validate(&app, "not-a-token").await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "坏凭据必须 401；实际 body：{json}"
    );
    assert_eq!(
        json["valid"].as_bool(),
        Some(false),
        "handler 必须真正校验（旧缺陷：JWT 模式恒 valid=true）；实际 body：{json}"
    );
}

#[tokio::test]
async fn jwt_mode_accepts_token_minted_by_same_config() {
    let cfg = jwt_config("p1-test-secret");
    let service = JwtService::new(cfg.clone()).expect("jwt service");
    let token = service
        .create_access_token("tester", vec!["admin".into()], vec!["*".into()])
        .expect("mint access token");

    let app = setup(None, Some(cfg)).await;
    let (status, json) = post_validate(&app, &token).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "有效 token 必须 200；实际 body：{json}"
    );
    assert_eq!(json["valid"].as_bool(), Some(true), "实际 body：{json}");
}

#[tokio::test]
async fn legacy_token_mode_compares_api_key() {
    let app = setup(Some("static-secret".into()), None).await;

    let (status, json) = post_validate(&app, "static-secret").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "正确 key 必须 200；实际 body：{json}"
    );
    assert_eq!(json["valid"].as_bool(), Some(true));

    let (status, json) = post_validate(&app, "wrong-key").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "错误 key 必须 401");
    assert_eq!(json["valid"].as_bool(), Some(false), "实际 body：{json}");
}

#[tokio::test]
async fn jwt_mode_with_empty_secret_treated_as_no_auth() {
    let app = setup(None, Some(jwt_config(""))).await;
    let (status, json) = post_validate(&app, "anything").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json["valid"].as_bool(),
        Some(true),
        "空 secret 与 jwt_auth_middleware 的放行语义一致"
    );
}

#[tokio::test]
async fn dev_mode_accepts_any_key() {
    let app = setup(None, None).await;
    let (status, json) = post_validate(&app, "anything").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["valid"].as_bool(), Some(true));
}
