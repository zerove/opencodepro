//! Integration tests for sisyphus-types `roadmap` module.
//!
//! Exercises the public data types and the [`Milestone::calculate_progress`]
//! pure function.

use sisyphus_types::roadmap::{
    GoalRef, Milestone, MilestoneStatus, Phase, Project, Roadmap,
};

// ---------------------------------------------------------------------------
// MilestoneStatus
// ---------------------------------------------------------------------------

#[test]
fn milestone_status_variants_distinct() {
    assert_ne!(MilestoneStatus::Pending, MilestoneStatus::InProgress);
    assert_ne!(MilestoneStatus::InProgress, MilestoneStatus::Completed);
    assert_ne!(MilestoneStatus::Completed, MilestoneStatus::Blocked);
}

#[test]
fn milestone_status_serde() {
    for s in [
        MilestoneStatus::Pending,
        MilestoneStatus::InProgress,
        MilestoneStatus::Completed,
        MilestoneStatus::Blocked,
    ] {
        let j = serde_json::to_string(&s).unwrap();
        let back: MilestoneStatus = serde_json::from_str(&j).unwrap();
        assert_eq!(back, s);
    }
}

// ---------------------------------------------------------------------------
// Milestone::calculate_progress
// ---------------------------------------------------------------------------

fn goal(id: &str, status: MilestoneStatus) -> GoalRef {
    GoalRef {
        goal_id: id.to_string(),
        description: format!("goal {id}"),
        status,
    }
}

fn milestone(id: &str, phases: Vec<Phase>) -> Milestone {
    Milestone {
        id: id.to_string(),
        name: format!("milestone {id}"),
        phases,
        status: MilestoneStatus::InProgress,
    }
}

#[test]
fn calculate_progress_empty_milestone() {
    let m = milestone("m1", vec![]);
    assert_eq!(m.calculate_progress(), 0.0);
}

#[test]
fn calculate_progress_no_completed() {
    let m = milestone(
        "m1",
        vec![Phase {
            id: "p1".into(),
            name: "p".into(),
            goals: vec![
                goal("a", MilestoneStatus::Pending),
                goal("b", MilestoneStatus::Pending),
            ],
            depends_on: vec![],
        }],
    );
    assert_eq!(m.calculate_progress(), 0.0);
}

#[test]
fn calculate_progress_all_completed() {
    let m = milestone(
        "m1",
        vec![Phase {
            id: "p1".into(),
            name: "p".into(),
            goals: vec![
                goal("a", MilestoneStatus::Completed),
                goal("b", MilestoneStatus::Completed),
            ],
            depends_on: vec![],
        }],
    );
    assert!((m.calculate_progress() - 1.0).abs() < 1e-9);
}

#[test]
fn calculate_progress_half_completed() {
    let m = milestone(
        "m1",
        vec![Phase {
            id: "p1".into(),
            name: "p".into(),
            goals: vec![
                goal("a", MilestoneStatus::Completed),
                goal("b", MilestoneStatus::Pending),
            ],
            depends_on: vec![],
        }],
    );
    assert!((m.calculate_progress() - 0.5).abs() < 1e-9);
}

#[test]
fn calculate_progress_across_phases() {
    let m = milestone(
        "m1",
        vec![
            Phase {
                id: "p1".into(),
                name: "p1".into(),
                goals: vec![
                    goal("a", MilestoneStatus::Completed),
                    goal("b", MilestoneStatus::Completed),
                ],
                depends_on: vec![],
            },
            Phase {
                id: "p2".into(),
                name: "p2".into(),
                goals: vec![goal("c", MilestoneStatus::Pending)],
                depends_on: vec![],
            },
        ],
    );
    // 2 out of 3 completed = 0.6666...
    assert!((m.calculate_progress() - 2.0 / 3.0).abs() < 1e-9);
}

#[test]
fn calculate_progress_in_progress_counts_as_incomplete() {
    let m = milestone(
        "m1",
        vec![Phase {
            id: "p1".into(),
            name: "p".into(),
            goals: vec![goal("a", MilestoneStatus::InProgress)],
            depends_on: vec![],
        }],
    );
    // InProgress is not Completed → 0.0.
    assert_eq!(m.calculate_progress(), 0.0);
}

#[test]
fn calculate_progress_blocked_counts_as_incomplete() {
    let m = milestone(
        "m1",
        vec![Phase {
            id: "p1".into(),
            name: "p".into(),
            goals: vec![goal("a", MilestoneStatus::Blocked)],
            depends_on: vec![],
        }],
    );
    // Blocked is not Completed → 0.0.
    assert_eq!(m.calculate_progress(), 0.0);
}

// ---------------------------------------------------------------------------
// Project / Roadmap
// ---------------------------------------------------------------------------

#[test]
fn project_new_constructs() {
    let p = Project::new("p1".into(), "P1".into(), "first project".into());
    assert_eq!(p.id, "p1");
    assert_eq!(p.name, "P1");
    assert_eq!(p.description, "first project");
    assert!(p.roadmap.milestones.is_empty());
}

#[test]
fn roadmap_serde_round_trip() {
    let r = Roadmap {
        milestones: vec![milestone(
            "m1",
            vec![Phase {
                id: "p1".into(),
                name: "p".into(),
                goals: vec![goal("a", MilestoneStatus::Completed)],
                depends_on: vec![],
            }],
        )],
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: Roadmap = serde_json::from_str(&j).unwrap();
    assert_eq!(back.milestones.len(), 1);
    assert_eq!(back.milestones[0].id, "m1");
}

// ---------------------------------------------------------------------------
// GoalRef
// ---------------------------------------------------------------------------

#[test]
fn goal_ref_field_assignment() {
    let g = GoalRef {
        goal_id: "g1".into(),
        description: "do the thing".into(),
        status: MilestoneStatus::Completed,
    };
    assert_eq!(g.goal_id, "g1");
    assert_eq!(g.status, MilestoneStatus::Completed);
}

// ---------------------------------------------------------------------------
// Phase
// ---------------------------------------------------------------------------

#[test]
fn phase_field_assignment() {
    let p = Phase {
        id: "p1".into(),
        name: "phase 1".into(),
        goals: vec![],
        depends_on: vec!["p0".into()],
    };
    assert_eq!(p.depends_on.len(), 1);
    assert_eq!(p.depends_on[0], "p0");
}
