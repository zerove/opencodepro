//! Integration tests for sisyphus-roadmap `types` module's
//! pure data types: `GoalProgress`, `PhaseProgress`, `MilestoneProgress`,
//! `ProgressReport`, `ExecutionOutcome`, `ProblemSeverity`.

use sisyphus_roadmap::types::{
    ExecutionOutcome, GoalProgress, MilestoneProgress, PhaseProgress, ProblemSeverity,
    ProgressReport,
};
use sisyphus_types::roadmap::MilestoneStatus;

// ---------------------------------------------------------------------------
// GoalProgress
// ---------------------------------------------------------------------------

#[test]
fn goal_progress_field_access() {
    let g = GoalProgress {
        goal_id: "g1".into(),
        description: "d".into(),
        status: MilestoneStatus::Completed,
    };
    assert_eq!(g.goal_id, "g1");
    assert_eq!(g.description, "d");
    assert_eq!(g.status, MilestoneStatus::Completed);
}

#[test]
fn goal_progress_clone() {
    let a = GoalProgress {
        goal_id: "g".into(),
        description: "d".into(),
        status: MilestoneStatus::Pending,
    };
    let b = a.clone();
    assert_eq!(a.goal_id, b.goal_id);
    assert_eq!(a.status, b.status);
}

#[test]
fn goal_progress_serde() {
    let g = GoalProgress {
        goal_id: "g".into(),
        description: "d".into(),
        status: MilestoneStatus::InProgress,
    };
    let j = serde_json::to_string(&g).unwrap();
    let back: GoalProgress = serde_json::from_str(&j).unwrap();
    assert_eq!(back.goal_id, "g");
}

// ---------------------------------------------------------------------------
// PhaseProgress
// ---------------------------------------------------------------------------

#[test]
fn phase_progress_field_access() {
    let p = PhaseProgress {
        phase_id: "p1".into(),
        name: "phase1".into(),
        goal_count: 5,
        completed_count: 3,
        progress: 0.6,
        goals: vec![],
    };
    assert_eq!(p.phase_id, "p1");
    assert_eq!(p.goal_count, 5);
    assert_eq!(p.completed_count, 3);
    assert!((p.progress - 0.6).abs() < 1e-9);
}

#[test]
fn phase_progress_empty() {
    let p = PhaseProgress {
        phase_id: "p".into(),
        name: "n".into(),
        goal_count: 0,
        completed_count: 0,
        progress: 0.0,
        goals: vec![],
    };
    assert!(p.goals.is_empty());
    assert_eq!(p.progress, 0.0);
}

#[test]
fn phase_progress_clone() {
    let a = PhaseProgress {
        phase_id: "p".into(),
        name: "n".into(),
        goal_count: 1,
        completed_count: 1,
        progress: 1.0,
        goals: vec![],
    };
    let _b = a.clone();
}

// ---------------------------------------------------------------------------
// MilestoneProgress
// ---------------------------------------------------------------------------

#[test]
fn milestone_progress_field_access() {
    let m = MilestoneProgress {
        milestone_id: "m1".into(),
        name: "milestone1".into(),
        status: MilestoneStatus::InProgress,
        phase_count: 3,
        goal_count: 10,
        completed_count: 4,
        progress: 0.4,
        phases: vec![],
    };
    assert_eq!(m.milestone_id, "m1");
    assert_eq!(m.phase_count, 3);
    assert!((m.progress - 0.4).abs() < 1e-9);
}

#[test]
fn milestone_progress_empty() {
    let m = MilestoneProgress {
        milestone_id: "m".into(),
        name: "n".into(),
        status: MilestoneStatus::Pending,
        phase_count: 0,
        goal_count: 0,
        completed_count: 0,
        progress: 0.0,
        phases: vec![],
    };
    assert!(m.phases.is_empty());
}

// ---------------------------------------------------------------------------
// ProgressReport
// ---------------------------------------------------------------------------

#[test]
fn progress_report_field_access() {
    let r = ProgressReport {
        project_id: "p1".into(),
        project_name: "MyProject".into(),
        overall_progress: 0.5,
        milestone_count: 3,
        total_goals: 20,
        completed_goals: 10,
        milestones: vec![],
    };
    assert_eq!(r.project_id, "p1");
    assert!((r.overall_progress - 0.5).abs() < 1e-9);
    assert_eq!(r.total_goals, 20);
    assert_eq!(r.completed_goals, 10);
}

#[test]
fn progress_report_empty() {
    let r = ProgressReport {
        project_id: "p".into(),
        project_name: "n".into(),
        overall_progress: 0.0,
        milestone_count: 0,
        total_goals: 0,
        completed_goals: 0,
        milestones: vec![],
    };
    assert!(r.milestones.is_empty());
}

// ---------------------------------------------------------------------------
// ExecutionOutcome
// ---------------------------------------------------------------------------

#[test]
fn execution_outcome_success() {
    let o = ExecutionOutcome {
        goal_id: "g1".into(),
        project_id: "p1".into(),
        milestone_id: "m1".into(),
        phase_id: "ph1".into(),
        success: true,
        deviation_score: Some(0.1),
        error: None,
        timestamp: 1234,
            actual_cost: 0.0,
        duration_secs: 0.0,
    };
    assert!(o.success);
    assert!(o.error.is_none());
    assert_eq!(o.deviation_score, Some(0.1));
}

#[test]
fn execution_outcome_failure() {
    let o = ExecutionOutcome {
        goal_id: "g1".into(),
        project_id: "p1".into(),
        milestone_id: "m1".into(),
        phase_id: "ph1".into(),
        success: false,
        deviation_score: Some(1.0),
        error: Some("timeout".into()),
        timestamp: 1000,
            actual_cost: 0.0,
        duration_secs: 0.0,
    };
    assert!(!o.success);
    assert_eq!(o.error.as_deref(), Some("timeout"));
}

#[test]
fn execution_outcome_no_deviation() {
    let o = ExecutionOutcome {
        goal_id: "g".into(),
        project_id: "p".into(),
        milestone_id: "m".into(),
        phase_id: "ph".into(),
        success: true,
        deviation_score: None,
        error: None,
        timestamp: 0,
            actual_cost: 0.0,
        duration_secs: 0.0,
    };
    assert!(o.deviation_score.is_none());
}

// ---------------------------------------------------------------------------
// ProblemSeverity
// ---------------------------------------------------------------------------

#[test]
fn problem_severity_variants() {
    let _ = ProblemSeverity::Warning;
    let _ = ProblemSeverity::Critical;
}

#[test]
fn problem_severity_distinct() {
    // Just check distinct Debug
    let a = format!("{:?}", ProblemSeverity::Warning);
    let b = format!("{:?}", ProblemSeverity::Critical);
    assert_ne!(a, b);
}
