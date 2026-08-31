//! Integration tests for sisyphus-core `CognitiveTraceBuffer`.
//!
//! Exercises the public `CognitiveTraceBuffer` / `CognitiveTraceEntry`
//! API which is a thread-safe ring buffer of recent cognitive
//! decisions. The buffer is used by the explainability UI to query
//! recent loop activity after the fact.
//!
//! Each test uses a unique sop_id to avoid cross-test interference
//! (entries are sequenced by timestamp in the buffer).

use sisyphus_core::execution_manager::{CognitiveTraceBuffer, CognitiveTraceEntry};
use sisyphus_types::{ExecutionEvent, NodeOutput};
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
}

fn entry(sop_id: &str, node_name: &str, ts: u128) -> CognitiveTraceEntry {
    CognitiveTraceEntry {
        timestamp_ms: ts,
        sop_id: sop_id.to_string(),
        event: ExecutionEvent::NodeStarted {
            node_name: node_name.to_string(),
        },
    }
}

fn completed_entry(sop_id: &str, node_name: &str, ts: u128) -> CognitiveTraceEntry {
    CognitiveTraceEntry {
        timestamp_ms: ts,
        sop_id: sop_id.to_string(),
        event: ExecutionEvent::NodeCompleted {
            node_name: node_name.to_string(),
            output: NodeOutput::from(serde_json::json!("ok")),
            fuel_consumed: 10,
            duration_ms: 50,
        },
    }
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

#[test]
fn new_buffer_starts_empty() {
    let b = CognitiveTraceBuffer::new(10);
    assert!(b.is_empty());
    assert_eq!(b.len(), 0);
}

#[test]
fn new_buffer_with_zero_capacity_clamps_to_one() {
    // Implementation clamps capacity to max(1, requested).
    let b = CognitiveTraceBuffer::new(0);
    b.push(entry("s1", "n1", now_ms()));
    assert_eq!(b.len(), 1);
}

#[test]
fn new_buffer_with_one_capacity() {
    let b = CognitiveTraceBuffer::new(1);
    b.push(entry("s1", "n1", 1));
    b.push(entry("s1", "n2", 2));
    assert_eq!(b.len(), 1);
}

// ---------------------------------------------------------------------------
// push / len / is_empty
// ---------------------------------------------------------------------------

#[test]
fn push_adds_entries() {
    let b = CognitiveTraceBuffer::new(10);
    b.push(entry("s1", "n1", 1));
    assert_eq!(b.len(), 1);
    assert!(!b.is_empty());
}

#[test]
fn push_evicts_oldest_when_at_capacity() {
    let b = CognitiveTraceBuffer::new(3);
    b.push(entry("s1", "n1", 1));
    b.push(entry("s1", "n2", 2));
    b.push(entry("s1", "n3", 3));
    b.push(entry("s1", "n4", 4)); // evicts n1
    assert_eq!(b.len(), 3);
    // The most recent entry should be n4.
    let recent = b.recent(1);
    assert_eq!(recent[0].timestamp_ms, 4);
}

#[test]
fn push_preserves_sop_id() {
    let b = CognitiveTraceBuffer::new(5);
    b.push(entry("my-sop", "n1", 1));
    let recent = b.recent(1);
    assert_eq!(recent[0].sop_id, "my-sop");
}

#[test]
fn push_with_different_event_types() {
    let b = CognitiveTraceBuffer::new(5);
    b.push(entry("s1", "n1", 1));
    b.push(completed_entry("s1", "n2", 2));
    let recent = b.recent(2);
    assert_eq!(recent.len(), 2);
}

// ---------------------------------------------------------------------------
// recent
// ---------------------------------------------------------------------------

#[test]
fn recent_returns_newest_first() {
    let b = CognitiveTraceBuffer::new(10);
    b.push(entry("s1", "n1", 1));
    b.push(entry("s1", "n2", 2));
    b.push(entry("s1", "n3", 3));
    let recent = b.recent(3);
    assert_eq!(recent[0].timestamp_ms, 3);
    assert_eq!(recent[1].timestamp_ms, 2);
    assert_eq!(recent[2].timestamp_ms, 1);
}

#[test]
fn recent_with_limit_larger_than_size_returns_all() {
    let b = CognitiveTraceBuffer::new(10);
    b.push(entry("s1", "n1", 1));
    b.push(entry("s1", "n2", 2));
    let recent = b.recent(100);
    assert_eq!(recent.len(), 2);
}

#[test]
fn recent_with_zero_limit_returns_empty() {
    let b = CognitiveTraceBuffer::new(10);
    b.push(entry("s1", "n1", 1));
    let recent = b.recent(0);
    assert!(recent.is_empty());
}

#[test]
fn recent_on_empty_buffer_returns_empty() {
    let b = CognitiveTraceBuffer::new(10);
    let recent = b.recent(5);
    assert!(recent.is_empty());
}

#[test]
fn recent_partial() {
    let b = CognitiveTraceBuffer::new(10);
    for i in 0..5 {
        b.push(entry("s1", &format!("n{i}"), i));
    }
    let recent = b.recent(2);
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].timestamp_ms, 4);
    assert_eq!(recent[1].timestamp_ms, 3);
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
fn is_empty_after_clearing_via_eviction() {
    let b = CognitiveTraceBuffer::new(2);
    assert!(b.is_empty());
    b.push(entry("s1", "n1", 1));
    assert!(!b.is_empty());
    // Eviction does NOT empty the buffer — it just rotates entries.
    for i in 0..10 {
        b.push(entry("s1", "n{i}", i + 100));
    }
    assert!(!b.is_empty());
    assert_eq!(b.len(), 2);
}

#[test]
fn concurrent_push_does_not_panic() {
    use std::sync::Arc;
    use std::thread;
    let b = Arc::new(CognitiveTraceBuffer::new(100));
    let mut handles = vec![];
    for t in 0..4 {
        let b_clone = b.clone();
        handles.push(thread::spawn(move || {
            for i in 0..25 {
                b_clone.push(entry(&format!("s{t}"), &format!("n{i}"), i));
            }
        }));
    }
    for h in handles {
        h.join().expect("join");
    }
    // Total entries: 4 threads × 25 = 100, all fit.
    assert_eq!(b.len(), 100);
}
