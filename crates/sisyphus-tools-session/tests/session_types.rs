//! Integration tests for sisyphus-tools-session `session::types` and
//! `collaboration` modules.
//!
//! Covers the pure-data types and constructors that don't require a
//! filesystem: [`Message`], [`Session`], [`SessionCheckpoint`],
//! [`CompressStrategy`], and [`SessionVersion`] / [`VersionHistory`].

use sisyphus_tools_session::{
    CompressStrategy, Message, Session, SessionCheckpoint, SessionVersion, VersionHistory,
};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Message construction
// ---------------------------------------------------------------------------

#[test]
fn message_field_assignment() {
    let m = Message {
        role: "user".into(),
        content: "hello".into(),
        created_at: "2026-01-01T00:00:00.000Z".into(),
        estimated_tokens: 10,
        is_compressed: false,
    };
    assert_eq!(m.role, "user");
    assert_eq!(m.content, "hello");
    assert_eq!(m.estimated_tokens, 10);
    assert!(!m.is_compressed);
}

#[test]
fn message_serializes() {
    let m = Message {
        role: "assistant".into(),
        content: "hi back".into(),
        created_at: "t".into(),
        estimated_tokens: 2,
        is_compressed: false,
    };
    let j = serde_json::to_string(&m).unwrap();
    assert!(j.contains("assistant"));
    assert!(j.contains("hi back"));
}

// ---------------------------------------------------------------------------
// CompressStrategy
// ---------------------------------------------------------------------------

#[test]
fn compress_strategy_default() {
    let s = CompressStrategy::default();
    match s {
        CompressStrategy::Threshold(limit) => assert_eq!(limit, 32000),
    }
}

#[test]
fn compress_strategy_threshold_serde() {
    let s = CompressStrategy::Threshold(8000);
    let j = serde_json::to_string(&s).unwrap();
    let back: CompressStrategy = serde_json::from_str(&j).unwrap();
    match back {
        CompressStrategy::Threshold(limit) => assert_eq!(limit, 8000),
    }
}

// ---------------------------------------------------------------------------
// Session
// ---------------------------------------------------------------------------

#[test]
fn session_field_assignment() {
    let s = Session {
        id: "s1".into(),
        name: "My Session".into(),
        created_at: "t0".into(),
        updated_at: "t1".into(),
        message_count: 0,
        messages: vec![],
        metadata: HashMap::new(),
        parent_session_id: None,
        child_session_ids: vec![],
    };
    assert_eq!(s.id, "s1");
    assert_eq!(s.name, "My Session");
    assert_eq!(s.message_count, 0);
    assert!(s.messages.is_empty());
    assert!(s.parent_session_id.is_none());
    assert!(s.child_session_ids.is_empty());
}

#[test]
fn session_with_parent_id() {
    let s = Session {
        id: "child".into(),
        name: "n".into(),
        created_at: "t".into(),
        updated_at: "t".into(),
        message_count: 0,
        messages: vec![],
        metadata: HashMap::new(),
        parent_session_id: Some("parent".into()),
        child_session_ids: vec![],
    };
    assert_eq!(s.parent_session_id.as_deref(), Some("parent"));
}

// ---------------------------------------------------------------------------
// SessionCheckpoint
// ---------------------------------------------------------------------------

#[test]
fn session_checkpoint_field_assignment() {
    let c = SessionCheckpoint {
        session_id: "s1".into(),
        last_message_idx: 7,
        timestamp: "t".into(),
        metadata: HashMap::new(),
    };
    assert_eq!(c.session_id, "s1");
    assert_eq!(c.last_message_idx, 7);
}

// ---------------------------------------------------------------------------
// VersionHistory
// ---------------------------------------------------------------------------

#[test]
fn version_history_new_empty() {
    let h = VersionHistory::new("s1");
    assert_eq!(h.session_id, "s1");
    assert!(h.versions.is_empty());
    assert_eq!(h.len(), 0);
    assert!(h.is_empty());
}

#[test]
fn version_history_push_and_latest() {
    let mut h = VersionHistory::new("s1");
    h.push(SessionVersion {
        version: "v1".into(),
        session_id: "s1".into(),
        messages: vec![],
        metadata: HashMap::new(),
        created_at: "t1".into(),
        commit_message: None,
    });
    assert_eq!(h.len(), 1);
    assert!(!h.is_empty());
    let latest = h.latest().expect("latest");
    assert_eq!(latest.version, "v1");
    assert_eq!(latest.session_id, "s1");
}

#[test]
fn version_history_push_multiple() {
    let mut h = VersionHistory::new("s1");
    for i in 0..3 {
        h.push(SessionVersion {
            version: format!("v{i}"),
            session_id: "s1".into(),
            messages: vec![],
            metadata: HashMap::new(),
            created_at: format!("t{i}"),
            commit_message: Some(format!("commit {i}")),
        });
    }
    assert_eq!(h.len(), 3);
    assert_eq!(h.latest().unwrap().version, "v2");
}

#[test]
fn version_history_get_version() {
    let mut h = VersionHistory::new("s1");
    h.push(SessionVersion {
        version: "v1".into(),
        session_id: "s1".into(),
        messages: vec![],
        metadata: HashMap::new(),
        created_at: "t".into(),
        commit_message: None,
    });
    h.push(SessionVersion {
        version: "v2".into(),
        session_id: "s1".into(),
        messages: vec![],
        metadata: HashMap::new(),
        created_at: "t".into(),
        commit_message: None,
    });
    assert!(h.get_version("v1").is_some());
    assert!(h.get_version("v2").is_some());
    assert!(h.get_version("v3").is_none());
}

#[test]
fn version_history_latest_empty() {
    let h = VersionHistory::new("s1");
    assert!(h.latest().is_none());
}

// ---------------------------------------------------------------------------
// SessionVersion
// ---------------------------------------------------------------------------

#[test]
fn session_version_field_assignment() {
    let v = SessionVersion {
        version: "v1".into(),
        session_id: "s1".into(),
        messages: vec![],
        metadata: HashMap::new(),
        created_at: "t".into(),
        commit_message: Some("initial".into()),
    };
    assert_eq!(v.version, "v1");
    assert_eq!(v.session_id, "s1");
    assert_eq!(v.commit_message.as_deref(), Some("initial"));
}
