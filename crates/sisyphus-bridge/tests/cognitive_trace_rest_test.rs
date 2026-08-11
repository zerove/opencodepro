//! REST API tests for the cognitive trace (explainability) endpoint.
//!
//! `GET /api/cognitive/trace?limit=N` should return the recorded cognitive
//! decisions (CognitiveStep / MicroReflection) from the ring buffer, newest
//! first, with the limit honoured and clamped.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use sisyphus_bridge::bridge_service::MySisyphusBridge;
use sisyphus_bridge::http::{build_router, RestApiState};
use sisyphus_core::execution_manager::CognitiveTraceEntry;
use sisyphus_tools_llm::metis::DummyLlmClient;
use sisyphus_types::{CognitiveMetrics, ExecutionEvent};
use std::sync::{Arc, RwLock};
use tower::ServiceExt;

/// Build a router plus a handle to the underlying bridge so tests can record
/// cognitive events directly into the trace buffer.
async fn setup() -> (Router, Arc<MySisyphusBridge>) {
    let mut config = sisyphus_types::Config::default();
    config.cache.database_url = "sqlite::memory:".to_string();
    config.sandbox.wasm_tools_dir = "tests/wasm_tools".to_string();
    let shared_config = Arc::new(RwLock::new(config));

    let bridge = Arc::new(
        MySisyphusBridge::new(shared_config, Some(Arc::new(DummyLlmClient)))
            .await
            .expect("Failed to create bridge"),
    );

    let router = build_router(RestApiState::new(bridge.clone(), std::env::temp_dir()));
    (router, bridge)
}

async fn get(app: &Router, uri: &str) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("request build failed"),
        )
        .await
        .expect("request failed");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body read failed");
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

fn metrics(deviation: f64, confidence: f64) -> CognitiveMetrics {
    CognitiveMetrics {
        iteration: 1,
        deviation,
        confidence,
        progress: 0.5,
        token_cost: 10,
        stability: 0.9,
        info_gain: 0.1,
    }
}

fn step_entry(timestamp_ms: u128, sop_id: &str, step: &str) -> CognitiveTraceEntry {
    CognitiveTraceEntry {
        timestamp_ms,
        sop_id: sop_id.to_string(),
        event: ExecutionEvent::CognitiveStep {
            step: step.to_string(),
            message: format!("{step} message"),
            metrics: Some(metrics(0.2, 0.8)),
        },
    }
}

fn reflect_entry(
    timestamp_ms: u128,
    sop_id: &str,
    prediction: &str,
    actual: &str,
) -> CognitiveTraceEntry {
    CognitiveTraceEntry {
        timestamp_ms,
        sop_id: sop_id.to_string(),
        event: ExecutionEvent::MicroReflection {
            node_name: "node-a".to_string(),
            prediction: prediction.to_string(),
            actual: actual.to_string(),
            decision: "keep".to_string(),
            updates: None,
            reason: Some("within tolerance".to_string()),
        },
    }
}

#[tokio::test]
async fn test_cognitive_trace_empty() {
    let (app, _bridge) = setup().await;
    let (status, json) = get(&app, "/api/cognitive/trace").await;
    assert_eq!(status, StatusCode::OK);
    assert!(json["entries"].is_array());
    assert!(json["entries"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_cognitive_trace_returns_recorded_events() {
    let (app, bridge) = setup().await;
    let buffer = &bridge.kernel().execution_manager.cognitive_trace;
    buffer.push(step_entry(1000, "sop-a", "Predicting"));
    buffer.push(reflect_entry(2000, "sop-a", "expected out", "actual out"));

    let (status, json) = get(&app, "/api/cognitive/trace").await;
    assert_eq!(status, StatusCode::OK);
    let entries = json["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);

    // Newest first: the reflection comes before the step.
    let first = &entries[0];
    assert_eq!(first["sop_id"], "sop-a");
    assert!(first["event"]["MicroReflection"]["node_name"].is_string());
    assert_eq!(
        first["event"]["MicroReflection"]["prediction"],
        "expected out"
    );
    assert_eq!(first["event"]["MicroReflection"]["actual"], "actual out");

    let second = &entries[1];
    assert!(second["event"]["CognitiveStep"]["step"].is_string());
    assert_eq!(second["event"]["CognitiveStep"]["step"], "Predicting");
    assert_eq!(
        second["event"]["CognitiveStep"]["metrics"]["deviation"],
        0.2
    );
    assert_eq!(
        second["event"]["CognitiveStep"]["metrics"]["confidence"],
        0.8
    );
}

#[tokio::test]
async fn test_cognitive_trace_limit_and_order() {
    let (app, bridge) = setup().await;
    let buffer = &bridge.kernel().execution_manager.cognitive_trace;
    for i in 0..5 {
        buffer.push(step_entry(1000 + i as u128, "sop-a", &format!("Step{i}")));
    }

    let (status, json) = get(&app, "/api/cognitive/trace?limit=2").await;
    assert_eq!(status, StatusCode::OK);
    let entries = json["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);

    // Newest first and limit honoured.
    let steps: Vec<String> = entries
        .iter()
        .map(|e| {
            e["event"]["CognitiveStep"]["step"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(steps, vec!["Step4".to_string(), "Step3".to_string()]);
}

#[tokio::test]
async fn test_cognitive_trace_eviction_cap() {
    let (app, bridge) = setup().await;
    let buffer = &bridge.kernel().execution_manager.cognitive_trace;
    // Exceed the 200-entry ring capacity; the API clamps limit to 200.
    for i in 0..205_u32 {
        buffer.push(step_entry(1000 + i as u128, "sop-a", &format!("Bulk{i}")));
    }

    let (status, json) = get(&app, "/api/cognitive/trace?limit=1000").await;
    assert_eq!(status, StatusCode::OK);
    let entries = json["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 200);

    // Oldest entry must have been evicted (starts at index 5, not 0).
    let first = entries.last().unwrap();
    let step = first["event"]["CognitiveStep"]["step"].as_str().unwrap();
    assert_eq!(step, "Bulk5");
}
