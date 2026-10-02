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
    // 关键：DummyLlmClient 没有 embedding 凭据，
    // 显式允许 dummy embedding 避免桥接启动失败。
    config.llm.embedding.allow_dummy_embeddings = true;
    let shared_config = Arc::new(RwLock::new(config));

    let bridge = Arc::new(
        MySisyphusBridge::new(shared_config, Some(Arc::new(DummyLlmClient)), None)
            .await
            .expect("Failed to create bridge"),
    );

    let router = build_router(RestApiState::new(bridge.clone(), std::env::temp_dir()));
    (router, bridge)
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
            decision: "CONTINUE".to_string(),
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
        .map(|e| e["event"]["CognitiveStep"]["step"].as_str().unwrap().to_string())
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

/// Poll the history endpoint until `want` entries appear (the persistence
/// writer is asynchronous).
async fn wait_history_entries(app: &Router, uri: &str, want: usize) -> serde_json::Value {
    for _ in 0..100 {
        let (status, json) = get(app, uri).await;
        if status == StatusCode::OK {
            if let Some(arr) = json["entries"].as_array() {
                if arr.len() >= want {
                    return json;
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("history endpoint did not reach {want} entries for {uri}");
}

#[tokio::test]
async fn test_cognitive_trace_history_returns_persisted() {
    let (app, bridge) = setup().await;
    let em = &bridge.kernel().execution_manager;
    em.record_cognitive_trace("sop-h", step_entry(0, "sop-h", "Predicting").event);
    em.record_cognitive_trace(
        "sop-h",
        reflect_entry(0, "sop-h", "expected out", "actual out").event,
    );
    em.record_cognitive_trace("sop-h", step_entry(0, "sop-h", "Reflecting").event);

    let json = wait_history_entries(&app, "/api/cognitive/trace/history?limit=10", 3).await;
    let entries = json["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 3);
    // Newest first (insertion order preserved via DESC id).
    let steps: Vec<String> = entries
        .iter()
        .map(|e| {
            if let Some(s) = e["event"]["CognitiveStep"]["step"].as_str() {
                s.to_string()
            } else {
                e["event"]["MicroReflection"]["node_name"].as_str().unwrap().to_string()
            }
        })
        .collect();
    assert_eq!(
        steps,
        vec![
            "Reflecting".to_string(),
            "node-a".to_string(),
            "Predicting".to_string()
        ]
    );
    // All rows belong to the recorded SOP.
    assert!(entries.iter().all(|e| e["sop_id"] == "sop-h"));
}

#[tokio::test]
async fn test_cognitive_trace_history_sop_filter_and_limit() {
    let (app, bridge) = setup().await;
    let em = &bridge.kernel().execution_manager;
    for i in 0..4 {
        em.record_cognitive_trace("sop-a", step_entry(0, "sop-a", &format!("A{i}")).event);
    }
    em.record_cognitive_trace("sop-b", step_entry(0, "sop-b", "B0").event);

    // Filter: only sop-b.
    let json = wait_history_entries(&app, "/api/cognitive/trace/history?sop_id=sop-b", 1).await;
    let entries = json["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["sop_id"], "sop-b");

    // Limit applies after filtering (sop-a has 4; request 2).
    let json =
        wait_history_entries(&app, "/api/cognitive/trace/history?sop_id=sop-a&limit=2", 2).await;
    let entries = json["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().all(|e| e["sop_id"] == "sop-a"));
    // Newest first.
    let step = entries[0]["event"]["CognitiveStep"]["step"].as_str().unwrap();
    assert_eq!(step, "A3");
}

/// Poll the summary endpoint until `total` decisions are aggregated.
async fn wait_summary_total(app: &Router, uri: &str, total: u64) -> serde_json::Value {
    for _ in 0..100 {
        let (status, json) = get(app, uri).await;
        if status == StatusCode::OK {
            if let Some(t) = json["total_decisions"].as_u64() {
                if t >= total {
                    return json;
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("summary endpoint did not reach {total} decisions for {uri}");
}

#[tokio::test]
async fn test_cognitive_trace_summary_endpoint() {
    let (app, bridge) = setup().await;
    let em = &bridge.kernel().execution_manager;
    // 2 steps with metrics (0.2 dev / 0.8 conf / 10 tok each) + 1 reflection.
    em.record_cognitive_trace("sop-m", step_entry(0, "sop-m", "Predicting").event);
    em.record_cognitive_trace("sop-m", step_entry(0, "sop-m", "Reflecting").event);
    em.record_cognitive_trace(
        "sop-m",
        reflect_entry(0, "sop-m", "expected out", "actual out").event,
    );

    let json = wait_summary_total(&app, "/api/cognitive/summary?limit=100", 3).await;
    assert_eq!(json["total_decisions"], 3);
    assert_eq!(json["step_count"], 2);
    assert_eq!(json["reflection_count"], 1);
    assert!((json["avg_deviation"].as_f64().unwrap() - 0.2).abs() < 1e-9);
    assert!((json["avg_confidence"].as_f64().unwrap() - 0.8).abs() < 1e-9);
    assert_eq!(json["total_token_cost"], 20);
    assert_eq!(json["decision_counts"]["CONTINUE"], 1);
    assert!(json["first_timestamp_ms"].as_u64().is_some());
    assert!(json["last_timestamp_ms"].as_u64().is_some());
}

#[tokio::test]
async fn test_cognitive_trace_summary_sop_filter() {
    let (app, bridge) = setup().await;
    let em = &bridge.kernel().execution_manager;
    em.record_cognitive_trace("sop-a", step_entry(0, "sop-a", "A0").event);
    em.record_cognitive_trace("sop-a", step_entry(0, "sop-a", "A1").event);
    em.record_cognitive_trace(
        "sop-b",
        reflect_entry(0, "sop-b", "expected out", "actual out").event,
    );

    // Filter: only sop-b aggregates 1 reflection.
    let json = wait_summary_total(&app, "/api/cognitive/summary?sop_id=sop-b", 1).await;
    assert_eq!(json["total_decisions"], 1);
    assert_eq!(json["reflection_count"], 1);
    assert_eq!(json["step_count"], 0);
}
