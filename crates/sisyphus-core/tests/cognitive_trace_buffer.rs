//! 集成测试：core CognitiveTraceBuffer 环形缓冲
//!
//! 覆盖 `sisyphus_core::execution_manager::CognitiveTraceBuffer`：
//! - new / push / recent / len / is_empty / capacity
//! - 超过容量时丢弃最旧
//! - recent 倒序（newest first）

use sisyphus_core::execution_manager::{CognitiveTraceBuffer, CognitiveTraceEntry};
use sisyphus_types::ExecutionEvent;

fn make_entry(sop_id: &str, ts: u128) -> CognitiveTraceEntry {
    CognitiveTraceEntry {
        timestamp_ms: ts,
        sop_id: sop_id.to_string(),
        event: ExecutionEvent::NodeStarted {
            node_name: format!("node-{ts}"),
        },
    }
}

#[test]
fn buffer_new_is_empty() {
    let b = CognitiveTraceBuffer::new(10);
    assert!(b.is_empty());
    assert_eq!(b.len(), 0);
    assert!(b.recent(10).is_empty());
}

#[test]
fn buffer_push_one() {
    let b = CognitiveTraceBuffer::new(10);
    b.push(make_entry("sop-1", 1000));
    assert_eq!(b.len(), 1);
    assert!(!b.is_empty());
}

#[test]
fn buffer_recent_returns_newest_first() {
    let b = CognitiveTraceBuffer::new(10);
    b.push(make_entry("sop-1", 1000));
    b.push(make_entry("sop-1", 2000));
    b.push(make_entry("sop-1", 3000));
    let r = b.recent(3);
    assert_eq!(r.len(), 3);
    // newest first
    assert_eq!(r[0].timestamp_ms, 3000);
    assert_eq!(r[1].timestamp_ms, 2000);
    assert_eq!(r[2].timestamp_ms, 1000);
}

#[test]
fn buffer_recent_respects_limit() {
    let b = CognitiveTraceBuffer::new(10);
    for i in 0..5 {
        b.push(make_entry("sop-1", (i + 1) * 1000));
    }
    let r = b.recent(2);
    assert_eq!(r.len(), 2);
    assert_eq!(r[0].timestamp_ms, 5000);
    assert_eq!(r[1].timestamp_ms, 4000);
}

#[test]
fn buffer_recent_limit_larger_than_len() {
    let b = CognitiveTraceBuffer::new(10);
    b.push(make_entry("sop-1", 1000));
    b.push(make_entry("sop-1", 2000));
    let r = b.recent(100);
    assert_eq!(r.len(), 2);
}

#[test]
fn buffer_overflow_evicts_oldest() {
    let b = CognitiveTraceBuffer::new(3);
    for i in 1..=5 {
        b.push(make_entry("sop-1", i * 1000));
    }
    // Capacity 3, so only the last 3 remain: 3000, 4000, 5000
    assert_eq!(b.len(), 3);
    let r = b.recent(10);
    assert_eq!(r.len(), 3);
    assert_eq!(r[0].timestamp_ms, 5000);
    assert_eq!(r[1].timestamp_ms, 4000);
    assert_eq!(r[2].timestamp_ms, 3000);
}

#[test]
fn buffer_capacity_zero_clamped_to_one() {
    // capacity.max(1) ensures capacity 0 → 1
    let b = CognitiveTraceBuffer::new(0);
    b.push(make_entry("a", 1));
    assert_eq!(b.len(), 1);
    b.push(make_entry("a", 2));
    assert_eq!(b.len(), 1); // capacity 1 → oldest evicted
}

#[test]
fn buffer_capacity_one_keeps_only_latest() {
    let b = CognitiveTraceBuffer::new(1);
    b.push(make_entry("a", 1));
    b.push(make_entry("a", 2));
    b.push(make_entry("a", 3));
    let r = b.recent(10);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].timestamp_ms, 3);
}

#[test]
fn buffer_recent_zero_limit() {
    let b = CognitiveTraceBuffer::new(10);
    b.push(make_entry("a", 1));
    let r = b.recent(0);
    assert!(r.is_empty());
}

#[test]
fn buffer_thread_safe() {
    use std::sync::Arc;
    let b = Arc::new(CognitiveTraceBuffer::new(1000));
    let mut handles = vec![];
    for t in 0..5 {
        let b2 = b.clone();
        handles.push(std::thread::spawn(move || {
            for i in 0..20 {
                b2.push(make_entry("sop-1", t * 1000 + i));
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(b.len(), 100);
}

#[test]
fn buffer_entry_field_access() {
    let entry = make_entry("my-sop", 42);
    assert_eq!(entry.timestamp_ms, 42);
    assert_eq!(entry.sop_id, "my-sop");
    assert!(matches!(entry.event, ExecutionEvent::NodeStarted { .. }));
}

#[test]
fn buffer_entry_clone() {
    let entry = make_entry("sop", 100);
    let e2 = entry.clone();
    assert_eq!(entry.timestamp_ms, e2.timestamp_ms);
    assert_eq!(entry.sop_id, e2.sop_id);
}

#[test]
fn buffer_entry_serde() {
    let entry = make_entry("sop", 100);
    let json = serde_json::to_string(&entry).unwrap();
    let back: CognitiveTraceEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(back.timestamp_ms, 100);
    assert_eq!(back.sop_id, "sop");
}
