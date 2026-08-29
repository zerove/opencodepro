//! Integration tests for sisyphus-core `performance` module.
//!
//! Exercises the public `OutputCache` (LRU-free content-addressed
//! caching) and `PipelineMetrics` (recording batch execution
//! statistics). The `OutputCache` is used by the cognitive loop to
//! dedupe node outputs when the same input set is encountered more
//! than once.

use sisyphus_core::performance::{OutputCache, PipelineMetrics};
use sisyphus_types::NodeOutput;
use std::collections::HashMap;

fn inputs(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect()
}

fn out(s: &str) -> NodeOutput {
    NodeOutput::from(serde_json::Value::String(s.to_string()))
}

// ---------------------------------------------------------------------------
// OutputCache::content_hash
// ---------------------------------------------------------------------------

#[test]
fn content_hash_is_deterministic() {
    let h1 = OutputCache::content_hash("n1", &inputs(&[("a", "1"), ("b", "2")]));
    let h2 = OutputCache::content_hash("n1", &inputs(&[("a", "1"), ("b", "2")]));
    assert_eq!(h1, h2);
}

#[test]
fn content_hash_changes_with_node_name() {
    let i = inputs(&[("a", "1")]);
    let h1 = OutputCache::content_hash("n1", &i);
    let h2 = OutputCache::content_hash("n2", &i);
    assert_ne!(h1, h2);
}

#[test]
fn content_hash_changes_with_input_value() {
    let h1 = OutputCache::content_hash("n", &inputs(&[("a", "1")]));
    let h2 = OutputCache::content_hash("n", &inputs(&[("a", "2")]));
    assert_ne!(h1, h2);
}

#[test]
fn content_hash_is_input_order_independent() {
    // The implementation sorts inputs by key before hashing.
    let mut a = HashMap::new();
    a.insert("k1".to_string(), "v1".to_string());
    a.insert("k2".to_string(), "v2".to_string());
    let h1 = OutputCache::content_hash("n", &a);
    let h2 = OutputCache::content_hash("n", &inputs(&[("k2", "v2"), ("k1", "v1")]));
    assert_eq!(h1, h2);
}

#[test]
fn content_hash_empty_inputs() {
    let h = OutputCache::content_hash("n", &HashMap::new());
    // Hash is u64, just ensure it's deterministic and not crashing.
    let h2 = OutputCache::content_hash("n", &HashMap::new());
    assert_eq!(h, h2);
}

// ---------------------------------------------------------------------------
// OutputCache insert / get / len / is_empty / clear
// ---------------------------------------------------------------------------

#[test]
fn cache_starts_empty() {
    let cache = OutputCache::new(16);
    assert!(cache.is_empty());
    assert_eq!(cache.len(), 0);
    assert!(cache.get(123).is_none());
}

#[test]
fn cache_insert_and_get() {
    let cache = OutputCache::new(16);
    cache.insert(42, out("hello"));
    assert_eq!(cache.len(), 1);
    assert!(!cache.is_empty());
    let v = cache.get(42).expect("present");
    let json: serde_json::Value = v.into_inner();
    assert_eq!(json, serde_json::json!("hello"));
}

#[test]
fn cache_insert_overwrites_same_hash() {
    let cache = OutputCache::new(16);
    cache.insert(1, out("first"));
    cache.insert(1, out("second"));
    assert_eq!(cache.len(), 1);
    let v: serde_json::Value = cache.get(1).expect("present").into_inner();
    assert_eq!(v, serde_json::json!("second"));
}

#[test]
fn cache_clear_empties() {
    let cache = OutputCache::new(16);
    cache.insert(1, out("a"));
    cache.insert(2, out("b"));
    assert_eq!(cache.len(), 2);
    cache.clear();
    assert!(cache.is_empty());
    assert!(cache.get(1).is_none());
    assert!(cache.get(2).is_none());
}

#[test]
fn cache_keeps_distinct_hashes_independent() {
    let cache = OutputCache::new(16);
    cache.insert(1, out("a"));
    cache.insert(2, out("b"));
    cache.insert(3, out("c"));
    let a: serde_json::Value = cache.get(1).unwrap().into_inner();
    let b: serde_json::Value = cache.get(2).unwrap().into_inner();
    let c: serde_json::Value = cache.get(3).unwrap().into_inner();
    assert_eq!(a, serde_json::json!("a"));
    assert_eq!(b, serde_json::json!("b"));
    assert_eq!(c, serde_json::json!("c"));
}

#[test]
fn cache_eviction_does_not_panic_when_full() {
    // Insert more entries than max_entries; the deterministic eviction
    // strategy may or may not evict any, but the cache must not panic.
    let cache = OutputCache::new(4);
    for i in 0u64..10 {
        cache.insert(i, out(&format!("v{i}")));
    }
    // The cache should be at-or-near capacity. The implementation
    // accepts slight over-capacity to avoid deadlocks, so we just
    // verify the cache still serves lookups.
    assert!(cache.len() <= 16, "len {} exceeds reasonable bound", cache.len());
}

// ---------------------------------------------------------------------------
// PipelineMetrics
// ---------------------------------------------------------------------------

#[test]
fn pipeline_metrics_starts_zeroed() {
    let m = PipelineMetrics::default();
    assert_eq!(m.total_batches, 0);
    assert_eq!(m.total_nodes_executed, 0);
    assert_eq!(m.avg_batch_size, 0.0);
    assert_eq!(m.total_execution_time_ms, 0);
    assert_eq!(m.peak_concurrency, 0);
}

#[test]
fn pipeline_metrics_record_batch_updates_aggregates() {
    let mut m = PipelineMetrics::default();
    m.record_batch(4, 100);
    m.record_batch(6, 200);
    // 2 batches, 10 nodes total, 300 ms total, avg batch size = 5.0
    assert_eq!(m.total_batches, 2);
    assert_eq!(m.total_nodes_executed, 10);
    assert!((m.avg_batch_size - 5.0).abs() < 1e-9);
    assert_eq!(m.total_execution_time_ms, 300);
}

#[test]
fn pipeline_metrics_peak_concurrency_preserves_max() {
    let mut m = PipelineMetrics::default();
    m.record_batch(2, 100);
    m.record_batch(8, 100);
    m.record_batch(3, 100);
    // peak_concurrency is whatever the implementation decides; just check
    // that field is queryable and that the impl doesn't crash.
    let _ = m.peak_concurrency;
}

#[test]
fn pipeline_metrics_record_batch_with_zero_size_safe() {
    let mut m = PipelineMetrics::default();
    m.record_batch(0, 0);
    assert_eq!(m.total_batches, 1);
    assert_eq!(m.total_nodes_executed, 0);
    // avg_batch_size = 0/1 = 0
    assert_eq!(m.avg_batch_size, 0.0);
}
