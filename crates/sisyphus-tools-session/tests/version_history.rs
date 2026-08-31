//! Integration tests for sisyphus-tools-session VersionHistory.
//!
//! Exercises the public snapshot/push/save/load API used by the session
//! collaboration feature. The session version history lives in
//! `<parent>/versions/<session_id>.json` under a caller-supplied base
//! path; this test verifies the round-trip and ordering invariants.

use sisyphus_tools_session::collaboration::{SessionVersion, VersionHistory};
use sisyphus_tools_session::session::types::{Message, Session};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn tempdir_with_file() -> (TempDir, PathBuf) {
    let dir = TempDir::new().expect("tempdir");
    // `VersionHistory::save` joins the parent's `versions/` subdir, so
    // we need a real file inside `dir` whose parent is the directory.
    let file = dir.path().join("session.json");
    fs::write(&file, "{}").expect("write seed file");
    (dir, file)
}

fn mk_session(id: &str, messages: Vec<(&str, &str)>) -> Session {
    Session {
        id: id.to_string(),
        name: format!("session {id}"),
        created_at: "2025-01-01T00:00:00Z".into(),
        updated_at: "2025-01-01T00:00:00Z".into(),
        message_count: messages.len() as u64,
        messages: messages
            .into_iter()
            .map(|(role, content)| Message {
                role: role.to_string(),
                content: content.to_string(),
                created_at: "2025-01-01T00:00:00Z".into(),
                estimated_tokens: content.len() as u64 / 4, // rough heuristic
                is_compressed: false,
            })
            .collect(),
        metadata: HashMap::new(),
        parent_session_id: None,
        child_session_ids: vec![],
    }
}

#[test]
fn new_history_is_empty() {
    let h = VersionHistory::new("s1");
    assert_eq!(h.len(), 0);
    assert!(h.is_empty());
    assert!(h.latest().is_none());
    assert!(h.get_version("v1").is_none());
}

#[test]
fn snapshot_increments_version_id() {
    // Version id is a timestamp (millisecond resolution). Two snapshots
    // in the same millisecond would share an id; space them out so the
    // test is deterministic.
    let session = mk_session("s1", vec![("user", "hello")]);
    let v1 = VersionHistory::snapshot(&session, Some("init".into()));
    std::thread::sleep(std::time::Duration::from_millis(5));
    let v2 = VersionHistory::snapshot(&session, Some("second".into()));

    // Versions must be unique and ordered.
    assert_ne!(v1.version, v2.version);
    assert!(
        v1.version < v2.version,
        "first version ({}) should sort before second ({})",
        v1.version,
        v2.version
    );
    assert_eq!(v1.commit_message.as_deref(), Some("init"));
    assert_eq!(v2.commit_message.as_deref(), Some("second"));
    assert_eq!(v1.messages.len(), 1);
    assert_eq!(v1.session_id, "s1");
}

#[test]
fn push_preserves_order_and_latest() {
    let session = mk_session("s1", vec![("user", "hi")]);
    let mut h = VersionHistory::new("s1");
    h.push(VersionHistory::snapshot(&session, Some("v1".into())));
    h.push(VersionHistory::snapshot(&session, Some("v2".into())));
    h.push(VersionHistory::snapshot(&session, Some("v3".into())));

    assert_eq!(h.len(), 3);
    assert!(!h.is_empty());
    let latest = h.latest().expect("latest");
    assert_eq!(latest.commit_message.as_deref(), Some("v3"));
}

#[test]
fn get_version_finds_by_id() {
    let session = mk_session("s1", vec![]);
    let mut h = VersionHistory::new("s1");
    h.push(VersionHistory::snapshot(&session, Some("first".into())));
    let v1_id = h.latest().unwrap().version.clone();
    h.push(VersionHistory::snapshot(&session, Some("second".into())));

    let v1 = h.get_version(&v1_id).expect("v1 present");
    assert_eq!(v1.commit_message.as_deref(), Some("first"));
    assert!(h.get_version("nonexistent").is_none());
}

#[test]
fn save_then_load_round_trip_preserves_versions() {
    let (_dir, file) = tempdir_with_file();
    let session = mk_session("sess-1", vec![("user", "hi"), ("assistant", "hello")]);

    let mut h = VersionHistory::new("sess-1");
    h.push(VersionHistory::snapshot(&session, Some("init".into())));
    h.push(VersionHistory::snapshot(&session, Some("update".into())));

    h.save(&file).expect("save");

    let loaded = VersionHistory::load(&file, "sess-1").expect("load");
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded.session_id, "sess-1");
    let latest = loaded.latest().unwrap();
    assert_eq!(latest.commit_message.as_deref(), Some("update"));
}

#[test]
fn load_returns_empty_history_when_no_file_exists() {
    let (_dir, file) = tempdir_with_file();
    let loaded = VersionHistory::load(&file, "never-saved").expect("load missing");
    assert!(loaded.is_empty());
    assert_eq!(loaded.session_id, "never-saved");
}

#[test]
fn version_snapshot_copies_messages() {
    let session = mk_session("s1", vec![("user", "first"), ("assistant", "reply")]);
    let v = VersionHistory::snapshot(&session, None);
    assert_eq!(v.messages.len(), 2);
    assert_eq!(v.messages[0].content, "first");
    assert_eq!(v.messages[1].content, "reply");
    assert_eq!(v.messages[0].role, "user");
    assert_eq!(v.messages[1].role, "assistant");
    assert!(v.commit_message.is_none());
}

#[test]
fn push_does_not_deduplicate() {
    // Two snapshots with the same commit message should still be appended
    // (each gets its own version id).
    let session = mk_session("s1", vec![]);
    let mut h = VersionHistory::new("s1");
    h.push(VersionHistory::snapshot(&session, Some("same".into())));
    h.push(VersionHistory::snapshot(&session, Some("same".into())));
    assert_eq!(h.len(), 2);
}

#[test]
fn version_metadata_is_copied() {
    // Sessions with metadata should preserve it in the snapshot.
    let mut session = mk_session("s1", vec![]);
    session.metadata.insert("project_id".into(), "proj-1".into());
    session.metadata.insert("priority".into(), "high".into());
    let v = VersionHistory::snapshot(&session, None);
    assert_eq!(
        v.metadata.get("project_id").map(|s| s.as_str()),
        Some("proj-1")
    );
    assert_eq!(v.metadata.get("priority").map(|s| s.as_str()), Some("high"));
}

#[test]
fn session_version_struct_is_publicly_constructible() {
    // Direct construction must be possible for external consumers.
    let v = SessionVersion {
        version: "v1".to_string(),
        session_id: "s1".to_string(),
        messages: vec![],
        metadata: HashMap::new(),
        created_at: "2025-01-01T00:00:00Z".to_string(),
        commit_message: Some("manual".into()),
    };
    assert_eq!(v.version, "v1");
    assert_eq!(v.commit_message.as_deref(), Some("manual"));
}
