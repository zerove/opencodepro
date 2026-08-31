//! Integration tests for sisyphus-mem `Snapshot` and `WriteOp` pure data types.

use serde_json::json;
use sisyphus_mem::{Snapshot, WriteOp};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Snapshot
// ---------------------------------------------------------------------------

#[test]
fn snapshot_field_access() {
    let mut data = HashMap::new();
    data.insert("a".into(), json!(1));
    data.insert("b".into(), json!("x"));
    let s = Snapshot { data };
    assert_eq!(s.data.len(), 2);
    assert_eq!(s.data.get("a").unwrap(), &json!(1));
}

#[test]
fn snapshot_empty() {
    let s = Snapshot {
        data: HashMap::new(),
    };
    assert!(s.data.is_empty());
}

#[test]
fn snapshot_iteration() {
    let mut data = HashMap::new();
    data.insert("a".into(), json!(1));
    data.insert("b".into(), json!("v"));
    let s = Snapshot { data };
    // Iterate via the inner map
    let mut count = 0;
    for _ in s.data.iter() {
        count += 1;
    }
    assert_eq!(count, 2);
}

// ---------------------------------------------------------------------------
// WriteOp
// ---------------------------------------------------------------------------

#[test]
fn write_op_set_variant() {
    let op = WriteOp::Set {
        key: "k".into(),
        value: json!(42),
    };
    match op {
        WriteOp::Set { key, value } => {
            assert_eq!(key, "k");
            assert_eq!(value, json!(42));
        },
        _ => panic!("expected Set"),
    }
}

#[test]
fn write_op_delete_variant() {
    let op = WriteOp::Delete { key: "k".into() };
    match op {
        WriteOp::Delete { key } => assert_eq!(key, "k"),
        _ => panic!("expected Delete"),
    }
}

#[test]
fn write_op_clear_variant() {
    let op = WriteOp::Clear;
    // Just construct
    let _ = op;
}

#[test]
fn write_op_set_with_complex_value() {
    let v = json!({
        "nested": {
            "key": "value",
            "list": [1, 2, 3]
        }
    });
    let op = WriteOp::Set {
        key: "k".into(),
        value: v.clone(),
    };
    match op {
        WriteOp::Set { value, .. } => assert_eq!(value, v),
        _ => panic!("expected Set"),
    }
}

#[test]
fn write_op_variants_distinct() {
    let ops = vec![
        WriteOp::Set {
            key: "k".into(),
            value: json!(1),
        },
        WriteOp::Delete { key: "k".into() },
        WriteOp::Clear,
    ];
    let mut seen = std::collections::HashSet::new();
    for op in &ops {
        let s = format!("{op:?}");
        assert!(seen.insert(s));
    }
}

#[test]
fn write_op_debug() {
    let op = WriteOp::Delete { key: "x".into() };
    let s = format!("{op:?}");
    assert!(s.contains("Delete"));
    assert!(s.contains("x"));
}

#[test]
fn write_op_set_null_value() {
    let op = WriteOp::Set {
        key: "k".into(),
        value: json!(null),
    };
    match op {
        WriteOp::Set { value, .. } => assert!(value.is_null()),
        _ => panic!("expected Set"),
    }
}
