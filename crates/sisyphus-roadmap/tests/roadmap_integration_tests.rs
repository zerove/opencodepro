//! 集成测试：里程碑生成器、战略反思、提案管理、进度跟踪、健康检查

use async_trait::async_trait;
use std::sync::Arc;
use tempfile::TempDir;

use sisyphus_mem::{MfsStore, RoadmapSqlStore};
use sisyphus_roadmap::{
    milestone_generator::MilestoneGenerator,
    proposal_manager::ProposalManager,
    strategic_reflection::{
        ProposalAction, ProposalStatus, StrategicProposal, StrategicReflection, StrategicReflector,
    },
    ExecutionOutcome, RoadmapManager,
};
use sisyphus_tools_llm::metis::LlmClient;
use sisyphus_types::{GoalRef, Milestone, MilestoneStatus, Phase};

/// Mock LLM client for testing
struct MockLlmClient;

#[async_trait]
impl LlmClient for MockLlmClient {
    async fn generate(&self, prompt: &str) -> Result<String, sisyphus_types::TypesError> {
        self.chat("", prompt, false).await
    }

    async fn chat(
        &self,
        _system: &str,
        _user: &str,
        _stream: bool,
    ) -> Result<String, sisyphus_types::TypesError> {
        if _user.contains("Change request") || _user.contains("propose_refinements") {
            Ok(r#"[{"milestone_id": "ms1", "phase_id": "ph1", "action": "Split goal", "rationale": "Too large"}]"#.to_string())
        } else if _system.contains("senior engineering director")
            || _user.contains("strategic reflection")
        {
            // strategic reflector prompt
            Ok(r#"{"project_id": "p1", "health_assessment": "Warning", "trajectory": "Slightly off", "recommendations": [{"action": "Optimize core", "rationale": "Latency issues", "priority": "high", "target_milestone": "ms1"}], "risk_factors": ["Latency"], "confidence": 0.8}"#.to_string())
        } else if _system.contains("project planning expert")
            || _user.contains("web application")
            || _user.contains("Build a")
        {
            // milestone generator prompt
            Ok(r#"{"project_name": "Test Project", "milestones": [{"id": "ms1", "name": "Foundation", "phases": [{"id": "ph1", "name": "Setup", "goals": [{"id": "g1", "description": "Initialize project"}]}]}]}"#.to_string())
        } else {
            Ok("{}".to_string())
        }
    }

    fn get_params_for(&self, _agent_id: &str) -> (String, String, String) {
        (
            "mock_key".into(),
            "http://mock:8080".into(),
            "mock-model".into(),
        )
    }

    fn breaker(&self) -> sisyphus_tools_llm::circuit_breaker::CircuitBreaker {
        sisyphus_tools_llm::circuit_breaker::CircuitBreaker::default()
    }
}

/// Helper: create test manager with in-memory SQLite
async fn create_test_manager() -> (Arc<RoadmapManager>, String) {
    let store = Arc::new(
        MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
            .await
            .expect("Failed to create in-memory store"),
    );
    let sql_store = Arc::new(
        RoadmapSqlStore::new(store.pool().clone())
            .await
            .expect("Failed to create in-memory sql store"),
    );
    sql_store.init_schema().await.expect("Failed to init schema");
    let proposal_manager = Arc::new(ProposalManager::new(store.clone()));

    let manager = Arc::new(RoadmapManager::new(store, sql_store, proposal_manager));
    let project = manager
        .create_project(
            "test-project",
            "Test Project",
            "A test project for integration tests",
        )
        .await
        .unwrap();
    let project_id = project.id;
    (manager, project_id)
}

/// Helper: add milestone and goals
async fn setup_milestones_with_goals(
    manager: &Arc<RoadmapManager>,
    project_id: &str,
) -> (String, String) {
    let ms1_id = "ms1".to_string();
    let ms2_id = "ms2".to_string();

    manager
        .add_milestone(
            project_id,
            Milestone {
                id: ms1_id.clone(),
                name: "Milestone 1".to_string(),
                phases: vec![Phase {
                    id: "ms1-ph1".to_string(),
                    name: "Phase 1".to_string(),
                    goals: Vec::new(),
                    depends_on: Vec::new(),
                }],
                status: MilestoneStatus::Pending,
            },
        )
        .await
        .unwrap();
    manager
        .add_milestone(
            project_id,
            Milestone {
                id: ms2_id.clone(),
                name: "Milestone 2".to_string(),
                phases: vec![Phase {
                    id: "ms2-ph1".to_string(),
                    name: "Phase 1".to_string(),
                    goals: Vec::new(),
                    depends_on: Vec::new(),
                }],
                status: MilestoneStatus::Pending,
            },
        )
        .await
        .unwrap();

    for (gid, desc) in [("g1", "Goal 1"), ("g2", "Goal 2"), ("g3", "Goal 3")] {
        manager
            .add_goal(
                project_id,
                &ms1_id,
                "ms1-ph1",
                GoalRef {
                    goal_id: gid.to_string(),
                    description: desc.to_string(),
                    status: MilestoneStatus::Pending,
                },
            )
            .await
            .unwrap();
    }

    (ms1_id, ms2_id)
}

/// Test: milestone generator produces valid proposals
#[tokio::test]
async fn test_milestone_generator_generate() {
    let (manager, project_id) = create_test_manager().await;
    let llm = Arc::new(MockLlmClient);
    let generator = MilestoneGenerator::new(llm);

    let milestones = generator
        .generate_milestones(&manager, &project_id, "Build a web application")
        .await
        .unwrap();

    assert!(!milestones.is_empty());
    assert_eq!(milestones[0].id, "ms1");
    assert_eq!(milestones[0].name, "Foundation");
}

/// Test: milestone generator proposes refinements
#[tokio::test]
async fn test_milestone_generator_propose_refinements() {
    let (manager, project_id) = create_test_manager().await;
    let (ms1_id, _ms2_id) = setup_milestones_with_goals(&manager, &project_id).await;

    let llm = Arc::new(MockLlmClient);
    let generator = MilestoneGenerator::new(llm);

    let suggestions = generator
        .propose_refinements(&manager, &project_id, &ms1_id, "Goals are too large")
        .await
        .unwrap();

    assert!(!suggestions.is_empty());
    assert_eq!(suggestions[0].action, "Split goal");
}

/// Test: strategic reflector produces valid reflection
#[tokio::test]
async fn test_strategic_reflector_reflect() {
    let (manager, project_id) = create_test_manager().await;
    let (ms1_id, _ms2_id) = setup_milestones_with_goals(&manager, &project_id).await;

    // Record some execution outcomes
    let now = 1000;
    for (goal_id, duration, success) in [
        ("g1", 100.0, true),
        ("g2", 120.0, true),
        ("g3", 80.0, false),
    ] {
        let outcome = ExecutionOutcome {
            goal_id: goal_id.to_string(),
            project_id: project_id.clone(),
            milestone_id: ms1_id.clone(),
            phase_id: "ms1-ph1".to_string(),
            success,
            deviation_score: Some(0.1),
            error: if success {
                None
            } else {
                Some("timeout".into())
            },
            timestamp: now,
            actual_cost: 5.0,
            duration_secs: duration,
        };
        manager.record_outcome(&outcome).await.unwrap();
    }

    let llm = Arc::new(MockLlmClient);
    let reflector = StrategicReflector::new(llm);

    let reflection: StrategicReflection = reflector.reflect(&manager, &project_id).await.unwrap();

    assert_eq!(reflection.project_id, project_id);
    assert!(!reflection.recommendations.is_empty());
    assert!(reflection.confidence > 0.0);
}

/// Test: proposal manager creates and lists proposals
#[tokio::test]
async fn test_proposal_manager_create_and_list() {
    let (manager, project_id) = create_test_manager().await;
    let proposal_mgr = manager.proposal_manager();

    // Create a proposal
    let proposal = StrategicProposal {
        proposal_id: "test-proposal-1".to_string(),
        project_id: project_id.clone(),
        timestamp: 1000,
        trigger_reflection: StrategicReflection {
            project_id: project_id.clone(),
            project_name: "Test".to_string(),
            health_assessment: "OK".to_string(),
            trajectory: "On track".to_string(),
            risk_factors: vec![],
            recommendations: vec![],
            confidence: 0.8,
        },
        action: ProposalAction::PivotTrajectory {
            new_trajectory: "Test pivot".to_string(),
            rationale: "Test".to_string(),
        },
        expected_impact: "Test impact".to_string(),
        status: ProposalStatus::Pending,
        reviewer: None,
    };

    proposal_mgr.submit_proposal(&project_id, proposal.clone()).await.unwrap();

    // Get proposal
    let retrieved = proposal_mgr.get_proposal(&project_id, "test-proposal-1").await.unwrap();
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().proposal_id, "test-proposal-1");
}

/// Test: proposal manager approves and rejects
#[tokio::test]
async fn test_proposal_manager_approve_reject() {
    let (manager, project_id) = create_test_manager().await;
    let proposal_mgr = manager.proposal_manager();

    let proposal = StrategicProposal {
        proposal_id: "test-proposal-2".to_string(),
        project_id: project_id.clone(),
        timestamp: 1000,
        trigger_reflection: StrategicReflection {
            project_id: project_id.clone(),
            project_name: "Test".to_string(),
            health_assessment: "OK".to_string(),
            trajectory: "On track".to_string(),
            risk_factors: vec![],
            recommendations: vec![],
            confidence: 0.8,
        },
        action: ProposalAction::PivotTrajectory {
            new_trajectory: "Test pivot 2".to_string(),
            rationale: "Test".to_string(),
        },
        expected_impact: "Test impact".to_string(),
        status: ProposalStatus::Pending,
        reviewer: None,
    };

    proposal_mgr.submit_proposal(&project_id, proposal.clone()).await.unwrap();

    // Approve
    proposal_mgr.approve_proposal(&project_id, "test-proposal-2", "reviewer1").await.unwrap();
    let approved =
        proposal_mgr.get_proposal(&project_id, "test-proposal-2").await.unwrap().unwrap();
    assert_eq!(approved.status, ProposalStatus::Approved);
    assert_eq!(approved.reviewer, Some("reviewer1".to_string()));

    // Create another and reject
    let proposal2 = StrategicProposal {
        proposal_id: "test-proposal-3".to_string(),
        project_id: project_id.clone(),
        timestamp: 1001,
        trigger_reflection: StrategicReflection {
            project_id: project_id.clone(),
            project_name: "Test".to_string(),
            health_assessment: "OK".to_string(),
            trajectory: "On track".to_string(),
            risk_factors: vec![],
            recommendations: vec![],
            confidence: 0.8,
        },
        action: ProposalAction::PivotTrajectory {
            new_trajectory: "Test pivot 3".to_string(),
            rationale: "Test".to_string(),
        },
        expected_impact: "Test impact".to_string(),
        status: ProposalStatus::Pending,
        reviewer: None,
    };

    proposal_mgr.submit_proposal(&project_id, proposal2.clone()).await.unwrap();

    proposal_mgr.reject_proposal(&project_id, "test-proposal-3", "reviewer2").await.unwrap();
    let rejected =
        proposal_mgr.get_proposal(&project_id, "test-proposal-3").await.unwrap().unwrap();
    assert_eq!(rejected.status, ProposalStatus::Rejected);
    assert_eq!(rejected.reviewer, Some("reviewer2".to_string()));
}

/// Test: progress reporting
#[tokio::test]
async fn test_progress_reporting() {
    let (manager, project_id) = create_test_manager().await;
    let (ms1_id, ms2_id) = setup_milestones_with_goals(&manager, &project_id).await;

    // Complete some goals
    manager
        .update_goal_status_with_propagation(
            &project_id,
            &ms1_id,
            "ms1-ph1",
            "g1",
            MilestoneStatus::Completed,
        )
        .await
        .unwrap();
    manager
        .update_goal_status_with_propagation(
            &project_id,
            &ms1_id,
            "ms1-ph1",
            "g2",
            MilestoneStatus::Completed,
        )
        .await
        .unwrap();

    let report = manager.generate_progress_report(&project_id).await.unwrap();

    assert_eq!(report.project_id, project_id);
    assert_eq!(report.project_name, "Test Project");
    assert_eq!(report.completed_goals, 2);
    assert_eq!(report.total_goals, 3);
    assert!((report.overall_progress - 0.66666).abs() < 0.01);

    // Check milestone progress
    let ms1_progress = report.milestones.iter().find(|m| m.milestone_id == ms1_id).unwrap();
    assert_eq!(ms1_progress.completed_count, 2);
    assert_eq!(ms1_progress.goal_count, 3);
}

/// Test: health check
#[tokio::test]
async fn test_health_check() {
    let (manager, project_id) = create_test_manager().await;
    let (ms1_id, _ms2_id) = setup_milestones_with_goals(&manager, &project_id).await;

    // Add some failures
    let now = 1000;
    let outcome = ExecutionOutcome {
        goal_id: "g1".to_string(),
        project_id: project_id.clone(),
        milestone_id: ms1_id.clone(),
        phase_id: "ms1-ph1".to_string(),
        success: false,
        deviation_score: Some(0.5),
        error: Some("test error".into()),
        timestamp: now,
        actual_cost: 0.0,
        duration_secs: 10.0,
    };
    manager.record_outcome(&outcome).await.unwrap();

    let health = manager.evaluate_health(&project_id).await.unwrap();

    assert_eq!(health.project_id, project_id);
    // Failures only generate suggestions, not problems - health is still "healthy"
    assert!(health.healthy);
    assert!(health.recent_failures > 0);
    // But suggestions should be generated
    assert!(!health.suggestions.is_empty());
}

/// Test: resource optimizer bottleneck detection with multiple goals
#[tokio::test]
async fn test_resource_optimizer_bottlenecks_multiple_goals() {
    let (manager, project_id) = create_test_manager().await;
    let (ms1_id, _ms2_id) = setup_milestones_with_goals(&manager, &project_id).await;

    let now = 1000;
    // g1: slow (potential bottleneck)
    for i in 0..3 {
        let outcome = ExecutionOutcome {
            goal_id: format!("g{}", i + 1),
            project_id: project_id.clone(),
            milestone_id: ms1_id.clone(),
            phase_id: "ms1-ph1".to_string(),
            success: true,
            deviation_score: Some(0.1),
            error: None,
            timestamp: now + i,
            actual_cost: 10.0,
            duration_secs: if i == 0 { 500.0 } else { 50.0 }, // g1 is slow
        };
        manager.record_outcome(&outcome).await.unwrap();
    }

    let bottlenecks = manager.find_bottlenecks(&project_id).await.unwrap();

    // g1 should be detected as bottleneck
    assert!(bottlenecks.iter().any(|b| b.goal_id == "g1"));
}

/// Test: parallel execution suggestions
#[tokio::test]
async fn test_parallel_execution_suggestions() {
    let (manager, project_id) = create_test_manager().await;
    let (ms1_id, _ms2_id) = setup_milestones_with_goals(&manager, &project_id).await;

    let now = 1000;
    for (goal_id, duration) in [("g1", 30.0), ("g2", 60.0), ("g3", 90.0), ("g4", 45.0)] {
        let outcome = ExecutionOutcome {
            goal_id: goal_id.to_string(),
            project_id: project_id.clone(),
            milestone_id: ms1_id.clone(),
            phase_id: "ms1-ph1".to_string(),
            success: true,
            deviation_score: None,
            error: None,
            timestamp: now,
            actual_cost: 1.0,
            duration_secs: duration,
        };
        manager.record_outcome(&outcome).await.unwrap();
    }

    // Mark as completed
    for gid in ["g1", "g2", "g3", "g4"] {
        manager
            .update_goal_status_with_propagation(
                &project_id,
                &ms1_id,
                "ms1-ph1",
                gid,
                MilestoneStatus::Completed,
            )
            .await
            .unwrap();
    }

    let suggestions = manager.suggest_parallel_execution(&project_id).await.unwrap();

    assert!(!suggestions.is_empty());
    for s in suggestions {
        assert!(s.goal_ids.len() >= 2);
        assert!(s.estimated_time_savings_secs > 0.0);
        assert!(!s.rationale.is_empty());
    }
}

/// Test: goal resource estimates
#[tokio::test]
async fn test_goal_resource_estimates() {
    let (manager, project_id) = create_test_manager().await;
    let (ms1_id, _ms2_id) = setup_milestones_with_goals(&manager, &project_id).await;

    let now = 1000;
    // Add history for g1
    let outcome = ExecutionOutcome {
        goal_id: "g1".to_string(),
        project_id: project_id.clone(),
        milestone_id: ms1_id.clone(),
        phase_id: "ms1-ph1".to_string(),
        success: true,
        deviation_score: None,
        error: None,
        timestamp: now,
        actual_cost: 6.0,
        duration_secs: 45.0,
    };
    manager.record_outcome(&outcome).await.unwrap();

    let estimates = manager
        .estimate_goal_resources(&project_id, &["g1".to_string(), "g2".to_string()])
        .await
        .unwrap();

    assert_eq!(estimates.len(), 2);
    let g1_est = estimates.iter().find(|e| e.goal_id == "g1").unwrap();
    assert!((g1_est.predicted_duration_secs - 45.0).abs() < 0.01);
    assert!((g1_est.predicted_cost - 6.0).abs() < 0.01);
    assert_eq!(g1_est.historical_samples, 1);

    // g2 has no history
    let g2_est = estimates.iter().find(|e| e.goal_id == "g2").unwrap();
    assert_eq!(g2_est.historical_samples, 0);
}

/// Test: project cost projection
#[tokio::test]
async fn test_project_cost_projection() {
    let (manager, project_id) = create_test_manager().await;
    let (ms1_id, _ms2_id) = setup_milestones_with_goals(&manager, &project_id).await;

    let now = 1000;
    for (goal_id, duration, cost) in [("g1", 100.0, 5.0), ("g2", 120.0, 6.0)] {
        let outcome = ExecutionOutcome {
            goal_id: goal_id.to_string(),
            project_id: project_id.clone(),
            milestone_id: ms1_id.clone(),
            phase_id: "ms1-ph1".to_string(),
            success: true,
            deviation_score: None,
            error: None,
            timestamp: now,
            actual_cost: cost,
            duration_secs: duration,
        };
        manager.record_outcome(&outcome).await.unwrap();
    }

    // Mark completed
    for gid in ["g1", "g2"] {
        manager
            .update_goal_status_with_propagation(
                &project_id,
                &ms1_id,
                "ms1-ph1",
                gid,
                MilestoneStatus::Completed,
            )
            .await
            .unwrap();
    }

    // Total 5 goals, completed 2
    let projection = manager.project_costs(&project_id, 5, 2).await.unwrap();

    assert_eq!(projection.total_goals, 5);
    assert_eq!(projection.completed_goals, 2);
    assert_eq!(projection.remaining_goals, 3);
    assert!((projection.avg_cost_per_goal - 5.5).abs() < 0.01);
    assert!((projection.avg_duration_per_goal_secs - 110.0).abs() < 0.01);
    assert!((projection.projected_remaining_cost - 16.5).abs() < 0.01);
    assert!((projection.projected_remaining_duration_secs - 330.0).abs() < 0.01);
}

/// Test: completion time prediction
#[tokio::test]
async fn test_completion_time_prediction() {
    let (manager, project_id) = create_test_manager().await;
    let (ms1_id, _ms2_id) = setup_milestones_with_goals(&manager, &project_id).await;

    let now = 1000;
    for (goal_id, duration) in [("g1", 100.0), ("g2", 120.0)] {
        let outcome = ExecutionOutcome {
            goal_id: goal_id.to_string(),
            project_id: project_id.clone(),
            milestone_id: ms1_id.clone(),
            phase_id: "ms1-ph1".to_string(),
            success: true,
            deviation_score: None,
            error: None,
            timestamp: now,
            actual_cost: 5.0,
            duration_secs: duration,
        };
        manager.record_outcome(&outcome).await.unwrap();
    }

    for gid in ["g1", "g2"] {
        manager
            .update_goal_status_with_propagation(
                &project_id,
                &ms1_id,
                "ms1-ph1",
                gid,
                MilestoneStatus::Completed,
            )
            .await
            .unwrap();
    }

    let prediction = manager.predict_completion_time(&project_id).await.unwrap();

    assert_eq!(prediction.project_id, project_id);
    assert_eq!(prediction.project_name, "Test Project");
    assert!(prediction.overall_velocity_goals_per_sec > 0.0);
    assert!(!prediction.milestone_predictions.is_empty());
}

/// Test: delay alerts
#[tokio::test]
async fn test_delay_alerts() {
    let (manager, project_id) = create_test_manager().await;
    let (ms1_id, _ms2_id) = setup_milestones_with_goals(&manager, &project_id).await;

    // Add history
    let now = 1000;
    for i in 0..5 {
        let outcome = ExecutionOutcome {
            goal_id: format!("hist_{}", i),
            project_id: project_id.clone(),
            milestone_id: ms1_id.clone(),
            phase_id: "ms1-ph1".to_string(),
            success: true,
            deviation_score: None,
            error: None,
            timestamp: now,
            actual_cost: 1.0,
            duration_secs: 10.0,
        };
        manager.record_outcome(&outcome).await.unwrap();
    }

    // Mark original goals as completed
    for gid in ["g1", "g2", "g3"] {
        manager
            .update_goal_status_with_propagation(
                &project_id,
                &ms1_id,
                "ms1-ph1",
                gid,
                MilestoneStatus::Completed,
            )
            .await
            .unwrap();
    }

    let alerts = manager.check_delay_alerts(&project_id, Some(2.0)).await.unwrap();

    // ms1 is done, should not alert
    assert!(alerts.is_empty() || alerts.iter().all(|a| a.milestone_id != ms1_id));
}

/// Test: persistence across restarts (using temp file)
#[tokio::test]
async fn test_persistence_across_restart() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("test.db");
    let db_url = format!("sqlite:{}", db_path.display());

    // First manager session
    {
        let store = Arc::new(
            MfsStore::new(&db_url, &sisyphus_types::CacheConfig::default()).await.unwrap(),
        );
        let sql_store = Arc::new(RoadmapSqlStore::new(store.pool().clone()).await.unwrap());
        sql_store.init_schema().await.unwrap();
        let proposal_manager = Arc::new(ProposalManager::new(store.clone()));
        let manager = Arc::new(RoadmapManager::new(store, sql_store, proposal_manager));

        let project = manager
            .create_project("persist-test", "Persist Test", "Testing persistence")
            .await
            .unwrap();
        let project_id = project.id;

        let ms1_id = "ms1".to_string();
        manager
            .add_milestone(
                &project_id,
                Milestone {
                    id: ms1_id.clone(),
                    name: "MS 1".to_string(),
                    phases: vec![Phase {
                        id: "ph1".to_string(),
                        name: "Phase 1".to_string(),
                        goals: Vec::new(),
                        depends_on: Vec::new(),
                    }],
                    status: MilestoneStatus::Pending,
                },
            )
            .await
            .unwrap();

        manager
            .add_goal(
                &project_id,
                &ms1_id,
                "ph1",
                GoalRef {
                    goal_id: "g1".to_string(),
                    description: "Goal 1".to_string(),
                    status: MilestoneStatus::Completed,
                },
            )
            .await
            .unwrap();

        // Record outcome
        let outcome = ExecutionOutcome {
            goal_id: "g1".to_string(),
            project_id: project_id.clone(),
            milestone_id: ms1_id.clone(),
            phase_id: "ph1".to_string(),
            success: true,
            deviation_score: None,
            error: None,
            timestamp: 1000,
            actual_cost: 10.0,
            duration_secs: 100.0,
        };
        manager.record_outcome(&outcome).await.unwrap();
    }

    // Second manager session - load from same DB
    {
        let store = Arc::new(
            MfsStore::new(&db_url, &sisyphus_types::CacheConfig::default()).await.unwrap(),
        );
        let sql_store = Arc::new(RoadmapSqlStore::new(store.pool().clone()).await.unwrap());
        let proposal_manager = Arc::new(ProposalManager::new(store.clone()));
        let manager = Arc::new(RoadmapManager::new(store, sql_store, proposal_manager));

        let project = manager.get_project("persist-test").await.unwrap().unwrap();
        assert_eq!(project.name, "Persist Test");

        let report = manager.generate_progress_report("persist-test").await.unwrap();
        assert_eq!(report.completed_goals, 1);
    }
}

/// Test: list project IDs
#[tokio::test]
async fn test_list_project_ids() {
    let (manager, project_id) = create_test_manager().await;

    let project2 =
        manager.create_project("project-2", "Project 2", "Second project").await.unwrap();

    let project3 = manager.create_project("project-3", "Project 3", "Third project").await.unwrap();

    let ids = manager.list_project_ids().await.unwrap();
    assert!(ids.contains(&project_id));
    assert!(ids.contains(&project2.id));
    assert!(ids.contains(&project3.id));
    assert_eq!(ids.len(), 3);
}

/// Test: delete project
#[tokio::test]
async fn test_delete_project() {
    let (manager, project_id) = create_test_manager().await;

    let project2 =
        manager.create_project("project-to-delete", "To Delete", "Will be deleted").await.unwrap();

    let ids_before = manager.list_project_ids().await.unwrap();
    assert!(ids_before.contains(&project2.id));

    manager.delete_project(&project2.id).await.unwrap();

    let ids_after = manager.list_project_ids().await.unwrap();
    assert!(!ids_after.contains(&project2.id));
    assert_eq!(ids_after.len(), ids_before.len() - 1);
}

/// Test: concurrent access to roadmap manager
#[tokio::test]
async fn test_concurrent_roadmap_access() {
    let (manager, project_id) = create_test_manager().await;
    let (ms1_id, _ms2_id) = setup_milestones_with_goals(&manager, &project_id).await;

    let manager = Arc::new(manager);
    let mut handles = vec![];

    // Spawn tasks that add goals and update status
    for i in 10..20 {
        let mgr = manager.clone();
        let pid = project_id.clone();
        let msid = ms1_id.clone();
        let handle = tokio::spawn(async move {
            let gid = format!("concurrent_g{}", i);
            mgr.add_goal(
                &pid,
                &msid,
                "ms1-ph1",
                GoalRef {
                    goal_id: gid.clone(),
                    description: format!("Concurrent goal {}", i),
                    status: MilestoneStatus::Pending,
                },
            )
            .await
            .unwrap();

            // Small delay
            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;

            mgr.update_goal_status_with_propagation(
                &pid,
                &msid,
                "ms1-ph1",
                &gid,
                MilestoneStatus::Completed,
            )
            .await
            .unwrap();
        });
        handles.push(handle);
    }

    for h in handles {
        h.await.unwrap();
    }

    // Verify all goals added
    let report = manager.generate_progress_report(&project_id).await.unwrap();
    // Original 3 (pending) + 10 concurrent (completed) = 13 total, 10 completed
    assert_eq!(report.total_goals, 13);
    assert_eq!(report.completed_goals, 10); // Only the 10 concurrent ones are completed
}
