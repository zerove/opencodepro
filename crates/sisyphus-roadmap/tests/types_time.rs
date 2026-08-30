//! Integration tests for sisyphus-roadmap `types` module's
//! `MilestoneTimePrediction` and `CompletionTimePrediction` data types.

use sisyphus_roadmap::types::{CompletionTimePrediction, MilestoneTimePrediction};

// ---------------------------------------------------------------------------
// MilestoneTimePrediction
// ---------------------------------------------------------------------------

#[test]
fn milestone_time_prediction_field_access() {
    let m = MilestoneTimePrediction {
        milestone_id: "m1".into(),
        name: "Milestone 1".into(),
        estimated_remaining_secs: 3600.0,
        ci_lower_secs: 1800.0,
        ci_upper_secs: 5400.0,
        progress_pct: 0.5,
        remaining_goals: 5,
        risk_level: "medium".into(),
        velocity_goals_per_sec: 0.001,
    };
    assert_eq!(m.milestone_id, "m1");
    assert_eq!(m.name, "Milestone 1");
    assert_eq!(m.estimated_remaining_secs, 3600.0);
    assert!(m.ci_lower_secs < m.ci_upper_secs);
}

#[test]
fn milestone_time_prediction_done_risk() {
    let m = MilestoneTimePrediction {
        milestone_id: "m".into(),
        name: "n".into(),
        estimated_remaining_secs: 0.0,
        ci_lower_secs: 0.0,
        ci_upper_secs: 0.0,
        progress_pct: 1.0,
        remaining_goals: 0,
        risk_level: "done".into(),
        velocity_goals_per_sec: 0.0,
    };
    assert_eq!(m.risk_level, "done");
    assert_eq!(m.remaining_goals, 0);
}

#[test]
fn milestone_time_prediction_high_risk() {
    let m = MilestoneTimePrediction {
        milestone_id: "m".into(),
        name: "n".into(),
        estimated_remaining_secs: 1000.0,
        ci_lower_secs: 500.0,
        ci_upper_secs: 5000.0,
        progress_pct: 0.1,
        remaining_goals: 100,
        risk_level: "high".into(),
        velocity_goals_per_sec: 0.0001,
    };
    assert_eq!(m.risk_level, "high");
}

#[test]
fn milestone_time_prediction_serde() {
    let m = MilestoneTimePrediction {
        milestone_id: "m".into(),
        name: "n".into(),
        estimated_remaining_secs: 100.0,
        ci_lower_secs: 50.0,
        ci_upper_secs: 200.0,
        progress_pct: 0.5,
        remaining_goals: 3,
        risk_level: "low".into(),
        velocity_goals_per_sec: 0.01,
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: MilestoneTimePrediction = serde_json::from_str(&j).unwrap();
    assert_eq!(back.milestone_id, "m");
    assert_eq!(back.estimated_remaining_secs, 100.0);
}

#[test]
fn milestone_time_prediction_clone() {
    let a = MilestoneTimePrediction {
        milestone_id: "m".into(),
        name: "n".into(),
        estimated_remaining_secs: 1.0,
        ci_lower_secs: 1.0,
        ci_upper_secs: 1.0,
        progress_pct: 0.0,
        remaining_goals: 0,
        risk_level: "low".into(),
        velocity_goals_per_sec: 0.0,
    };
    let b = a.clone();
    assert_eq!(a.milestone_id, b.milestone_id);
}

// ---------------------------------------------------------------------------
// CompletionTimePrediction
// ---------------------------------------------------------------------------

#[test]
fn completion_time_prediction_field_access() {
    let c = CompletionTimePrediction {
        project_id: "p1".into(),
        project_name: "My Project".into(),
        overall_progress_pct: 0.3,
        remaining_goals: 7,
        estimated_total_remaining_secs: 7200.0,
        ci_lower_total_secs: 3600.0,
        ci_upper_total_secs: 14400.0,
        milestone_predictions: vec![],
        estimated_completion_order: vec!["m1".into()],
        risk_level: "medium".into(),
        overall_velocity_goals_per_sec: 0.001,
        has_sufficient_data: true,
    };
    assert_eq!(c.project_id, "p1");
    assert_eq!(c.remaining_goals, 7);
    assert!(c.has_sufficient_data);
}

#[test]
fn completion_time_prediction_no_sufficient_data() {
    let c = CompletionTimePrediction {
        project_id: "p".into(),
        project_name: "n".into(),
        overall_progress_pct: 0.0,
        remaining_goals: 100,
        estimated_total_remaining_secs: 0.0,
        ci_lower_total_secs: 0.0,
        ci_upper_total_secs: 0.0,
        milestone_predictions: vec![],
        estimated_completion_order: vec![],
        risk_level: "high".into(),
        overall_velocity_goals_per_sec: 0.0,
        has_sufficient_data: false,
    };
    assert!(!c.has_sufficient_data);
}

#[test]
fn completion_time_prediction_serde() {
    let c = CompletionTimePrediction {
        project_id: "p".into(),
        project_name: "n".into(),
        overall_progress_pct: 0.5,
        remaining_goals: 5,
        estimated_total_remaining_secs: 1000.0,
        ci_lower_total_secs: 500.0,
        ci_upper_total_secs: 2000.0,
        milestone_predictions: vec![],
        estimated_completion_order: vec![],
        risk_level: "low".into(),
        overall_velocity_goals_per_sec: 0.005,
        has_sufficient_data: true,
    };
    let j = serde_json::to_string(&c).unwrap();
    let back: CompletionTimePrediction = serde_json::from_str(&j).unwrap();
    assert_eq!(back.remaining_goals, 5);
    assert_eq!(back.overall_progress_pct, 0.5);
}

#[test]
fn completion_time_prediction_clone() {
    let a = CompletionTimePrediction {
        project_id: "p".into(),
        project_name: "n".into(),
        overall_progress_pct: 0.0,
        remaining_goals: 0,
        estimated_total_remaining_secs: 0.0,
        ci_lower_total_secs: 0.0,
        ci_upper_total_secs: 0.0,
        milestone_predictions: vec![],
        estimated_completion_order: vec![],
        risk_level: "low".into(),
        overall_velocity_goals_per_sec: 0.0,
        has_sufficient_data: false,
    };
    let b = a.clone();
    assert_eq!(a.project_id, b.project_id);
    assert_eq!(a.risk_level, b.risk_level);
}

#[test]
fn completion_time_prediction_velocity_zero() {
    let c = CompletionTimePrediction {
        project_id: "p".into(),
        project_name: "n".into(),
        overall_progress_pct: 0.0,
        remaining_goals: 0,
        estimated_total_remaining_secs: 0.0,
        ci_lower_total_secs: 0.0,
        ci_upper_total_secs: 0.0,
        milestone_predictions: vec![],
        estimated_completion_order: vec![],
        risk_level: "low".into(),
        overall_velocity_goals_per_sec: 0.0,
        has_sufficient_data: false,
    };
    assert_eq!(c.overall_velocity_goals_per_sec, 0.0);
}
