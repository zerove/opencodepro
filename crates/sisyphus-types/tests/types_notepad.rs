//! Integration tests for sisyphus-types `types` (SisyphusNotepad,
//! NotepadSnapshot, EvolutionOp, FormalVerificationConfig,
//! MetaLearningSample) module.

use sisyphus_types::{
    EvolutionOp, FormalVerificationConfig, MetaLearningSample, NotepadSnapshot, SisyphusNotepad,
};

// ---------------------------------------------------------------------------
// SisyphusNotepad
// ---------------------------------------------------------------------------

#[test]
fn sisyphus_notepad_field_assignment() {
    let n = SisyphusNotepad {
        sections: Default::default(),
        history: vec![],
    };
    assert!(n.sections.is_empty());
    assert!(n.history.is_empty());
}

// ---------------------------------------------------------------------------
// NotepadSnapshot
// ---------------------------------------------------------------------------

#[test]
fn notepad_snapshot_field_assignment() {
    let s = NotepadSnapshot {
        timestamp: 12345,
        snapshot: Default::default(),
        trigger_node: "node-1".into(),
    };
    assert_eq!(s.timestamp, 12345);
    assert_eq!(s.trigger_node, "node-1");
}

#[test]
fn notepad_snapshot_serde() {
    let s = NotepadSnapshot {
        timestamp: 1,
        snapshot: Default::default(),
        trigger_node: "n".into(),
    };
    let j = serde_json::to_string(&s).unwrap();
    let back: NotepadSnapshot = serde_json::from_str(&j).unwrap();
    assert_eq!(back.trigger_node, "n");
}

// ---------------------------------------------------------------------------
// EvolutionOp
// ---------------------------------------------------------------------------

#[test]
fn evolution_op_adjust_fuel_serde() {
    let op = EvolutionOp::AdjustFuel {
        node_name: "n".into(),
        delta: 1000,
    };
    let j = serde_json::to_string(&op).unwrap();
    let back: EvolutionOp = serde_json::from_str(&j).unwrap();
    match back {
        EvolutionOp::AdjustFuel { node_name, delta } => {
            assert_eq!(node_name, "n");
            assert_eq!(delta, 1000);
        },
        _ => panic!("expected AdjustFuel"),
    }
}

#[test]
fn evolution_op_add_retry_serde() {
    let op = EvolutionOp::AddRetry {
        node_name: "n".into(),
        max_retries: 3,
    };
    let j = serde_json::to_string(&op).unwrap();
    let back: EvolutionOp = serde_json::from_str(&j).unwrap();
    match back {
        EvolutionOp::AddRetry {
            node_name,
            max_retries,
        } => {
            assert_eq!(node_name, "n");
            assert_eq!(max_retries, 3);
        },
        _ => panic!("expected AddRetry"),
    }
}

#[test]
fn evolution_op_insert_node_serde() {
    let op = EvolutionOp::InsertNode {
        after_node: "a".into(),
        new_name: "b".into(),
        tool_id: "t".into(),
    };
    let j = serde_json::to_string(&op).unwrap();
    let back: EvolutionOp = serde_json::from_str(&j).unwrap();
    match back {
        EvolutionOp::InsertNode {
            after_node,
            new_name,
            tool_id,
        } => {
            assert_eq!(after_node, "a");
            assert_eq!(new_name, "b");
            assert_eq!(tool_id, "t");
        },
        _ => panic!("expected InsertNode"),
    }
}

#[test]
fn evolution_op_distinct_variants() {
    let ops = [
        EvolutionOp::AdjustFuel {
            node_name: "n".into(),
            delta: 1,
        },
        EvolutionOp::AddRetry {
            node_name: "n".into(),
            max_retries: 1,
        },
        EvolutionOp::InsertNode {
            after_node: "a".into(),
            new_name: "b".into(),
            tool_id: "t".into(),
        },
    ];
    let mut seen = std::collections::HashSet::new();
    for op in ops {
        let d = format!("{op:?}");
        assert!(seen.insert(d.clone()));
    }
}

// ---------------------------------------------------------------------------
// FormalVerificationConfig
// ---------------------------------------------------------------------------

#[test]
fn formal_verification_config_field_assignment() {
    let c = FormalVerificationConfig {
        enable_kani: true,
        enable_loom: false,
        enable_proptest: true,
        max_proptest_cases: 1000,
    };
    assert!(c.enable_kani);
    assert!(!c.enable_loom);
    assert_eq!(c.max_proptest_cases, 1000);
}

#[test]
fn formal_verification_config_serde() {
    let c = FormalVerificationConfig {
        enable_kani: false,
        enable_loom: true,
        enable_proptest: false,
        max_proptest_cases: 100,
    };
    let j = serde_json::to_string(&c).unwrap();
    let back: FormalVerificationConfig = serde_json::from_str(&j).unwrap();
    assert!(back.enable_loom);
    assert_eq!(back.max_proptest_cases, 100);
}

// ---------------------------------------------------------------------------
// MetaLearningSample
// ---------------------------------------------------------------------------

#[test]
fn meta_learning_sample_field_assignment() {
    let s = MetaLearningSample {
        context: serde_json::json!({"k": "v"}),
        strategy: "explore".into(),
        params: serde_json::json!({"depth": 3}),
        success: true,
        duration_secs: 0.5,
        timestamp: 12345,
    };
    assert_eq!(s.strategy, "explore");
    assert!(s.success);
    assert_eq!(s.timestamp, 12345);
}

#[test]
fn meta_learning_sample_serde() {
    let s = MetaLearningSample {
        context: serde_json::json!(null),
        strategy: "s".into(),
        params: serde_json::json!(null),
        success: false,
        duration_secs: 0.0,
        timestamp: 0,
    };
    let j = serde_json::to_string(&s).unwrap();
    let back: MetaLearningSample = serde_json::from_str(&j).unwrap();
    assert_eq!(back.strategy, "s");
    assert!(!back.success);
}
