//! `trace_id_middleware` 的单元测试（T4.4）。
//!
//! 独立成文件而非写在 `http/mod.rs` 内：OpenAPI 覆盖测试会扫描
//! `http/mod.rs` 源码中的 `.route(` 调用，测试用的探针路由不应被当作
//! 生产路由计入文档覆盖检查。

use super::*;
use axum::body::Body;
use axum::http::Request as HttpRequest;
use tower::ServiceExt as _;

/// 回显 handler 从 request extensions 读到的 trace id。
async fn echo_trace_id(request: Request) -> String {
    request
        .extensions()
        .get::<sisyphus_types::trace::TraceId>()
        .map(|id| id.as_str().to_string())
        .unwrap_or_default()
}

fn probe_app() -> Router {
    Router::new()
        .route("/probe", axum::routing::get(echo_trace_id))
        .layer(middleware::from_fn(trace_id_middleware))
}

fn response_trace_id(response: &Response) -> String {
    response
        .headers()
        .get(sisyphus_types::trace::TRACE_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string()
}

#[tokio::test]
async fn trace_id_is_generated_when_request_has_none() {
    let response = probe_app()
        .oneshot(HttpRequest::builder().uri("/probe").body(Body::empty()).unwrap())
        .await
        .unwrap();

    let trace_id = response_trace_id(&response);
    assert!(
        trace_id.starts_with("trace-"),
        "应生成 trace id: {trace_id}"
    );
}

#[tokio::test]
async fn trace_id_is_propagated_to_handler_and_response() {
    let response = probe_app()
        .oneshot(
            HttpRequest::builder()
                .uri("/probe")
                .header(sisyphus_types::trace::TRACE_ID_HEADER, "trace-fixed")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response_trace_id(&response), "trace-fixed");
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(String::from_utf8_lossy(&body), "trace-fixed");
}

#[tokio::test]
async fn invalid_trace_id_is_replaced_by_generated_one() {
    let response = probe_app()
        .oneshot(
            HttpRequest::builder()
                .uri("/probe")
                .header(sisyphus_types::trace::TRACE_ID_HEADER, "bad id with space")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let trace_id = response_trace_id(&response);
    assert!(trace_id.starts_with("trace-"), "非法值应被替换: {trace_id}");
    assert_ne!(trace_id, "bad id with space");
}
