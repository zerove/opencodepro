//! Integration tests for sisyphus-roadmap `types` module's
//! `ParallelizationOpportunity` and `GoalResourceEstimate` data types.

use sisyphus_roadmap::types::{GoalResourceEstimate, ParallelizationOpportunity};

// ---------------------------------------------------------------------------
// ParallelizationOpportunity
// ---------------------------------------------------------------------------

#[test]
fn parallel_opportunity_field_access() {
    let p = ParallelizationOpportunity {
        goal_ids: vec!["g1".into(), "g2".into(), "g3".into()],
        rationale: "no dependencies".into(),
        estimated_time_savings_secs: 60.0,
    };
    assert_eq!(p.goal_ids.len(), 3);
    assert_eq!(p.rationale, "no dependencies");
    assert_eq!(p.estimated_time_savings_secs, 60.0);
}

#[test]
fn parallel_opportunity_empty_goals() {
    let p = ParallelizationOpportunity {
        goal_ids: vec![],
        rationale: "r".into(),
        estimated_time_savings_secs: 0.0,
    };
    assert!(p.goal_ids.is_empty());
    assert_eq!(p.estimated_time_savings_secs, 0.0);
}

#[test]
fn parallel_opportunity_serde() {
    let p = ParallelizationOpportunity {
        goal_ids: vec!["g".into()],
        rationale: "r".into(),
        estimated_time_savings_secs: 1.0,
    };
    let j = serde_json::to_string(&p).unwrap();
    let back: ParallelizationOpportunity = serde_json::from_str(&j).unwrap();
    assert_eq!(back.goal_ids, vec!["g"]);
}

#[test]
fn parallel_opportunity_clone() {
    let a = ParallelizationOpportunity {
        goal_ids: vec!["g1".into()],
        rationale: "r".into(),
        estimated_time_savings_secs: 10.0,
    };
    let b = a.clone();
    assert_eq!(a.goal_ids, b.goal_ids);
    assert_eq!(a.estimated_time_savings_secs, b.estimated_time_savings_secs);
}

#[test]
fn parallel_opportunity_zero_savings() {
    let p = ParallelizationOpportunity {
        goal_ids: vec!["a".into(), "b".into()],
        rationale: "parallel but no saving".into(),
        estimated_time_savings_secs: 0.0,
    };
    assert_eq!(p.estimated_time_savings_secs, 0.0);
}

// ---------------------------------------------------------------------------
// GoalResourceEstimate
// ---------------------------------------------------------------------------

#[test]
fn goal_resource_estimate_field_access() {
    let g = GoalResourceEstimate {
        goal_id: "g1".into(),
        predicted_cost: 1.5,
        predicted_duration_secs: 60.0,
        confidence: 0.8,
        historical_samples: 5,
    };
    assert_eq!(g.goal_id, "g1");
    assert_eq!(g.predicted_cost, 1.5);
    assert_eq!(g.predicted_duration_secs, 60.0);
    assert_eq!(g.historical_samples, 5);
}

#[test]
fn goal_resource_estimate_zero_confidence() {
    let g = GoalResourceEstimate {
        goal_id: "g".into(),
        predicted_cost: 0.0,
        predicted_duration_secs: 0.0,
        confidence: 0.0,
        historical_samples: 0,
    };
    assert_eq!(g.confidence, 0.0);
    assert_eq!(g.historical_samples, 0);
}

#[test]
fn goal_resource_estimate_full_confidence() {
    let g = GoalResourceEstimate {
        goal_id: "g".into(),
        predicted_cost: 100.0,
        predicted_duration_secs: 3600.0,
        confidence: 1.0,
        historical_samples: 100,
    };
    assert_eq!(g.confidence, 1.0);
}

#[test]
fn goal_resource_estimate_serde() {
    let g = GoalResourceEstimate {
        goal_id: "g".into(),
        predicted_cost: 2.5,
        predicted_duration_secs: 30.0,
        confidence: 0.5,
        historical_samples: 3,
    };
    let j = serde_json::to_string(&g).unwrap();
    let back: GoalResourceEstimate = serde_json::from_str(&j).unwrap();
    assert_eq!(back.goal_id, "g");
    assert_eq!(back.historical_samples, 3);
}

#[test]
fn goal_resource_estimate_clone() {
    let a = GoalResourceEstimate {
        goal_id: "g".into(),
        predicted_cost: 1.0,
        predicted_duration_secs: 10.0,
        confidence: 0.5,
        historical_samples: 2,
    };
    let b = a.clone();
    assert_eq!(a.goal_id, b.goal_id);
    assert_eq!(a.predicted_cost, b.predicted_cost);
}
