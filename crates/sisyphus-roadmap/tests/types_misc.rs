//! Integration tests for sisyphus-roadmap `types` module's
//! remaining pure data types: `RoadmapProblem`, `AdjustmentSuggestion`,
//! `MilestonePrediction`, `ProgressPrediction`, `RoadmapHealth`,
//! `DelayAlert`, `CostProjection`, `Bottleneck`.

use sisyphus_roadmap::types::{
    AdjustmentSuggestion, Bottleneck, CostProjection, DelayAlert, MilestonePrediction,
    ProgressPrediction, RoadmapHealth, RoadmapProblem,
};
use sisyphus_roadmap::types::ProblemSeverity;

// ---------------------------------------------------------------------------
// RoadmapProblem
// ---------------------------------------------------------------------------

#[test]
fn roadmap_problem_field_access() {
    let p = RoadmapProblem {
        severity: ProblemSeverity::Critical,
        message: "blocked".into(),
        milestone_id: Some("m1".into()),
        phase_id: Some("ph1".into()),
        goal_id: Some("g1".into()),
    };
    assert_eq!(p.message, "blocked");
    assert!(matches!(p.severity, ProblemSeverity::Critical));
    assert_eq!(p.milestone_id.as_deref(), Some("m1"));
}

#[test]
fn roadmap_problem_no_ids() {
    let p = RoadmapProblem {
        severity: ProblemSeverity::Warning,
        message: "x".into(),
        milestone_id: None,
        phase_id: None,
        goal_id: None,
    };
    assert!(p.milestone_id.is_none());
    assert!(p.phase_id.is_none());
    assert!(p.goal_id.is_none());
}

#[test]
fn roadmap_problem_serde() {
    let p = RoadmapProblem {
        severity: ProblemSeverity::Warning,
        message: "m".into(),
        milestone_id: None,
        phase_id: None,
        goal_id: None,
    };
    let j = serde_json::to_string(&p).unwrap();
    let back: RoadmapProblem = serde_json::from_str(&j).unwrap();
    assert_eq!(back.message, "m");
}

// ---------------------------------------------------------------------------
// AdjustmentSuggestion
// ---------------------------------------------------------------------------

#[test]
fn adjustment_suggestion_field_access() {
    let a = AdjustmentSuggestion {
        problem: "p".into(),
        action: "a".into(),
        target_goal_id: Some("g".into()),
    };
    assert_eq!(a.problem, "p");
    assert_eq!(a.action, "a");
    assert_eq!(a.target_goal_id.as_deref(), Some("g"));
}

#[test]
fn adjustment_suggestion_no_target() {
    let a = AdjustmentSuggestion {
        problem: "p".into(),
        action: "a".into(),
        target_goal_id: None,
    };
    assert!(a.target_goal_id.is_none());
}

// ---------------------------------------------------------------------------
// MilestonePrediction
// ---------------------------------------------------------------------------

#[test]
fn milestone_prediction_field_access() {
    let m = MilestonePrediction {
        name: "m".into(),
        progress_pct: 0.5,
        remaining_goals: 5,
        risk_level: "low".into(),
    };
    assert_eq!(m.name, "m");
    assert_eq!(m.remaining_goals, 5);
    assert_eq!(m.risk_level, "low");
}

#[test]
fn milestone_prediction_serde() {
    let m = MilestonePrediction {
        name: "x".into(),
        progress_pct: 0.0,
        remaining_goals: 0,
        risk_level: "high".into(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: MilestonePrediction = serde_json::from_str(&j).unwrap();
    assert_eq!(back.risk_level, "high");
}

// ---------------------------------------------------------------------------
// ProgressPrediction
// ---------------------------------------------------------------------------

#[test]
fn progress_prediction_field_access() {
    let p = ProgressPrediction {
        project_id: "p1".into(),
        project_name: "Project".into(),
        overall_progress_pct: 0.7,
        remaining_goals: 3,
        milestone_predictions: vec![],
        estimated_completion_order: vec!["m1".into(), "m2".into()],
        risk_level: "medium".into(),
    };
    assert_eq!(p.project_id, "p1");
    assert_eq!(p.milestone_predictions.len(), 0);
    assert_eq!(p.estimated_completion_order.len(), 2);
}

// ---------------------------------------------------------------------------
// RoadmapHealth
// ---------------------------------------------------------------------------

#[test]
fn roadmap_health_healthy() {
    let h = RoadmapHealth {
        project_id: "p".into(),
        healthy: true,
        blocked_milestones: vec![],
        stale_goals: vec![],
        recent_failures: 0,
        problems: vec![],
        suggestions: vec![],
    };
    assert!(h.healthy);
    assert!(h.problems.is_empty());
    assert!(h.suggestions.is_empty());
}

#[test]
fn roadmap_health_unhealthy() {
    let h = RoadmapHealth {
        project_id: "p".into(),
        healthy: false,
        blocked_milestones: vec!["m1".into()],
        stale_goals: vec!["g1".into()],
        recent_failures: 5,
        problems: vec![RoadmapProblem {
            severity: ProblemSeverity::Critical,
            message: "stuck".into(),
            milestone_id: Some("m1".into()),
            phase_id: None,
            goal_id: None,
        }],
        suggestions: vec![],
    };
    assert!(!h.healthy);
    assert_eq!(h.recent_failures, 5);
    assert_eq!(h.problems.len(), 1);
}

#[test]
fn roadmap_health_serde() {
    let h = RoadmapHealth {
        project_id: "p".into(),
        healthy: true,
        blocked_milestones: vec![],
        stale_goals: vec![],
        recent_failures: 0,
        problems: vec![],
        suggestions: vec![],
    };
    let j = serde_json::to_string(&h).unwrap();
    let back: RoadmapHealth = serde_json::from_str(&j).unwrap();
    assert!(back.healthy);
}

// ---------------------------------------------------------------------------
// DelayAlert
// ---------------------------------------------------------------------------

#[test]
fn delay_alert_field_access() {
    let a = DelayAlert {
        project_id: "p".into(),
        milestone_id: "m".into(),
        milestone_name: "M".into(),
        estimated_remaining_secs: 3600.0,
        ci_upper_secs: 5400.0,
        threshold_multiplier: 1.5,
        severity: "warning".into(),
        message: "delayed".into(),
    };
    assert_eq!(a.severity, "warning");
    assert_eq!(a.threshold_multiplier, 1.5);
}

#[test]
fn delay_alert_critical() {
    let a = DelayAlert {
        project_id: "p".into(),
        milestone_id: "m".into(),
        milestone_name: "M".into(),
        estimated_remaining_secs: 100.0,
        ci_upper_secs: 500.0,
        threshold_multiplier: 5.0,
        severity: "critical".into(),
        message: "very delayed".into(),
    };
    assert_eq!(a.severity, "critical");
}

// ---------------------------------------------------------------------------
// CostProjection
// ---------------------------------------------------------------------------

#[test]
fn cost_projection_field_access() {
    let c = CostProjection {
        total_goals: 10,
        completed_goals: 4,
        remaining_goals: 6,
        avg_cost_per_goal: 1.5,
        avg_duration_per_goal_secs: 60.0,
        projected_remaining_cost: 9.0,
        projected_remaining_duration_secs: 360.0,
        confidence: 0.8,
    };
    assert_eq!(c.remaining_goals, 6);
    assert_eq!(c.total_goals, 10);
    assert_eq!(c.completed_goals, 4);
}

#[test]
fn cost_projection_serde() {
    let c = CostProjection {
        total_goals: 0,
        completed_goals: 0,
        remaining_goals: 0,
        avg_cost_per_goal: 0.0,
        avg_duration_per_goal_secs: 0.0,
        projected_remaining_cost: 0.0,
        projected_remaining_duration_secs: 0.0,
        confidence: 0.0,
    };
    let j = serde_json::to_string(&c).unwrap();
    let back: CostProjection = serde_json::from_str(&j).unwrap();
    assert_eq!(back.total_goals, 0);
}

// ---------------------------------------------------------------------------
// Bottleneck
// ---------------------------------------------------------------------------

#[test]
fn bottleneck_field_access() {
    let b = Bottleneck {
        goal_id: "g".into(),
        description: "slow".into(),
        failure_count: 3,
        avg_duration_secs: 100.0,
        blocking_count: 2,
        severity: "high".into(),
    };
    assert_eq!(b.goal_id, "g");
    assert_eq!(b.severity, "high");
}

#[test]
fn bottleneck_serde() {
    let b = Bottleneck {
        goal_id: "g".into(),
        description: "d".into(),
        failure_count: 0,
        avg_duration_secs: 0.0,
        blocking_count: 0,
        severity: "low".into(),
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: Bottleneck = serde_json::from_str(&j).unwrap();
    assert_eq!(back.goal_id, "g");
}
