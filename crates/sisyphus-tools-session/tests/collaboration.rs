//! Integration tests for sisyphus-tools-session `collaboration` module.
//!
//! Exercises the pure-data types: [`AccessLevel`], [`DiffOp`],
//! [`SessionBranch`], [`SessionBranches`], [`CollaborationEvent`],
//! [`CollaborationEventType`], and [`CollaborationLog`].

use sisyphus_tools_session::collaboration::{
    AccessLevel, CollaborationEvent, CollaborationEventType, CollaborationLog, DiffOp,
    SessionBranch, SessionBranches,
};
use sisyphus_tools_session::Message;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// AccessLevel
// ---------------------------------------------------------------------------

#[test]
fn access_level_variants_distinct() {
    let mut seen = std::collections::HashSet::new();
    for a in [
        AccessLevel::Read,
        AccessLevel::Comment,
        AccessLevel::Write,
        AccessLevel::Owner,
    ] {
        let d = format!("{a:?}");
        assert!(seen.insert(d.clone()));
    }
}

#[test]
fn access_level_serde() {
    for a in [
        AccessLevel::Read,
        AccessLevel::Comment,
        AccessLevel::Write,
        AccessLevel::Owner,
    ] {
        let j = serde_json::to_string(&a).unwrap();
        let back: AccessLevel = serde_json::from_str(&j).unwrap();
        assert_eq!(format!("{back:?}"), format!("{a:?}"));
    }
}

// ---------------------------------------------------------------------------
// DiffOp
// ---------------------------------------------------------------------------

#[test]
fn diff_op_insert_serde() {
    let msg = Message {
        role: "user".into(),
        content: "hi".into(),
        created_at: "t".into(),
        estimated_tokens: 1,
        is_compressed: false,
    };
    let op = DiffOp::Insert {
        index: 0,
        message: msg,
    };
    let j = serde_json::to_string(&op).unwrap();
    let back: DiffOp = serde_json::from_str(&j).unwrap();
    match back {
        DiffOp::Insert { index, message } => {
            assert_eq!(index, 0);
            assert_eq!(message.content, "hi");
        }
        _ => panic!("expected Insert"),
    }
}

#[test]
fn diff_op_delete_serde() {
    let msg = Message {
        role: "user".into(),
        content: "old".into(),
        created_at: "t".into(),
        estimated_tokens: 1,
        is_compressed: false,
    };
    let op = DiffOp::Delete {
        index: 3,
        message: msg,
    };
    let j = serde_json::to_string(&op).unwrap();
    let back: DiffOp = serde_json::from_str(&j).unwrap();
    match back {
        DiffOp::Delete { index, message } => {
            assert_eq!(index, 3);
            assert_eq!(message.content, "old");
        }
        _ => panic!("expected Delete"),
    }
}

// ---------------------------------------------------------------------------
// SessionBranch
// ---------------------------------------------------------------------------

#[test]
fn session_branch_field_assignment() {
    let b = SessionBranch {
        name: "feature-x".into(),
        session_id: "s1".into(),
        tip_version: "v1".into(),
        created_at: "t".into(),
        description: None,
    };
    assert_eq!(b.name, "feature-x");
    assert_eq!(b.tip_version, "v1");
}

#[test]
fn session_branch_serde() {
    let b = SessionBranch {
        name: "b".into(),
        session_id: "s".into(),
        tip_version: "v".into(),
        created_at: "t".into(),
        description: None,
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: SessionBranch = serde_json::from_str(&j).unwrap();
    assert_eq!(back.name, "b");
}

// ---------------------------------------------------------------------------
// SessionBranches
// ---------------------------------------------------------------------------

#[test]
fn session_branches_field_assignment() {
    let b = SessionBranches {
        session_id: "s1".into(),
        active_branch: "main".into(),
        branches: HashMap::new(),
    };
    assert_eq!(b.active_branch, "main");
    assert!(b.branches.is_empty());
}

#[test]
fn session_branches_serde() {
    let mut m = HashMap::new();
    m.insert(
        "main".to_string(),
        SessionBranch {
            name: "main".into(),
            session_id: "s1".into(),
            tip_version: "v1".into(),
            created_at: "t".into(),
            description: None,
        },
    );
    let b = SessionBranches {
        session_id: "s1".into(),
        active_branch: "main".into(),
        branches: m,
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: SessionBranches = serde_json::from_str(&j).unwrap();
    assert_eq!(back.branches.len(), 1);
}

// ---------------------------------------------------------------------------
// CollaborationEventType
// ---------------------------------------------------------------------------

#[test]
fn collaboration_event_type_forked_serde() {
    let e = CollaborationEventType::Forked {
        new_session_id: "s2".into(),
    };
    let j = serde_json::to_string(&e).unwrap();
    let back: CollaborationEventType = serde_json::from_str(&j).unwrap();
    match back {
        CollaborationEventType::Forked { new_session_id } => {
            assert_eq!(new_session_id, "s2");
        }
        _ => panic!("expected Forked"),
    }
}

#[test]
fn collaboration_event_type_branched_serde() {
    let e = CollaborationEventType::Branched {
        branch_name: "feature".into(),
    };
    let j = serde_json::to_string(&e).unwrap();
    let back: CollaborationEventType = serde_json::from_str(&j).unwrap();
    match back {
        CollaborationEventType::Branched { branch_name } => {
            assert_eq!(branch_name, "feature");
        }
        _ => panic!("expected Branched"),
    }
}

// ---------------------------------------------------------------------------
// CollaborationEvent / CollaborationLog
// ---------------------------------------------------------------------------

#[test]
fn collaboration_event_field_assignment() {
    let e = CollaborationEvent {
        id: "e-1".into(),
        session_id: "s1".into(),
        actor: "alice".into(),
        timestamp: "2026-01-01".into(),
        event_type: CollaborationEventType::Branched {
            branch_name: "x".into(),
        },
    };
    assert_eq!(e.actor, "alice");
}

#[test]
fn collaboration_event_serde() {
    let e = CollaborationEvent {
        id: "e".into(),
        session_id: "s".into(),
        actor: "bob".into(),
        timestamp: "t".into(),
        event_type: CollaborationEventType::Forked {
            new_session_id: "new".into(),
        },
    };
    let j = serde_json::to_string(&e).unwrap();
    let back: CollaborationEvent = serde_json::from_str(&j).unwrap();
    assert_eq!(back.actor, "bob");
}

#[test]
fn collaboration_log_serde() {
    let log = CollaborationLog {
        session_id: "s1".into(),
        events: vec![],
    };
    let j = serde_json::to_string(&log).unwrap();
    let back: CollaborationLog = serde_json::from_str(&j).unwrap();
    assert!(back.events.is_empty());
    assert_eq!(back.session_id, "s1");
}
