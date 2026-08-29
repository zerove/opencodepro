//! Integration tests for sisyphus-types `execution` module's
//! pure data types: `SupervisorDecision`, `ExecutionEvent`,
//! `ExecutionHistory`, `HistoryRecord`.

use sisyphus_types::execution::{ExecutionEvent, ExecutionHistory, HistoryRecord, SupervisorDecision};

// ---------------------------------------------------------------------------
// SupervisorDecision
// ---------------------------------------------------------------------------

#[test]
fn supervisor_decision_continue() {
    let d = SupervisorDecision::Continue { deviation: 0.3 };
    match d {
        SupervisorDecision::Continue { deviation } => assert!((deviation - 0.3).abs() < 1e-9),
        _ => panic!("expected Continue"),
    }
}

#[test]
fn supervisor_decision_pivot_no_correction() {
    let d = SupervisorDecision::Pivot {
        reason: "off track".into(),
        suggested_correction: None,
    };
    match d {
        SupervisorDecision::Pivot { reason, suggested_correction } => {
            assert_eq!(reason, "off track");
            assert!(suggested_correction.is_none());
        }
        _ => panic!("expected Pivot"),
    }
}

#[test]
fn supervisor_decision_pivot_with_correction() {
    use sisyphus_types::graph::SOPGraphConfig;
    let d = SupervisorDecision::Pivot {
        reason: "use retry".into(),
        suggested_correction: Some(SOPGraphConfig::default()),
    };
    match d {
        SupervisorDecision::Pivot { suggested_correction, .. } => {
            assert!(suggested_correction.is_some());
        }
        _ => panic!("expected Pivot"),
    }
}

#[test]
fn supervisor_decision_variants_distinct() {
    let v = vec![
        SupervisorDecision::Continue { deviation: 0.0 },
        SupervisorDecision::Pivot {
            reason: "x".into(),
            suggested_correction: None,
        },
    ];
    let mut seen = std::collections::HashSet::new();
    for d in &v {
        let s = format!("{d:?}");
        assert!(seen.insert(s));
    }
}

// ---------------------------------------------------------------------------
// HistoryRecord
// ---------------------------------------------------------------------------

#[test]
fn history_record_field_access() {
    let r = HistoryRecord {
        timestamp: 1234,
        trace: "executed X".into(),
        deviation_score: 0.1,
        success: true,
        cost: 50,
    };
    assert_eq!(r.timestamp, 1234);
    assert_eq!(r.trace, "executed X");
    assert_eq!(r.deviation_score, 0.1);
    assert!(r.success);
    assert_eq!(r.cost, 50);
}

#[test]
fn history_record_failed() {
    let r = HistoryRecord {
        timestamp: 0,
        trace: "errored".into(),
        deviation_score: 1.0,
        success: false,
        cost: 0,
    };
    assert!(!r.success);
    assert_eq!(r.deviation_score, 1.0);
}

#[test]
fn history_record_clone() {
    let a = HistoryRecord {
        timestamp: 1,
        trace: "t".into(),
        deviation_score: 0.5,
        success: true,
        cost: 10,
    };
    let b = a.clone();
    assert_eq!(a.timestamp, b.timestamp);
    assert_eq!(a.trace, b.trace);
}

#[test]
fn history_record_serde() {
    let r = HistoryRecord {
        timestamp: 100,
        trace: "ok".into(),
        deviation_score: 0.2,
        success: true,
        cost: 5,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: HistoryRecord = serde_json::from_str(&j).unwrap();
    assert_eq!(back.timestamp, 100);
    assert_eq!(back.cost, 5);
}

// ---------------------------------------------------------------------------
// ExecutionHistory
// ---------------------------------------------------------------------------

#[test]
fn execution_history_field_access() {
    let h = ExecutionHistory {
        sop_id: "sop-1".into(),
        records: vec![
            HistoryRecord {
                timestamp: 1,
                trace: "a".into(),
                deviation_score: 0.1,
                success: true,
                cost: 1,
            },
            HistoryRecord {
                timestamp: 2,
                trace: "b".into(),
                deviation_score: 0.2,
                success: true,
                cost: 2,
            },
        ],
    };
    assert_eq!(h.sop_id, "sop-1");
    assert_eq!(h.records.len(), 2);
}

#[test]
fn execution_history_empty() {
    let h = ExecutionHistory {
        sop_id: "x".into(),
        records: vec![],
    };
    assert!(h.records.is_empty());
}

#[test]
fn execution_history_clone() {
    let h = ExecutionHistory {
        sop_id: "x".into(),
        records: vec![],
    };
    let h2 = h.clone();
    assert_eq!(h.sop_id, h2.sop_id);
}

// ---------------------------------------------------------------------------
// ExecutionEvent
// ---------------------------------------------------------------------------

#[test]
fn execution_event_variants() {
    // Just construct a couple of variants
    let _ = ExecutionEvent::NodeStarted { node_name: "n1".into() };
}

#[test]
fn execution_event_node_started() {
    let e = ExecutionEvent::NodeStarted { node_name: "alpha".into() };
    match e {
        ExecutionEvent::NodeStarted { node_name } => assert_eq!(node_name, "alpha"),
        _ => panic!("expected NodeStarted"),
    }
}

#[test]
fn execution_event_node_failed() {
    let e = ExecutionEvent::NodeFailed {
        node_name: "beta".into(),
        error: "timeout".into(),
        fuel_consumed: 0,
        duration_ms: 100,
    };
    match e {
        ExecutionEvent::NodeFailed { node_name, error, .. } => {
            assert_eq!(node_name, "beta");
            assert_eq!(error, "timeout");
        }
        _ => panic!("expected NodeFailed"),
    }
}
