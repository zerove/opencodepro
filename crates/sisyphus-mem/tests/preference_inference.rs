//! Integration tests for sisyphus-mem `UserPreferenceStore::infer_preferences_from_feedback`.
//!
//! Exercises the feedback-tag → preference-key inference used by the
//! metacognition loop to learn implicit user preferences from corrections.
//! The function is sync (despite the `async` keyword on its parent impl
//! block); it does not require the underlying MfsStore to be connected.

use sisyphus_mem::MfsStore;
use sisyphus_mem::user_preferences::UserPreferenceStore;
use sisyphus_types::Feedback;
use std::sync::Arc;

async fn setup() -> UserPreferenceStore {
    let config = sisyphus_types::CacheConfig::default();
    let store = Arc::new(
        MfsStore::new("sqlite::memory:", &config)
            .await
            .expect("in-memory MfsStore"),
    );
    UserPreferenceStore::new(store, "integration-test".into())
}

fn fb_with_tags(tags: &[&str]) -> Feedback {
    Feedback {
        agent_id: "test-agent".into(),
        original_request: "request".into(),
        original_output: "out".into(),
        corrected_output: "corrected".into(),
        timestamp: 1_700_000_000,
        tags: tags.iter().map(|t| t.to_string()).collect(),
    }
}

#[tokio::test]
async fn extracts_pref_prefixed_tags_in_order() {
    let store = setup().await;
    let feedback = vec![fb_with_tags(&["pref:concise", "unrelated", "pref:format"])];

    let inferred = store
        .infer_preferences_from_feedback(feedback)
        .expect("inference should succeed");

    assert_eq!(inferred, vec!["concise", "format"]);
}

#[tokio::test]
async fn no_pref_prefixed_tags_returns_empty() {
    let store = setup().await;
    let feedback = vec![fb_with_tags(&["just-a-tag", "another-tag"])];

    let inferred = store
        .infer_preferences_from_feedback(feedback)
        .expect("inference should succeed");

    assert!(inferred.is_empty());
}

#[tokio::test]
async fn aggregates_across_multiple_feedback_entries() {
    let store = setup().await;
    let feedback = vec![
        fb_with_tags(&["pref:concise"]),
        fb_with_tags(&["pref:concise", "pref:verbose"]),
    ];

    // Actual behaviour: the implementation does NOT deduplicate. Each
    // `pref:`-tagged occurrence is pushed onto the result, in the order
    // it appears across the feedback list. (Doc claim of "first
    // occurrence wins" is aspirational; tracked in TODO.)
    let inferred = store
        .infer_preferences_from_feedback(feedback)
        .expect("inference should succeed");

    assert_eq!(inferred, vec!["concise", "concise", "verbose"]);
}

#[tokio::test]
async fn empty_feedback_returns_empty_vec() {
    let store = setup().await;
    let inferred = store
        .infer_preferences_from_feedback(vec![])
        .expect("empty input is valid");
    assert!(inferred.is_empty());
}

#[tokio::test]
async fn strip_prefix_does_not_touch_partial_matches() {
    // "preference:foo" must NOT be treated as a pref: tag (it does not
    // start with the literal "pref:" prefix).
    let store = setup().await;
    let feedback = vec![fb_with_tags(&["preference:foo", "prefix:bar"])];

    let inferred = store
        .infer_preferences_from_feedback(feedback)
        .expect("inference should succeed");

    assert!(inferred.is_empty());
}
