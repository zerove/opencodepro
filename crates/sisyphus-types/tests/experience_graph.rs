//! Integration tests for sisyphus-types `experience` and
//! `graph` (smaller data-only types) modules.
//!
//! Exercises the pure-data surfaces of [`ExperienceTuple`],
//! [`ExperienceQuery`], [`SopEvaluation`], and
//! [`MicroReflectionDecision`].

use sisyphus_types::experience::{ExperienceQuery, ExperienceTuple};
use sisyphus_types::graph::{MicroReflectionDecision, SopEvaluation};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// ExperienceTuple
// ---------------------------------------------------------------------------

#[test]
fn experience_tuple_field_assignment() {
    let t = ExperienceTuple {
        context_text: "build a thing".into(),
        context_hash: "abc".into(),
        action_id: "act-1".into(),
        project_id: Some("p1".into()),
        outcome_success: true,
        deviation_score: 0.1,
        cost: 100,
        timestamp: 12345,
        embedding: None,
        visual_embedding: None,
        insight: Some("a tip".into()),
        version: 0,
        vector_clock: std::collections::BTreeMap::new(),
    };
    assert_eq!(t.context_text, "build a thing");
    assert!(t.outcome_success);
    assert_eq!(t.deviation_score, 0.1);
    assert_eq!(t.cost, 100);
    assert_eq!(t.project_id.as_deref(), Some("p1"));
    assert!(t.embedding.is_none());
    assert_eq!(t.insight.as_deref(), Some("a tip"));
}

#[test]
fn experience_tuple_serde_round_trip() {
    let t = ExperienceTuple {
        context_text: "ctx".into(),
        context_hash: "h".into(),
        action_id: "a".into(),
        project_id: None,
        outcome_success: false,
        deviation_score: 0.5,
        cost: 0,
        timestamp: 1,
        embedding: None,
        visual_embedding: None,
        insight: None,
        version: 0,
        vector_clock: std::collections::BTreeMap::new(),
    };
    let j = serde_json::to_string(&t).unwrap();
    let back: ExperienceTuple = serde_json::from_str(&j).unwrap();
    assert_eq!(back.context_text, "ctx");
    assert!(!back.outcome_success);
}

// ---------------------------------------------------------------------------
// ExperienceQuery
// ---------------------------------------------------------------------------

#[test]
fn experience_query_field_assignment() {
    let q = ExperienceQuery {
        query_text: "search".into(),
        context_hash: "h".into(),
        project_id: Some("p1".into()),
        action_id: None,
        visual_embedding: None,
        limit: 10,
    };
    assert_eq!(q.query_text, "search");
    assert_eq!(q.limit, 10);
    assert!(q.action_id.is_none());
}

#[test]
fn experience_query_serde_round_trip() {
    let q = ExperienceQuery {
        query_text: "q".into(),
        context_hash: "h".into(),
        project_id: None,
        action_id: Some("a".into()),
        visual_embedding: None,
        limit: 5,
    };
    let j = serde_json::to_string(&q).unwrap();
    let back: ExperienceQuery = serde_json::from_str(&j).unwrap();
    assert_eq!(back.limit, 5);
    assert_eq!(back.action_id.as_deref(), Some("a"));
}

// ---------------------------------------------------------------------------
// SopEvaluation
// ---------------------------------------------------------------------------

#[test]
fn sop_evaluation_field_assignment() {
    let mut metrics = HashMap::new();
    metrics.insert("accuracy".to_string(), 0.95);
    let e = SopEvaluation {
        score: 0.8,
        metrics,
        feedback: "looks good".into(),
        is_acceptable: true,
    };
    assert!((e.score - 0.8).abs() < 1e-6);
    assert!(e.is_acceptable);
    assert_eq!(e.metrics.get("accuracy"), Some(&0.95));
}

#[test]
fn sop_evaluation_serde_round_trip() {
    let e = SopEvaluation {
        score: 0.5,
        metrics: HashMap::new(),
        feedback: "x".into(),
        is_acceptable: false,
    };
    let j = serde_json::to_string(&e).unwrap();
    let back: SopEvaluation = serde_json::from_str(&j).unwrap();
    assert!((back.score - 0.5).abs() < 1e-6);
    assert!(!back.is_acceptable);
}

// ---------------------------------------------------------------------------
// MicroReflectionDecision
// ---------------------------------------------------------------------------

#[test]
fn micro_reflection_decision_continue() {
    let d = MicroReflectionDecision::Continue;
    let j = serde_json::to_string(&d).unwrap();
    assert!(j.contains("Continue"));
    let back: MicroReflectionDecision = serde_json::from_str(&j).unwrap();
    matches!(back, MicroReflectionDecision::Continue);
}

#[test]
fn micro_reflection_decision_adjust() {
    let mut updates = HashMap::new();
    updates.insert("k".to_string(), serde_json::json!(1));
    let d = MicroReflectionDecision::Adjust { updates };
    let j = serde_json::to_string(&d).unwrap();
    assert!(j.contains("Adjust"));
    let back: MicroReflectionDecision = serde_json::from_str(&j).unwrap();
    match back {
        MicroReflectionDecision::Adjust { updates } => {
            assert_eq!(updates.get("k"), Some(&serde_json::json!(1)));
        },
        _ => panic!("expected Adjust"),
    }
}

#[test]
fn micro_reflection_decision_pivot() {
    let d = MicroReflectionDecision::Pivot {
        reason: "no progress".into(),
    };
    let j = serde_json::to_string(&d).unwrap();
    let back: MicroReflectionDecision = serde_json::from_str(&j).unwrap();
    match back {
        MicroReflectionDecision::Pivot { reason } => assert_eq!(reason, "no progress"),
        _ => panic!("expected Pivot"),
    }
}

#[test]
fn micro_reflection_decision_variants_distinct() {
    let d1 = MicroReflectionDecision::Continue;
    let d2 = MicroReflectionDecision::Pivot { reason: "x".into() };
    // Compare as Debug strings (Decision has no PartialEq derived).
    assert_ne!(format!("{d1:?}"), format!("{d2:?}"));
}
