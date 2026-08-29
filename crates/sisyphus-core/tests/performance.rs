//! Integration tests for sisyphus-core `performance` module.
//!
//! Exercises the public surfaces of [`PipelineMetrics`] (record_batch)
//! and [`OutputCache`] (new, content_hash, get, insert, len, is_empty).

use sisyphus_core::performance::{OutputCache, PipelineMetrics};
use sisyphus_types::execution::NodeOutput;
use std::collections::HashMap;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// PipelineMetrics
// ---------------------------------------------------------------------------

#[test]
fn pipeline_metrics_default_zero() {
    let m = PipelineMetrics {
        total_batches: 0,
        total_nodes_executed: 0,
        avg_batch_size: 0.0,
        total_execution_time_ms: 0,
        peak_concurrency: 0,
    };
    assert_eq!(m.total_batches, 0);
    assert_eq!(m.total_nodes_executed, 0);
    assert_eq!(m.avg_batch_size, 0.0);
    assert_eq!(m.total_execution_time_ms, 0);
    assert_eq!(m.peak_concurrency, 0);
}

#[test]
fn pipeline_metrics_record_batch_first() {
    let mut m = PipelineMetrics {
        total_batches: 0,
        total_nodes_executed: 0,
        avg_batch_size: 0.0,
        total_execution_time_ms: 0,
        peak_concurrency: 0,
    };
    m.record_batch(4, 100);
    assert_eq!(m.total_batches, 1);
    assert_eq!(m.total_nodes_executed, 4);
    assert_eq!(m.total_execution_time_ms, 100);
    assert!((m.avg_batch_size - 4.0).abs() < 1e-9);
    assert_eq!(m.peak_concurrency, 4);
}

#[test]
fn pipeline_metrics_record_batch_multiple() {
    let mut m = PipelineMetrics {
        total_batches: 0,
        total_nodes_executed: 0,
        avg_batch_size: 0.0,
        total_execution_time_ms: 0,
        peak_concurrency: 0,
    };
    m.record_batch(2, 50);
    m.record_batch(6, 100);
    m.record_batch(2, 50);
    assert_eq!(m.total_batches, 3);
    assert_eq!(m.total_nodes_executed, 10);
    assert_eq!(m.total_execution_time_ms, 200);
    // avg = 10/3
    assert!((m.avg_batch_size - 10.0 / 3.0).abs() < 1e-9);
    // peak = max(2, 6, 2) = 6
    assert_eq!(m.peak_concurrency, 6);
}

#[test]
fn pipeline_metrics_peak_concurrency_does_not_decrease() {
    let mut m = PipelineMetrics {
        total_batches: 0,
        total_nodes_executed: 0,
        avg_batch_size: 0.0,
        total_execution_time_ms: 0,
        peak_concurrency: 0,
    };
    m.record_batch(10, 100);
    m.record_batch(2, 50);
    // Even though later batch is smaller, peak stays at 10.
    assert_eq!(m.peak_concurrency, 10);
}

// ---------------------------------------------------------------------------
// OutputCache
// ---------------------------------------------------------------------------

#[test]
fn output_cache_new_is_empty() {
    let c = OutputCache::new(16);
    assert_eq!(c.len(), 0);
    assert!(c.is_empty());
}

#[test]
fn output_cache_insert_and_get() {
    let c = OutputCache::new(16);
    let h = 12345_u64;
    let out = NodeOutput(Arc::new(serde_json::json!("hello")));
    c.insert(h, out.clone());
    assert_eq!(c.len(), 1);
    assert!(!c.is_empty());
    let got = c.get(h).unwrap();
    assert_eq!(got.to_string(), out.to_string());
}

#[test]
fn output_cache_get_missing_returns_none() {
    let c = OutputCache::new(16);
    assert!(c.get(99999).is_none());
}

#[test]
fn output_cache_overwrite_same_hash() {
    let c = OutputCache::new(16);
    let h = 1;
    c.insert(h, NodeOutput(Arc::new(serde_json::json!("v1"))));
    c.insert(h, NodeOutput(Arc::new(serde_json::json!("v2"))));
    // Still 1 entry; the value is "v2".
    assert_eq!(c.len(), 1);
    assert_eq!(c.get(h).unwrap().to_string(), "\"v2\"");
}

#[test]
fn output_cache_content_hash_deterministic() {
    let mut m1 = HashMap::new();
    m1.insert("a".to_string(), "1".to_string());
    m1.insert("b".to_string(), "2".to_string());
    // Same map, different insertion order.
    let mut m2 = HashMap::new();
    m2.insert("b".to_string(), "2".to_string());
    m2.insert("a".to_string(), "1".to_string());
    let h1 = OutputCache::content_hash("node", &m1);
    let h2 = OutputCache::content_hash("node", &m2);
    assert_eq!(h1, h2);
}

#[test]
fn output_cache_content_hash_differs_by_name() {
    let m = HashMap::new();
    let h1 = OutputCache::content_hash("a", &m);
    let h2 = OutputCache::content_hash("b", &m);
    assert_ne!(h1, h2);
}

#[test]
fn output_cache_content_hash_differs_by_input() {
    let mut m1 = HashMap::new();
    m1.insert("x".to_string(), "1".to_string());
    let mut m2 = HashMap::new();
    m2.insert("x".to_string(), "2".to_string());
    let h1 = OutputCache::content_hash("n", &m1);
    let h2 = OutputCache::content_hash("n", &m2);
    assert_ne!(h1, h2);
}

#[test]
fn output_cache_content_hash_empty_inputs() {
    let m = HashMap::new();
    // Should not panic.
    let _ = OutputCache::content_hash("n", &m);
}
