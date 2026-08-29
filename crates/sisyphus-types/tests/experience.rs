//! Integration tests for sisyphus-types `experience` types.
//!
//! Exercises the public `ExperienceTuple` and `ExperienceQuery`
//! types — the data structures that flow through the experience
//! bank. The bank is queried at hot-start and after each
//! iteration to find similar past experiences.

use sisyphus_types::experience::{ExperienceQuery, ExperienceTuple};

fn make_tuple(success: bool) -> ExperienceTuple {
    ExperienceTuple {
        context_text: "user wanted refactor".into(),
        context_hash: "abc123".into(),
        action_id: "sop-1".into(),
        project_id: Some("proj-1".into()),
        outcome_success: success,
        deviation_score: 0.05,
        cost: 100,
        timestamp: 1_700_000_000,
        embedding: None,
        visual_embedding: None,
        insight: Some("lesson learned".into()),
    }
}

// ---------------------------------------------------------------------------
// ExperienceTuple
// ---------------------------------------------------------------------------

#[test]
fn tuple_with_all_fields_serialization() {
    let t = make_tuple(true);
    let json = serde_json::to_string(&t).expect("");
    let parsed: ExperienceTuple = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.context_text, "user wanted refactor");
    assert_eq!(parsed.context_hash, "abc123");
    assert_eq!(parsed.action_id, "sop-1");
    assert_eq!(parsed.project_id.as_deref(), Some("proj-1"));
    assert!(parsed.outcome_success);
    assert!((parsed.deviation_score - 0.05).abs() < 1e-9);
    assert_eq!(parsed.cost, 100);
    assert_eq!(parsed.timestamp, 1_700_000_000);
    assert!(parsed.insight.is_some());
}

#[test]
fn tuple_with_failure_outcome() {
    let t = make_tuple(false);
    let json = serde_json::to_string(&t).expect("");
    let parsed: ExperienceTuple = serde_json::from_str(&json).expect("");
    assert!(!parsed.outcome_success);
}

#[test]
fn tuple_without_project_id() {
    let mut t = make_tuple(true);
    t.project_id = None;
    let json = serde_json::to_string(&t).expect("");
    let parsed: ExperienceTuple = serde_json::from_str(&json).expect("");
    assert!(parsed.project_id.is_none());
}

#[test]
fn tuple_without_insight() {
    let mut t = make_tuple(true);
    t.insight = None;
    let json = serde_json::to_string(&t).expect("");
    let parsed: ExperienceTuple = serde_json::from_str(&json).expect("");
    assert!(parsed.insight.is_none());
}

#[test]
fn tuple_with_embedding() {
    let mut t = make_tuple(true);
    t.embedding = Some(vec![0.1, 0.2, 0.3]);
    t.visual_embedding = Some(vec![0.4, 0.5]);
    let json = serde_json::to_string(&t).expect("");
    let parsed: ExperienceTuple = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.embedding.expect("").len(), 3);
    assert_eq!(parsed.visual_embedding.expect("").len(), 2);
}

#[test]
fn tuple_clone_preserves_all_fields() {
    let t = make_tuple(true);
    let cloned = t.clone();
    assert_eq!(cloned.context_text, t.context_text);
    assert_eq!(cloned.action_id, t.action_id);
    assert_eq!(cloned.outcome_success, t.outcome_success);
}

// ---------------------------------------------------------------------------
// ExperienceQuery
// ---------------------------------------------------------------------------

#[test]
fn query_serialization_roundtrip() {
    let q = ExperienceQuery {
        query_text: "how to refactor".into(),
        context_hash: "h1".into(),
        project_id: Some("p1".into()),
        action_id: Some("a1".into()),
        visual_embedding: None,
        limit: 10,
    };
    let json = serde_json::to_string(&q).expect("");
    let parsed: ExperienceQuery = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.query_text, "how to refactor");
    assert_eq!(parsed.context_hash, "h1");
    assert_eq!(parsed.project_id.as_deref(), Some("p1"));
    assert_eq!(parsed.action_id.as_deref(), Some("a1"));
    assert_eq!(parsed.limit, 10);
}

#[test]
fn query_with_optional_fields_none() {
    let q = ExperienceQuery {
        query_text: "x".into(),
        context_hash: "y".into(),
        project_id: None,
        action_id: None,
        visual_embedding: None,
        limit: 5,
    };
    let json = serde_json::to_string(&q).expect("");
    let parsed: ExperienceQuery = serde_json::from_str(&json).expect("");
    assert!(parsed.project_id.is_none());
    assert!(parsed.action_id.is_none());
    assert!(parsed.visual_embedding.is_none());
}

#[test]
fn query_with_visual_embedding() {
    let q = ExperienceQuery {
        query_text: "x".into(),
        context_hash: "y".into(),
        project_id: None,
        action_id: None,
        visual_embedding: Some(vec![0.5, 0.6]),
        limit: 1,
    };
    let json = serde_json::to_string(&q).expect("");
    let parsed: ExperienceQuery = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.visual_embedding.expect("").len(), 2);
    assert_eq!(parsed.limit, 1);
}

#[test]
fn query_zero_limit_serializes() {
    let q = ExperienceQuery {
        query_text: "x".into(),
        context_hash: "y".into(),
        project_id: None,
        action_id: None,
        visual_embedding: None,
        limit: 0,
    };
    let json = serde_json::to_string(&q).expect("");
    let parsed: ExperienceQuery = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.limit, 0);
}

#[test]
fn query_clone_preserves_all_fields() {
    let q = ExperienceQuery {
        query_text: "x".into(),
        context_hash: "y".into(),
        project_id: Some("p".into()),
        action_id: Some("a".into()),
        visual_embedding: Some(vec![1.0]),
        limit: 5,
    };
    let cloned = q.clone();
    assert_eq!(cloned.query_text, "x");
    assert_eq!(cloned.limit, 5);
    assert!(cloned.visual_embedding.is_some());
}
