//! Integration tests for sisyphus-types `roadmap` module.
//!
//! Exercises the public data types and the [`Milestone::calculate_progress`]
//! pure function.

use sisyphus_types::roadmap::{GoalRef, Milestone, MilestoneStatus, Phase, Project, Roadmap};

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

// ---------------------------------------------------------------------------
// Project and additional coverage
// ---------------------------------------------------------------------------

#[test]
fn project_new_constructs_empty_roadmap() {
    use sisyphus_types::roadmap::Project;
    let p = Project::new("p1".into(), "Test".into(), "A test project".into());
    assert_eq!(p.id, "p1");
    assert_eq!(p.name, "Test");
    assert!(p.roadmap.milestones.is_empty());
}

#[test]
fn project_unicode_fields() {
    use sisyphus_types::roadmap::Project;
    let p = Project::new("项目-α".into(), "项目名".into(), "描述-β".into());
    assert_eq!(p.id, "项目-α");
    assert_eq!(p.description, "描述-β");
}

#[test]
fn project_serde() {
    use sisyphus_types::roadmap::Project;
    let p = Project::new("p".into(), "n".into(), "d".into());
    let j = serde_json::to_string(&p).unwrap();
    let back: Project = serde_json::from_str(&j).unwrap();
    assert_eq!(back.id, "p");
    assert_eq!(back.name, "n");
}

#[test]
fn project_clone() {
    use sisyphus_types::roadmap::Project;
    let p = Project::new("p".into(), "n".into(), "d".into());
    let cloned = p.clone();
    assert_eq!(cloned.id, "p");
}

#[test]
fn project_with_milestones_serde() {
    use sisyphus_types::roadmap::{Milestone, MilestoneStatus, Phase, Project};
    let ms = Milestone {
        id: "m1".into(),
        name: "M1".into(),
        phases: vec![Phase {
            id: "p1".into(),
            name: "P1".into(),
            goals: vec![],
            depends_on: vec![],
        }],
        status: MilestoneStatus::InProgress,
    };
    let mut p = Project::new("p".into(), "n".into(), "d".into());
    p.roadmap.milestones.push(ms);
    let j = serde_json::to_string(&p).unwrap();
    let back: Project = serde_json::from_str(&j).unwrap();
    assert_eq!(back.roadmap.milestones.len(), 1);
    assert_eq!(back.roadmap.milestones[0].id, "m1");
}

#[test]
fn milestone_status_display() {
    use sisyphus_types::roadmap::MilestoneStatus;
    let _ = format!("{}", MilestoneStatus::Pending);
    let _ = format!("{}", MilestoneStatus::InProgress);
    let _ = format!("{}", MilestoneStatus::Completed);
    let _ = format!("{}", MilestoneStatus::Blocked);
}

#[test]
fn milestone_status_debug() {
    use sisyphus_types::roadmap::MilestoneStatus;
    let _ = format!("{:?}", MilestoneStatus::Pending);
    let _ = format!("{:?}", MilestoneStatus::Completed);
}

#[test]
fn milestone_status_clone() {
    use sisyphus_types::roadmap::MilestoneStatus;
    let a = MilestoneStatus::Completed;
    let b = a.clone();
    assert_eq!(a, b);
}

#[test]
fn phase_with_unicode() {
    use sisyphus_types::roadmap::{GoalRef, MilestoneStatus, Phase};
    let p = Phase {
        id: "阶段-α".into(),
        name: "阶段名".into(),
        goals: vec![GoalRef {
            goal_id: "目标-1".into(),
            description: "描述".into(),
            status: MilestoneStatus::Pending,
        }],
        depends_on: vec!["依赖-1".into()],
    };
    assert_eq!(p.id, "阶段-α");
    assert_eq!(p.depends_on[0], "依赖-1");
}

#[test]
fn goal_ref_serde() {
    use sisyphus_types::roadmap::{GoalRef, MilestoneStatus};
    let g = GoalRef {
        goal_id: "g".into(),
        description: "d".into(),
        status: MilestoneStatus::Completed,
    };
    let j = serde_json::to_string(&g).unwrap();
    let back: GoalRef = serde_json::from_str(&j).unwrap();
    assert_eq!(back.goal_id, "g");
    assert_eq!(back.status, MilestoneStatus::Completed);
}

// ---------------------------------------------------------------------------
// Additional Milestone and MilestoneStatus coverage
// ---------------------------------------------------------------------------

#[test]
fn milestone_with_empty_phases() {
    use sisyphus_types::roadmap::{Milestone, MilestoneStatus};
    let m = Milestone {
        id: "m1".into(),
        name: "M1".into(),
        phases: vec![],
        status: MilestoneStatus::Pending,
    };
    assert!(m.phases.is_empty());
    let j = serde_json::to_string(&m).unwrap();
    let back: Milestone = serde_json::from_str(&j).unwrap();
    assert!(back.phases.is_empty());
}

#[test]
fn milestone_with_many_phases() {
    use sisyphus_types::roadmap::{Milestone, MilestoneStatus, Phase};
    let phases: Vec<Phase> = (0..20)
        .map(|i| Phase {
            id: format!("p{}", i),
            name: format!("P{}", i),
            goals: vec![],
            depends_on: vec![],
        })
        .collect();
    let m = Milestone {
        id: "m".into(),
        name: "M".into(),
        phases,
        status: MilestoneStatus::InProgress,
    };
    assert_eq!(m.phases.len(), 20);
    let j = serde_json::to_string(&m).unwrap();
    let back: Milestone = serde_json::from_str(&j).unwrap();
    assert_eq!(back.phases.len(), 20);
}

#[test]
fn milestone_clone() {
    use sisyphus_types::roadmap::{Milestone, MilestoneStatus};
    let m = Milestone {
        id: "m".into(),
        name: "n".into(),
        phases: vec![],
        status: MilestoneStatus::Completed,
    };
    let cloned = m.clone();
    assert_eq!(cloned.id, "m");
    assert_eq!(cloned.status, MilestoneStatus::Completed);
}

#[test]
fn milestone_status_distinct_via_serde() {
    use sisyphus_types::roadmap::MilestoneStatus;
    let variants = [
        MilestoneStatus::Pending,
        MilestoneStatus::InProgress,
        MilestoneStatus::Completed,
        MilestoneStatus::Blocked,
    ];
    let serialized: Vec<String> =
        variants.iter().map(|s| serde_json::to_string(s).unwrap()).collect();
    let unique: std::collections::HashSet<_> = serialized.iter().collect();
    assert_eq!(unique.len(), 4);
}

#[test]
fn phase_with_many_dependencies() {
    use sisyphus_types::roadmap::Phase;
    let deps: Vec<String> = (0..100).map(|i| format!("dep-{}", i)).collect();
    let p = Phase {
        id: "p".into(),
        name: "P".into(),
        goals: vec![],
        depends_on: deps.clone(),
    };
    assert_eq!(p.depends_on.len(), 100);
}

#[test]
fn roadmap_with_many_milestones() {
    use sisyphus_types::roadmap::{Milestone, MilestoneStatus, Roadmap};
    let milestones: Vec<Milestone> = (0..50)
        .map(|i| Milestone {
            id: format!("m{}", i),
            name: format!("M{}", i),
            phases: vec![],
            status: MilestoneStatus::Pending,
        })
        .collect();
    let r = Roadmap { milestones };
    assert_eq!(r.milestones.len(), 50);
    let j = serde_json::to_string(&r).unwrap();
    let back: Roadmap = serde_json::from_str(&j).unwrap();
    assert_eq!(back.milestones.len(), 50);
}

#[test]
fn goal_ref_with_unicode() {
    use sisyphus_types::roadmap::{GoalRef, MilestoneStatus};
    let g = GoalRef {
        goal_id: "目标-α".into(),
        description: "描述-β".into(),
        status: MilestoneStatus::Pending,
    };
    let j = serde_json::to_string(&g).unwrap();
    let back: GoalRef = serde_json::from_str(&j).unwrap();
    assert_eq!(back.goal_id, "目标-α");
}

// ---------------------------------------------------------------------------
// Additional calculate_progress coverage
// ---------------------------------------------------------------------------

#[test]
fn calculate_progress_single_goal_completed() {
    use sisyphus_types::roadmap::{Milestone, MilestoneStatus, Phase};
    let m = Milestone {
        id: "m".into(),
        name: "M".into(),
        phases: vec![Phase {
            id: "p".into(),
            name: "P".into(),
            goals: vec![sisyphus_types::roadmap::GoalRef {
                goal_id: "g".into(),
                description: "d".into(),
                status: MilestoneStatus::Completed,
            }],
            depends_on: vec![],
        }],
        status: MilestoneStatus::Completed,
    };
    assert!((m.calculate_progress() - 1.0).abs() < 1e-9);
}

#[test]
fn calculate_progress_three_of_four_completed() {
    use sisyphus_types::roadmap::{GoalRef, Milestone, MilestoneStatus, Phase};
    let m = Milestone {
        id: "m".into(),
        name: "M".into(),
        phases: vec![Phase {
            id: "p".into(),
            name: "P".into(),
            goals: vec![
                GoalRef {
                    goal_id: "g1".into(),
                    description: "d".into(),
                    status: MilestoneStatus::Completed,
                },
                GoalRef {
                    goal_id: "g2".into(),
                    description: "d".into(),
                    status: MilestoneStatus::Completed,
                },
                GoalRef {
                    goal_id: "g3".into(),
                    description: "d".into(),
                    status: MilestoneStatus::Completed,
                },
                GoalRef {
                    goal_id: "g4".into(),
                    description: "d".into(),
                    status: MilestoneStatus::Pending,
                },
            ],
            depends_on: vec![],
        }],
        status: MilestoneStatus::InProgress,
    };
    assert!((m.calculate_progress() - 0.75).abs() < 1e-9);
}

#[test]
fn calculate_progress_with_three_phases() {
    use sisyphus_types::roadmap::{GoalRef, Milestone, MilestoneStatus, Phase};
    let m = Milestone {
        id: "m".into(),
        name: "M".into(),
        phases: vec![
            Phase {
                id: "p1".into(),
                name: "P1".into(),
                goals: vec![GoalRef {
                    goal_id: "g1".into(),
                    description: "d".into(),
                    status: MilestoneStatus::Completed,
                }],
                depends_on: vec![],
            },
            Phase {
                id: "p2".into(),
                name: "P2".into(),
                goals: vec![GoalRef {
                    goal_id: "g2".into(),
                    description: "d".into(),
                    status: MilestoneStatus::Pending,
                }],
                depends_on: vec![],
            },
            Phase {
                id: "p3".into(),
                name: "P3".into(),
                goals: vec![GoalRef {
                    goal_id: "g3".into(),
                    description: "d".into(),
                    status: MilestoneStatus::InProgress,
                }],
                depends_on: vec![],
            },
        ],
        status: MilestoneStatus::InProgress,
    };
    // 1/3 = 0.333...
    let progress = m.calculate_progress();
    assert!((progress - 1.0 / 3.0).abs() < 1e-9);
}

#[test]
fn calculate_progress_with_blocked_goals() {
    use sisyphus_types::roadmap::{GoalRef, Milestone, MilestoneStatus, Phase};
    let m = Milestone {
        id: "m".into(),
        name: "M".into(),
        phases: vec![Phase {
            id: "p".into(),
            name: "P".into(),
            goals: vec![
                GoalRef {
                    goal_id: "g1".into(),
                    description: "d".into(),
                    status: MilestoneStatus::Blocked,
                },
                GoalRef {
                    goal_id: "g2".into(),
                    description: "d".into(),
                    status: MilestoneStatus::Completed,
                },
            ],
            depends_on: vec![],
        }],
        status: MilestoneStatus::Blocked,
    };
    // 1/2
    assert!((m.calculate_progress() - 0.5).abs() < 1e-9);
}

// ── mutation testing 防护：验证 Display 输出文本 ──
// cargo-mutants 抓到"替换 fmt 返回 Ok(Default::default())"会清空所有变体输出
// → 显式 assert_eq 文本内容是必要的回归保护
#[test]
fn milestone_status_display_strings() {
    assert_eq!(format!("{}", MilestoneStatus::Pending), "Pending");
    assert_eq!(format!("{}", MilestoneStatus::InProgress), "InProgress");
    assert_eq!(format!("{}", MilestoneStatus::Completed), "Completed");
    assert_eq!(format!("{}", MilestoneStatus::Blocked), "Blocked");

    // 防止"全部返回相同字符串"型 mutant 漏检：4 个变体必须互不相等
    let displays: Vec<String> = [
        MilestoneStatus::Pending,
        MilestoneStatus::InProgress,
        MilestoneStatus::Completed,
        MilestoneStatus::Blocked,
    ]
    .iter()
    .map(|s| format!("{s}"))
    .collect();
    let unique: std::collections::HashSet<_> = displays.iter().collect();
    assert_eq!(unique.len(), 4, "displays must all be distinct");
}
