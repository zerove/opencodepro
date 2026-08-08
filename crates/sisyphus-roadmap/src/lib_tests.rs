use super::*;
use sisyphus_types::{GoalRef, Milestone, MilestoneStatus, Phase};

async fn setup_manager() -> RoadmapManager {
    let store = std::sync::Arc::new(
        sisyphus_mem::MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
            .await
            .expect("Failed to create in-memory store"),
    );
    let sql_store = std::sync::Arc::new(
        sisyphus_mem::RoadmapSqlStore::new(store.pool().clone())
            .await
            .expect("Failed to create in-memory sql store"),
    );
    sql_store
        .init_schema()
        .await
        .expect("Failed to init schema");
    let proposal_manager =
        std::sync::Arc::new(crate::proposal_manager::ProposalManager::new(store.clone()));
    RoadmapManager::new(store, sql_store, proposal_manager)
}

#[tokio::test]
async fn test_create_and_get_project() {
    let manager = setup_manager().await;
    let project = manager
        .create_project("test-1", "Test Project", "A test project")
        .await
        .expect("create_project should succeed");
    assert_eq!(project.id, "test-1");
    assert_eq!(project.name, "Test Project");

    let loaded = manager
        .get_project("test-1")
        .await
        .expect("get_project should succeed")
        .expect("project should exist");
    assert_eq!(loaded.id, "test-1");
}

#[tokio::test]
async fn test_list_projects() {
    let manager = setup_manager().await;
    manager
        .create_project("p1", "Project 1", "First")
        .await
        .unwrap();
    manager
        .create_project("p2", "Project 2", "Second")
        .await
        .unwrap();
    let ids = manager.list_project_ids().await.unwrap();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&"p1".to_string()));
    assert!(ids.contains(&"p2".to_string()));
}

#[tokio::test]
async fn test_duplicate_project_rejected() {
    let manager = setup_manager().await;
    manager.create_project("dup", "Original", "").await.unwrap();
    let err = manager
        .create_project("dup", "Duplicate", "")
        .await
        .unwrap_err();
    assert!(
        matches!(err, RoadmapError::DuplicateProject(_)),
        "Expected DuplicateProject, got {err:?}"
    );
}

#[tokio::test]
async fn test_delete_project() {
    let manager = setup_manager().await;
    manager
        .create_project("del-me", "To Delete", "")
        .await
        .unwrap();
    manager.delete_project("del-me").await.unwrap();
    let project = manager.get_project("del-me").await.unwrap();
    assert!(project.is_none());
}

#[tokio::test]
async fn test_add_and_update_milestone() {
    let manager = setup_manager().await;
    manager
        .create_project("ms-test", "Milestone Test", "")
        .await
        .unwrap();

    let milestone = Milestone {
        id: "ms-1".into(),
        name: "First Milestone".into(),
        phases: Vec::new(),
        status: MilestoneStatus::Pending,
    };
    manager.add_milestone("ms-test", milestone).await.unwrap();

    manager
        .update_milestone_status("ms-test", "ms-1", MilestoneStatus::InProgress)
        .await
        .unwrap();

    let project = manager.get_project("ms-test").await.unwrap().unwrap();
    assert_eq!(project.roadmap.milestones.len(), 1);
    assert_eq!(
        project.roadmap.milestones[0].status,
        MilestoneStatus::InProgress
    );
}

#[tokio::test]
async fn test_remove_milestone() {
    let manager = setup_manager().await;
    manager
        .create_project("rm-ms", "Remove Milestone", "")
        .await
        .unwrap();
    manager
        .add_milestone(
            "rm-ms",
            Milestone {
                id: "ms-1".into(),
                name: "To Remove".into(),
                phases: Vec::new(),
                status: MilestoneStatus::Pending,
            },
        )
        .await
        .unwrap();
    manager.remove_milestone("rm-ms", "ms-1").await.unwrap();
    let project = manager.get_project("rm-ms").await.unwrap().unwrap();
    assert!(project.roadmap.milestones.is_empty());
}

#[tokio::test]
async fn test_add_and_update_goal() {
    let manager = setup_manager().await;
    manager
        .create_project("goal-test", "Goal Test", "")
        .await
        .unwrap();
    manager
        .add_milestone(
            "goal-test",
            Milestone {
                id: "ms-1".into(),
                name: "MS".into(),
                phases: Vec::new(),
                status: MilestoneStatus::Pending,
            },
        )
        .await
        .unwrap();
    manager
        .add_phase(
            "goal-test",
            "ms-1",
            Phase {
                id: "ph-1".into(),
                name: "Phase 1".into(),
                goals: Vec::new(),
                depends_on: Vec::new(),
            },
        )
        .await
        .unwrap();

    let goal = GoalRef {
        goal_id: "g-1".into(),
        description: "First goal".into(),
        status: MilestoneStatus::Pending,
    };
    manager
        .add_goal("goal-test", "ms-1", "ph-1", goal)
        .await
        .unwrap();

    manager
        .update_goal_status(
            "goal-test",
            "ms-1",
            "ph-1",
            "g-1",
            MilestoneStatus::Completed,
        )
        .await
        .unwrap();

    let project = manager.get_project("goal-test").await.unwrap().unwrap();
    let goal = &project.roadmap.milestones[0].phases[0].goals[0];
    assert_eq!(goal.status, MilestoneStatus::Completed);
}

#[tokio::test]
async fn test_calculate_progress() {
    let manager = setup_manager().await;
    manager
        .create_project("progress", "Progress", "")
        .await
        .unwrap();
    manager
        .add_milestone(
            "progress",
            Milestone {
                id: "ms-1".into(),
                name: "MS".into(),
                phases: Vec::new(),
                status: MilestoneStatus::Pending,
            },
        )
        .await
        .unwrap();
    manager
        .add_phase(
            "progress",
            "ms-1",
            Phase {
                id: "ph-1".into(),
                name: "P1".into(),
                goals: Vec::new(),
                depends_on: Vec::new(),
            },
        )
        .await
        .unwrap();
    manager
        .add_goal(
            "progress",
            "ms-1",
            "ph-1",
            GoalRef {
                goal_id: "g-1".into(),
                description: "Goal 1".into(),
                status: MilestoneStatus::Pending,
            },
        )
        .await
        .unwrap();
    manager
        .add_goal(
            "progress",
            "ms-1",
            "ph-1",
            GoalRef {
                goal_id: "g-2".into(),
                description: "Goal 2".into(),
                status: MilestoneStatus::Pending,
            },
        )
        .await
        .unwrap();

    let pct = manager.calculate_progress("progress").await.unwrap();
    assert_eq!(pct, 0.0);

    manager
        .update_goal_status(
            "progress",
            "ms-1",
            "ph-1",
            "g-1",
            MilestoneStatus::Completed,
        )
        .await
        .unwrap();

    let pct = manager.calculate_progress("progress").await.unwrap();
    assert!((pct - 0.5).abs() < 0.001);
}

#[tokio::test]
async fn test_progress_report() {
    let manager = setup_manager().await;
    manager
        .create_project("report", "Report Test", "")
        .await
        .unwrap();
    manager
        .add_milestone(
            "report",
            Milestone {
                id: "ms-1".into(),
                name: "MS1".into(),
                phases: Vec::new(),
                status: MilestoneStatus::Pending,
            },
        )
        .await
        .unwrap();
    manager
        .add_phase(
            "report",
            "ms-1",
            Phase {
                id: "ph-1".into(),
                name: "P1".into(),
                goals: Vec::new(),
                depends_on: Vec::new(),
            },
        )
        .await
        .unwrap();
    manager
        .add_goal(
            "report",
            "ms-1",
            "ph-1",
            GoalRef {
                goal_id: "g-1".into(),
                description: "Goal 1".into(),
                status: MilestoneStatus::Pending,
            },
        )
        .await
        .unwrap();
    manager
        .update_goal_status("report", "ms-1", "ph-1", "g-1", MilestoneStatus::Completed)
        .await
        .unwrap();

    let report = manager.generate_progress_report("report").await.unwrap();
    assert_eq!(report.total_goals, 1);
    assert_eq!(report.completed_goals, 1);
}

#[tokio::test]
async fn test_project_context_synthesis() {
    let manager = setup_manager().await;
    manager
        .create_project("ctx", "Context Test", "Testing context")
        .await
        .unwrap();
    manager
        .add_milestone(
            "ctx",
            Milestone {
                id: "ms-1".into(),
                name: "MS1".into(),
                phases: Vec::new(),
                status: MilestoneStatus::Pending,
            },
        )
        .await
        .unwrap();
    manager
        .add_phase(
            "ctx",
            "ms-1",
            Phase {
                id: "ph-1".into(),
                name: "P1".into(),
                goals: Vec::new(),
                depends_on: Vec::new(),
            },
        )
        .await
        .unwrap();
    manager
        .add_goal(
            "ctx",
            "ms-1",
            "ph-1",
            GoalRef {
                goal_id: "g-1".into(),
                description: "Goal 1".into(),
                status: MilestoneStatus::Pending,
            },
        )
        .await
        .unwrap();

    let context = manager.synthesize_project_context("ctx").await.unwrap();
    assert!(context.contains("Context Test"));
    assert!(context.contains("MS1"));
    assert!(context.contains("Goal 1"));
}

#[tokio::test]
async fn test_goal_status_propagation() {
    let manager = setup_manager().await;
    manager
        .create_project("prop", "Propagation", "")
        .await
        .unwrap();
    manager
        .add_milestone(
            "prop",
            Milestone {
                id: "ms-1".into(),
                name: "MS1".into(),
                phases: Vec::new(),
                status: MilestoneStatus::Pending,
            },
        )
        .await
        .unwrap();
    manager
        .add_phase(
            "prop",
            "ms-1",
            Phase {
                id: "ph-1".into(),
                name: "P1".into(),
                goals: Vec::new(),
                depends_on: Vec::new(),
            },
        )
        .await
        .unwrap();
    manager
        .add_goal(
            "prop",
            "ms-1",
            "ph-1",
            GoalRef {
                goal_id: "g-1".into(),
                description: "Goal 1".into(),
                status: MilestoneStatus::Pending,
            },
        )
        .await
        .unwrap();
    manager
        .add_goal(
            "prop",
            "ms-1",
            "ph-1",
            GoalRef {
                goal_id: "g-2".into(),
                description: "Goal 2".into(),
                status: MilestoneStatus::Pending,
            },
        )
        .await
        .unwrap();

    // 完成所有目标 → 里程碑变为已完成
    manager
        .update_goal_status_with_propagation(
            "prop",
            "ms-1",
            "ph-1",
            "g-1",
            MilestoneStatus::Completed,
        )
        .await
        .unwrap();
    manager
        .update_goal_status_with_propagation(
            "prop",
            "ms-1",
            "ph-1",
            "g-2",
            MilestoneStatus::Completed,
        )
        .await
        .unwrap();

    let project = manager.get_project("prop").await.unwrap().unwrap();
    assert_eq!(
        project.roadmap.milestones[0].status,
        MilestoneStatus::Completed
    );
}

#[tokio::test]
async fn test_auto_propagate_all() {
    let manager = setup_manager().await;
    manager
        .create_project("auto", "Auto Propagate", "")
        .await
        .unwrap();
    manager
        .add_milestone(
            "auto",
            Milestone {
                id: "ms-1".into(),
                name: "MS1".into(),
                phases: Vec::new(),
                status: MilestoneStatus::Pending,
            },
        )
        .await
        .unwrap();
    manager
        .add_phase(
            "auto",
            "ms-1",
            Phase {
                id: "ph-1".into(),
                name: "P1".into(),
                goals: Vec::new(),
                depends_on: Vec::new(),
            },
        )
        .await
        .unwrap();
    manager
        .add_goal(
            "auto",
            "ms-1",
            "ph-1",
            GoalRef {
                goal_id: "g-1".into(),
                description: "Goal 1".into(),
                status: MilestoneStatus::Pending,
            },
        )
        .await
        .unwrap();
    manager
        .add_goal(
            "auto",
            "ms-1",
            "ph-1",
            GoalRef {
                goal_id: "g-2".into(),
                description: "Goal 2".into(),
                status: MilestoneStatus::Pending,
            },
        )
        .await
        .unwrap();
    manager
        .update_goal_status("auto", "ms-1", "ph-1", "g-1", MilestoneStatus::Completed)
        .await
        .unwrap();
    manager
        .update_goal_status("auto", "ms-1", "ph-1", "g-2", MilestoneStatus::Completed)
        .await
        .unwrap();

    manager.auto_propagate_all("auto").await.unwrap();
    let project = manager.get_project("auto").await.unwrap().unwrap();
    assert_eq!(
        project.roadmap.milestones[0].status,
        MilestoneStatus::Completed
    );
}

#[tokio::test]
async fn test_get_next_goal() {
    let manager = setup_manager().await;
    manager
        .create_project("next-goal", "Next Goal Test", "")
        .await
        .unwrap();
    manager
        .add_milestone(
            "next-goal",
            Milestone {
                id: "ms-1".into(),
                name: "MS1".into(),
                phases: Vec::new(),
                status: MilestoneStatus::Pending,
            },
        )
        .await
        .unwrap();
    manager
        .add_phase(
            "next-goal",
            "ms-1",
            Phase {
                id: "ph-1".into(),
                name: "P1".into(),
                goals: Vec::new(),
                depends_on: Vec::new(),
            },
        )
        .await
        .unwrap();
    manager
        .add_goal(
            "next-goal",
            "ms-1",
            "ph-1",
            GoalRef {
                goal_id: "g-1".into(),
                description: "Goal 1".into(),
                status: MilestoneStatus::Pending,
            },
        )
        .await
        .unwrap();
    manager
        .add_goal(
            "next-goal",
            "ms-1",
            "ph-1",
            GoalRef {
                goal_id: "g-2".into(),
                description: "Goal 2".into(),
                status: MilestoneStatus::Pending,
            },
        )
        .await
        .unwrap();

    // 完成 g-1，g-2 应是下一个
    manager
        .update_goal_status(
            "next-goal",
            "ms-1",
            "ph-1",
            "g-1",
            MilestoneStatus::Completed,
        )
        .await
        .unwrap();

    let next = manager.get_next_goal("next-goal").await.unwrap().unwrap();
    assert_eq!(next.goal_id, "g-2");
}

#[tokio::test]
async fn test_get_goal_context() {
    let manager = setup_manager().await;
    manager.create_project("gc", "Goal Ctx", "").await.unwrap();
    manager
        .add_milestone(
            "gc",
            Milestone {
                id: "ms-1".into(),
                name: "MS1".into(),
                phases: Vec::new(),
                status: MilestoneStatus::Pending,
            },
        )
        .await
        .unwrap();
    manager
        .add_phase(
            "gc",
            "ms-1",
            Phase {
                id: "ph-1".into(),
                name: "P1".into(),
                goals: Vec::new(),
                depends_on: Vec::new(),
            },
        )
        .await
        .unwrap();
    manager
        .add_goal(
            "gc",
            "ms-1",
            "ph-1",
            GoalRef {
                goal_id: "g-1".into(),
                description: "Goal 1".into(),
                status: MilestoneStatus::Pending,
            },
        )
        .await
        .unwrap();

    let ctx = manager.get_goal_context("gc", "g-1").await.unwrap();
    assert!(ctx.contains("Goal 1"));
}

#[tokio::test]
async fn test_predict_completion() {
    let manager = setup_manager().await;
    manager.create_project("pred", "Predict", "").await.unwrap();
    manager
        .add_milestone(
            "pred",
            Milestone {
                id: "ms-1".into(),
                name: "MS1".into(),
                phases: Vec::new(),
                status: MilestoneStatus::Pending,
            },
        )
        .await
        .unwrap();
    manager
        .add_phase(
            "pred",
            "ms-1",
            Phase {
                id: "ph-1".into(),
                name: "P1".into(),
                goals: Vec::new(),
                depends_on: Vec::new(),
            },
        )
        .await
        .unwrap();
    manager
        .add_goal(
            "pred",
            "ms-1",
            "ph-1",
            GoalRef {
                goal_id: "g-1".into(),
                description: "Goal 1".into(),
                status: MilestoneStatus::Pending,
            },
        )
        .await
        .unwrap();

    let prediction = manager.predict_completion("pred").await.unwrap();
    assert_eq!(prediction.remaining_goals, 1);
    assert_eq!(prediction.overall_progress_pct, 0.0);
}

#[tokio::test]
async fn test_record_and_get_outcomes() {
    let manager = setup_manager().await;
    manager.create_project("out", "Outcomes", "").await.unwrap();
    manager
        .add_milestone(
            "out",
            Milestone {
                id: "ms-1".into(),
                name: "MS1".into(),
                phases: Vec::new(),
                status: MilestoneStatus::Pending,
            },
        )
        .await
        .unwrap();
    manager
        .add_phase(
            "out",
            "ms-1",
            Phase {
                id: "ph-1".into(),
                name: "P1".into(),
                goals: Vec::new(),
                depends_on: Vec::new(),
            },
        )
        .await
        .unwrap();
    manager
        .add_goal(
            "out",
            "ms-1",
            "ph-1",
            GoalRef {
                goal_id: "g-1".into(),
                description: "Goal 1".into(),
                status: MilestoneStatus::Pending,
            },
        )
        .await
        .unwrap();

    let outcome = ExecutionOutcome {
        project_id: "out".into(),
        milestone_id: "ms-1".into(),
        phase_id: "ph-1".into(),
        goal_id: "g-1".into(),
        success: true,
        deviation_score: None,
        error: None,
        timestamp: 1,
        actual_cost: 0.0,
        duration_secs: 0.0,
    };
    manager.record_outcome(&outcome).await.unwrap();

    let outcomes = manager.get_outcomes("out").await.unwrap();
    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].success);
}

#[tokio::test]
async fn test_evaluate_health() {
    let manager = setup_manager().await;
    manager
        .create_project("health", "Health Check", "")
        .await
        .unwrap();
    manager
        .add_milestone(
            "health",
            Milestone {
                id: "ms-1".into(),
                name: "MS1".into(),
                phases: Vec::new(),
                status: MilestoneStatus::Pending,
            },
        )
        .await
        .unwrap();
    manager
        .add_phase(
            "health",
            "ms-1",
            Phase {
                id: "ph-1".into(),
                name: "P1".into(),
                goals: Vec::new(),
                depends_on: Vec::new(),
            },
        )
        .await
        .unwrap();
    manager
        .add_goal(
            "health",
            "ms-1",
            "ph-1",
            GoalRef {
                goal_id: "g-1".into(),
                description: "Goal 1".into(),
                status: MilestoneStatus::Pending,
            },
        )
        .await
        .unwrap();

    let health = manager.evaluate_health("health").await.unwrap();
    assert!(health.healthy); // nothing wrong yet
}

#[tokio::test]
async fn test_apply_blocking() {
    let manager = setup_manager().await;
    manager
        .create_project("block", "Apply Block", "")
        .await
        .unwrap();
    manager
        .add_milestone(
            "block",
            Milestone {
                id: "ms-1".into(),
                name: "MS1".into(),
                phases: Vec::new(),
                status: MilestoneStatus::Pending,
            },
        )
        .await
        .unwrap();
    manager
        .add_phase(
            "block",
            "ms-1",
            Phase {
                id: "ph-1".into(),
                name: "P1".into(),
                goals: Vec::new(),
                depends_on: Vec::new(),
            },
        )
        .await
        .unwrap();
    manager
        .add_goal(
            "block",
            "ms-1",
            "ph-1",
            GoalRef {
                goal_id: "g-1".into(),
                description: "Goal 1".into(),
                status: MilestoneStatus::Pending,
            },
        )
        .await
        .unwrap();
    manager
        .add_goal(
            "block",
            "ms-1",
            "ph-1",
            GoalRef {
                goal_id: "g-2".into(),
                description: "Goal 2".into(),
                status: MilestoneStatus::Pending,
            },
        )
        .await
        .unwrap();

    // 为 g-1 记录失败，然后 apply_blocking 应将 g-2 标记为已阻塞
    let outcome = ExecutionOutcome {
        project_id: "block".into(),
        milestone_id: "ms-1".into(),
        phase_id: "ph-1".into(),
        goal_id: "g-1".into(),
        success: false,
        deviation_score: None,
        error: Some("Failed".into()),
        timestamp: 1,
        actual_cost: 0.0,
        duration_secs: 0.0,
    };
    manager.record_outcome(&outcome).await.unwrap();
    manager.apply_blocking("block").await.unwrap();

    let project = manager.get_project("block").await.unwrap().unwrap();
    assert_eq!(
        project.roadmap.milestones[0].phases[0].goals[1].status,
        MilestoneStatus::Blocked
    );
}
