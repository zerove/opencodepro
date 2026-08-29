//! Integration tests for sisyphus-types `roadmap` module.
//!
//! Exercises the public `Milestone` / `Phase` / `GoalRef` /
//! `MilestoneStatus` / `Roadmap` / `Project` types. `Milestone`
//! has a `calculate_progress` method that returns the ratio of
//! completed goals across all phases.

use sisyphus_types::{
    GoalRef, Milestone, MilestoneStatus, Phase, Project, Roadmap,
};

fn goal(id: &str, status: MilestoneStatus) -> GoalRef {
    GoalRef {
        goal_id: id.to_string(),
        description: format!("description for {id}"),
        status,
    }
}

fn phase(id: &str, goals: Vec<GoalRef>) -> Phase {
    Phase {
        id: id.to_string(),
        name: format!("phase-{id}"),
        goals,
        depends_on: vec![],
    }
}

// ---------------------------------------------------------------------------
// MilestoneStatus Display
// ---------------------------------------------------------------------------

#[test]
fn milestone_status_display() {
    assert_eq!(MilestoneStatus::Pending.to_string(), "Pending");
    assert_eq!(MilestoneStatus::InProgress.to_string(), "InProgress");
    assert_eq!(MilestoneStatus::Completed.to_string(), "Completed");
    assert_eq!(MilestoneStatus::Blocked.to_string(), "Blocked");
}

#[test]
fn milestone_status_partial_eq() {
    assert_eq!(MilestoneStatus::Pending, MilestoneStatus::Pending);
    assert_ne!(MilestoneStatus::Pending, MilestoneStatus::Completed);
}

// ---------------------------------------------------------------------------
// Milestone::calculate_progress
// ---------------------------------------------------------------------------

#[test]
fn progress_no_phases_is_zero() {
    let m = Milestone {
        id: "m".into(),
        name: "empty".into(),
        phases: vec![],
        status: MilestoneStatus::Pending,
    };
    assert_eq!(m.calculate_progress(), 0.0);
}

#[test]
fn progress_no_goals_is_zero() {
    let m = Milestone {
        id: "m".into(),
        name: "no-goals".into(),
        phases: vec![phase("p1", vec![])],
        status: MilestoneStatus::Pending,
    };
    assert_eq!(m.calculate_progress(), 0.0);
}

#[test]
fn progress_all_completed_is_one() {
    let m = Milestone {
        id: "m".into(),
        name: "done".into(),
        phases: vec![phase(
            "p1",
            vec![
                goal("g1", MilestoneStatus::Completed),
                goal("g2", MilestoneStatus::Completed),
            ],
        )],
        status: MilestoneStatus::Completed,
    };
    assert!((m.calculate_progress() - 1.0).abs() < 1e-9);
}

#[test]
fn progress_partial_across_phases() {
    let m = Milestone {
        id: "m".into(),
        name: "partial".into(),
        phases: vec![
            phase(
                "p1",
                vec![
                    goal("g1", MilestoneStatus::Completed),
                    goal("g2", MilestoneStatus::Completed),
                ],
            ),
            phase(
                "p2",
                vec![
                    goal("g3", MilestoneStatus::Completed),
                    goal("g4", MilestoneStatus::Pending),
                ],
            ),
        ],
        status: MilestoneStatus::InProgress,
    };
    // 3 of 4 goals completed.
    assert!((m.calculate_progress() - 0.75).abs() < 1e-9);
}

#[test]
fn progress_excludes_in_progress_and_blocked() {
    let m = Milestone {
        id: "m".into(),
        name: "mixed".into(),
        phases: vec![phase(
            "p1",
            vec![
                goal("g1", MilestoneStatus::Completed),
                goal("g2", MilestoneStatus::InProgress),
                goal("g3", MilestoneStatus::Blocked),
                goal("g4", MilestoneStatus::Pending),
            ],
        )],
        status: MilestoneStatus::InProgress,
    };
    // 1 of 4 completed.
    assert!((m.calculate_progress() - 0.25).abs() < 1e-9);
}

#[test]
fn progress_all_pending_is_zero() {
    let m = Milestone {
        id: "m".into(),
        name: "all-pending".into(),
        phases: vec![phase(
            "p1",
            vec![
                goal("g1", MilestoneStatus::Pending),
                goal("g2", MilestoneStatus::Pending),
            ],
        )],
        status: MilestoneStatus::Pending,
    };
    assert_eq!(m.calculate_progress(), 0.0);
}

// ---------------------------------------------------------------------------
// Project::new
// ---------------------------------------------------------------------------

#[test]
fn project_new_creates_empty_roadmap() {
    let p = Project::new("p1".into(), "My Project".into(), "A test project".into());
    assert_eq!(p.id, "p1");
    assert_eq!(p.name, "My Project");
    assert_eq!(p.description, "A test project");
    assert!(p.roadmap.milestones.is_empty());
}

#[test]
fn project_new_allows_chained_milestone_addition() {
    let mut p = Project::new("p1".into(), "P".into(), "D".into());
    p.roadmap.milestones.push(Milestone {
        id: "m1".into(),
        name: "First".into(),
        phases: vec![],
        status: MilestoneStatus::Pending,
    });
    assert_eq!(p.roadmap.milestones.len(), 1);
    assert_eq!(p.roadmap.milestones[0].id, "m1");
}

// ---------------------------------------------------------------------------
// Roadmap
// ---------------------------------------------------------------------------

#[test]
fn roadmap_default_is_empty() {
    let r = Roadmap { milestones: vec![] };
    assert!(r.milestones.is_empty());
}

// ---------------------------------------------------------------------------
// Phase
// ---------------------------------------------------------------------------

#[test]
fn phase_carries_dependencies() {
    let p = Phase {
        id: "p1".into(),
        name: "phase-one".into(),
        goals: vec![goal("g1", MilestoneStatus::Pending)],
        depends_on: vec!["p0".into()],
    };
    assert_eq!(p.depends_on, vec!["p0".to_string()]);
    assert_eq!(p.goals.len(), 1);
}
