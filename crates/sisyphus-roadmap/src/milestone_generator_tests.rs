use super::*;
use crate::RoadmapManager;
use sisyphus_mem::MfsStore;
use sisyphus_tools_llm::metis::FixedResponseClient;
use std::sync::Arc;

async fn setup_manager() -> (RoadmapManager, Arc<MfsStore>) {
    let store = Arc::new(
        MfsStore::new("sqlite::memory:")
            .await
            .expect("Failed to create MfsStore"),
    );
    (RoadmapManager::new(store.clone()), store)
}

#[tokio::test]
async fn test_generate_milestones_parses_llm_response() {
    let (mgr, _store) = setup_manager().await;
    mgr.create_project("p1", "Test Project", "A test")
        .await
        .unwrap();

    let llm = Arc::new(FixedResponseClient(
        r#"{
            "project_name": "Test Project",
            "milestones": [
                {
                    "id": "m1",
                    "name": "Foundation",
                    "phases": [
                        {
                            "id": "m1_p1",
                            "name": "Setup",
                            "goals": [
                                {"id": "m1_p1_g1", "description": "Install dependencies"},
                                {"id": "m1_p1_g2", "description": "Configure CI"}
                            ]
                        }
                    ]
                },
                {
                    "id": "m2",
                    "name": "Core Features",
                    "phases": [
                        {
                            "id": "m2_p1",
                            "name": "Backend",
                            "goals": [
                                {"id": "m2_p1_g1", "description": "Build API"}
                            ]
                        }
                    ]
                }
            ]
        }"#
        .to_string(),
    ));

    let r#gen = MilestoneGenerator::new(llm);
    let milestones = r#gen
        .generate_milestones(&mgr, "p1", "Build a web application")
        .await
        .unwrap();

    assert_eq!(milestones.len(), 2);
    assert_eq!(milestones[0].name, "Foundation");
    assert_eq!(milestones[0].phases[0].goals.len(), 2);
    assert_eq!(milestones[1].phases[0].goals[0].goal_id, "m2_p1_g1");

    // Verify persisted
    let project = mgr.get_project("p1").await.unwrap().unwrap();
    assert_eq!(project.roadmap.milestones.len(), 2);
}

#[tokio::test]
async fn test_generate_milestones_invalid_json() {
    let (mgr, _store) = setup_manager().await;
    mgr.create_project("p2", "Bad", "desc").await.unwrap();

    let llm = Arc::new(FixedResponseClient("not valid json".to_string()));
    let r#gen = MilestoneGenerator::new(llm);
    let err = r#gen
        .generate_milestones(&mgr, "p2", "test")
        .await
        .unwrap_err();
    assert!(err
        .to_string()
        .contains("Failed to parse milestone proposal"));
}

#[tokio::test]
async fn test_propose_refinements_returns_suggestions() {
    let (mgr, _store) = setup_manager().await;
    mgr.create_project("p3", "Refine", "desc").await.unwrap();

    // Add a milestone with goals
    use sisyphus_types::{GoalRef, MilestoneStatus};
    let goal = GoalRef {
        goal_id: "g1".into(),
        description: "Implement feature X".into(),
        status: MilestoneStatus::Blocked,
    };
    let phase = Phase {
        id: "ph1".into(),
        name: "Development".into(),
        depends_on: vec![],
        goals: vec![goal],
    };
    let ms = Milestone {
        id: "ms1".into(),
        name: "Release".into(),
        status: MilestoneStatus::InProgress,
        phases: vec![phase],
    };
    mgr.add_milestone("p3", ms).await.unwrap();

    let llm = Arc::new(FixedResponseClient(
        r#"[
            {
                "milestone_id": "ms1",
                "phase_id": "ph1",
                "action": "Split blocked goal into smaller tasks",
                "rationale": "Goal is blocked and too large"
            }
        ]"#
        .to_string(),
    ));

    let r#gen = MilestoneGenerator::new(llm);
    let suggestions = r#gen
        .propose_refinements(&mgr, "p3", "ms1", "Goal is blocked")
        .await
        .unwrap();

    assert_eq!(suggestions.len(), 1);
    assert_eq!(suggestions[0].milestone_id.as_deref(), Some("ms1"));
    assert!(suggestions[0].action.contains("Split"));
}

#[tokio::test]
async fn test_fill_roadmap_gaps() {
    let (mgr, _store) = setup_manager().await;
    mgr.create_project("p4", "Gap Fill", "desc").await.unwrap();

    // Add one existing milestone
    let ms = Milestone {
        id: "m1".into(),
        name: "Foundation".into(),
        status: MilestoneStatus::InProgress,
        phases: vec![],
    };
    mgr.add_milestone("p4", ms).await.unwrap();

    let llm = Arc::new(FixedResponseClient(
        r#"{
            "project_name": "Gap Fill",
            "milestones": [
                {
                    "id": "m2",
                    "name": "Testing & QA",
                    "phases": [
                        {
                            "id": "m2_p1",
                            "name": "Test Suite",
                            "goals": [
                                {"id": "m2_p1_g1", "description": "Write unit tests"}
                            ]
                        }
                    ]
                }
            ]
        }"#
        .to_string(),
    ));

    let r#gen = MilestoneGenerator::new(llm);
    let new_ms = r#gen
        .fill_roadmap_gaps(&mgr, "p4", "Build a web app")
        .await
        .unwrap();

    assert_eq!(new_ms.len(), 1);
    assert_eq!(new_ms[0].name, "Testing & QA");

    // Verify both milestones exist
    let project = mgr.get_project("p4").await.unwrap().unwrap();
    assert_eq!(project.roadmap.milestones.len(), 2);
}

#[tokio::test]
async fn test_generate_empty_response() {
    let (mgr, _store) = setup_manager().await;
    mgr.create_project("p5", "Empty", "desc").await.unwrap();

    let llm = Arc::new(FixedResponseClient(
        r#"{"project_name": "Empty", "milestones": []}"#.to_string(),
    ));

    let r#gen = MilestoneGenerator::new(llm);
    let milestones = r#gen.generate_milestones(&mgr, "p5", "test").await.unwrap();
    assert!(milestones.is_empty());
}
