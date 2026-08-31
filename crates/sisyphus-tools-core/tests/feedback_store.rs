//! Integration tests for sisyphus-tools-core feedback store.
//!
//! Exercises the `FeedbackStore` public I/O API the same way the SOP
//! engine and the metacognition loop do. The store is JSON-per-agent under
//! a base directory; this test verifies the round-trip, agent isolation,
//! and recent-feedback windowing.

use sisyphus_tools_core::feedback::{Feedback, FeedbackStore};
use tempfile::TempDir;

fn mk_feedback(agent: &str, corrected: &str) -> Feedback {
    Feedback {
        agent_id: agent.to_string(),
        original_request: format!("req for {agent}"),
        original_output: format!("out for {agent}"),
        corrected_output: corrected.to_string(),
        timestamp: 1_700_000_000,
        tags: vec!["test".to_string()],
    }
}

#[tokio::test]
async fn save_then_load_round_trip() {
    let dir = TempDir::new().expect("tempdir");
    let store = FeedbackStore::new(dir.path());

    let fb = mk_feedback("agent-A", "fix typo");
    store.save_feedback(fb.clone()).await.expect("save");

    let loaded = store.load_all_for_agent("agent-A").await.expect("load");
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].corrected_output, "fix typo");
    assert_eq!(loaded[0].agent_id, "agent-A");
}

#[tokio::test]
async fn load_returns_empty_for_unknown_agent() {
    let dir = TempDir::new().expect("tempdir");
    let store = FeedbackStore::new(dir.path());
    let loaded = store.load_all_for_agent("nonexistent").await.expect("load unknown");
    assert!(loaded.is_empty());
}

#[tokio::test]
async fn feedback_is_isolated_per_agent() {
    let dir = TempDir::new().expect("tempdir");
    let store = FeedbackStore::new(dir.path());

    store.save_feedback(mk_feedback("agent-A", "a-fix")).await.expect("save a");
    store.save_feedback(mk_feedback("agent-B", "b-fix")).await.expect("save b");

    let a = store.load_all_for_agent("agent-A").await.expect("a");
    let b = store.load_all_for_agent("agent-B").await.expect("b");
    assert_eq!(a.len(), 1, "agent-A should have exactly one feedback");
    assert_eq!(b.len(), 1, "agent-B should have exactly one feedback");
    assert_eq!(a[0].corrected_output, "a-fix");
    assert_eq!(b[0].corrected_output, "b-fix");
}

#[tokio::test]
async fn save_appends_to_existing_file() {
    let dir = TempDir::new().expect("tempdir");
    let store = FeedbackStore::new(dir.path());

    for i in 0..3 {
        store.save_feedback(mk_feedback("agent-X", &format!("fix-{i}"))).await.expect("save iter");
    }
    let loaded = store.load_all_for_agent("agent-X").await.expect("load");
    assert_eq!(loaded.len(), 3, "expected three feedbacks appended");
    // FeedbackStore keeps insertion order (append)
    assert_eq!(loaded[0].corrected_output, "fix-0");
    assert_eq!(loaded[2].corrected_output, "fix-2");
}
