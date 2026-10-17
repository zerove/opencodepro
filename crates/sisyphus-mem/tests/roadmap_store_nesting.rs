//! Lock the correctness of the batched `RoadmapSqlStore::get_project`.
//!
//! That implementation used to query phases per milestone and goals per
//! phase, costing `1 + M + M*P` round trips. These tests pin the assembled
//! result for a nested project so the batching refactor cannot silently drop
//! goals, dependencies or statuses.

use sisyphus_mem::roadmap_store::RoadmapSqlStore;
use sisyphus_types::roadmap::{GoalRef, Milestone, MilestoneStatus, Phase, Project, Roadmap};
use sqlx::SqlitePool;

async fn store() -> RoadmapSqlStore {
    let pool = SqlitePool::connect("sqlite::memory:").await.expect("pool");
    let store = RoadmapSqlStore::new(pool).await.expect("store");
    store.init_schema().await.expect("schema");
    store
}

async fn seed(s: &RoadmapSqlStore) {
    s.insert_project(&Project {
        id: "p1".into(),
        name: "Project".into(),
        description: "desc".into(),
        roadmap: Roadmap {
            milestones: vec![
                Milestone {
                    id: "m1".into(),
                    name: "M1".into(),
                    status: MilestoneStatus::InProgress,
                    phases: vec![
                        Phase {
                            id: "ph1".into(),
                            name: "P1".into(),
                            depends_on: vec![],
                            goals: vec![GoalRef {
                                goal_id: "g1".into(),
                                description: "goal one".into(),
                                status: MilestoneStatus::Completed,
                            }],
                        },
                        Phase {
                            id: "ph2".into(),
                            name: "P2".into(),
                            depends_on: vec!["ph1".into()],
                            goals: vec![],
                        },
                    ],
                },
                Milestone {
                    id: "m2".into(),
                    name: "M2".into(),
                    status: MilestoneStatus::Pending,
                    phases: vec![],
                },
            ],
        },
    })
    .await
    .expect("insert project");

    s.insert_milestone("m1", "p1", "M1", "InProgress").await.expect("insert m1");
    s.insert_milestone("m2", "p1", "M2", "Pending").await.expect("insert m2");

    s.insert_phase("p1", "ph1", "m1", "P1", None).await.expect("insert ph1");
    s.insert_phase("p1", "ph2", "m1", "P2", Some("ph1".into())).await.expect("insert ph2");

    s.insert_goal("p1", "g1", "ph1", "goal one", "Completed").await.expect("insert g1");
}

#[tokio::test]
async fn get_project_reassembles_the_full_nested_graph() {
    let s = store().await;
    seed(&s).await;

    let loaded = s.get_project("p1").await.expect("query").expect("project must exist");

    assert_eq!(loaded.id, "p1");
    assert_eq!(loaded.roadmap.milestones.len(), 2, "both milestones load");

    let m1 = loaded.roadmap.milestones.iter().find(|m| m.id == "m1").expect("m1");
    assert_eq!(m1.phases.len(), 2, "both phases of m1 load");

    let ph1 = m1.phases.iter().find(|p| p.id == "ph1").expect("ph1");
    assert_eq!(ph1.goals.len(), 1, "goal of ph1 loads");
    assert_eq!(ph1.goals[0].goal_id, "g1");
    assert_eq!(ph1.goals[0].status, MilestoneStatus::Completed);

    let ph2 = m1.phases.iter().find(|p| p.id == "ph2").expect("ph2");
    assert_eq!(ph2.depends_on, vec!["ph1".to_string()], "deps survive");
    assert!(ph2.goals.is_empty(), "phase without goals");

    let m2 = loaded.roadmap.milestones.iter().find(|m| m.id == "m2").expect("m2");
    assert!(m2.phases.is_empty(), "milestone without phases");
}

#[tokio::test]
async fn get_project_returns_none_for_unknown_id() {
    let s = store().await;
    assert!(s.get_project("missing").await.expect("query").is_none());
}

#[tokio::test]
async fn insert_milestone_tree_persists_all_levels() {
    let s = store().await;
    s.insert_project(&Project {
        id: "p2".into(),
        name: "P2".into(),
        description: String::new(),
        roadmap: Roadmap { milestones: vec![] },
    })
    .await
    .expect("insert project");

    let milestone = Milestone {
        id: "m9".into(),
        name: "M9".into(),
        status: MilestoneStatus::InProgress,
        phases: vec![Phase {
            id: "ph9".into(),
            name: "P9".into(),
            depends_on: vec![],
            goals: vec![
                GoalRef {
                    goal_id: "g9a".into(),
                    description: "a".into(),
                    status: MilestoneStatus::Pending,
                },
                GoalRef {
                    goal_id: "g9b".into(),
                    description: "b".into(),
                    status: MilestoneStatus::Blocked,
                },
            ],
        }],
    };
    s.insert_milestone_tree("p2", &milestone).await.expect("insert tree");

    let loaded = s.get_project("p2").await.expect("q").expect("exists");
    let m = loaded.roadmap.milestones.iter().find(|m| m.id == "m9").expect("milestone");
    assert_eq!(m.phases.len(), 1);
    assert_eq!(m.phases[0].goals.len(), 2, "both goals land in one tx");
    assert_eq!(m.status, MilestoneStatus::InProgress);
}
