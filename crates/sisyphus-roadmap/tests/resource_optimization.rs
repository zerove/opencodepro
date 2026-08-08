//! 集成测试：资源优化功能（成本预测、瓶颈识别、并行化建议、目标资源估算）

use sisyphus_mem::{MfsStore, RoadmapSqlStore};
use sisyphus_roadmap::{
    proposal_manager::ProposalManager, Bottleneck, CostProjection, ExecutionOutcome,
    GoalResourceEstimate, ParallelizationOpportunity, RoadmapManager,
};
use sisyphus_types::{GoalRef, Milestone, MilestoneStatus, Phase};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// 辅助：创建测试管理器
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
    sql_store
        .init_schema()
        .await
        .expect("Failed to init schema");
    let proposal_manager = Arc::new(ProposalManager::new(store.clone()));

    let manager = Arc::new(RoadmapManager::new(store, sql_store, proposal_manager));
    let project = manager
        .create_project(
            "test-project",
            "Test Project",
            "A test project for resource optimization",
        )
        .await
        .unwrap();
    let project_id = project.id;
    (manager, project_id)
}

/// 辅助：添加里程碑和目标
async fn setup_milestones_with_goals(
    manager: &Arc<RoadmapManager>,
    project_id: &str,
) -> (String, String) {
    let ms1_id = "ms1".to_string();
    let ms2_id = "ms2".to_string();

    // 添加里程碑 1
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
    // 添加里程碑 2
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

    // 在 ms1 中添加目标
    let g1_id = "g1".to_string();
    let g2_id = "g2".to_string();
    let g3_id = "g3".to_string();
    let g4_id = "g4".to_string();
    let g5_id = "g5".to_string();
    let g6_id = "g6".to_string();

    for (gid, desc) in [
        (&g1_id, "Goal 1"),
        (&g2_id, "Goal 2"),
        (&g3_id, "Goal 3"),
        (&g4_id, "Goal 4"),
        (&g5_id, "Goal 5"),
        (&g6_id, "Goal 6"),
    ] {
        manager
            .add_goal(
                project_id,
                &ms1_id,
                "ms1-ph1",
                GoalRef {
                    goal_id: gid.clone(),
                    description: desc.to_string(),
                    status: MilestoneStatus::Pending,
                },
            )
            .await
            .unwrap();
    }

    // 在 ms2 中添加目标
    manager
        .add_goal(
            project_id,
            &ms2_id,
            "ms2-ph1",
            GoalRef {
                goal_id: "g7".to_string(),
                description: "Goal 7".to_string(),
                status: MilestoneStatus::Pending,
            },
        )
        .await
        .unwrap();

    (ms1_id, ms2_id)
}

/// 测试：成本预测（无历史数据 - 使用默认值）
#[tokio::test]
async fn test_project_costs_empty() {
    let (manager, project_id) = create_test_manager().await;
    let (_ms1_id, _ms2_id) = setup_milestones_with_goals(&manager, &project_id).await;

    // 无历史数据，预测总目标 7，已完成 0
    let projection = manager.project_costs(&project_id, 7, 0).await.unwrap();

    assert_eq!(projection.total_goals, 7);
    assert_eq!(projection.completed_goals, 0);
    assert_eq!(projection.remaining_goals, 7);
    assert!(projection.avg_cost_per_goal > 0.0);
    assert!(projection.avg_duration_per_goal_secs > 0.0);
    assert!(projection.projected_remaining_cost > 0.0);
    assert!(projection.projected_remaining_duration_secs > 0.0);
    assert!(projection.confidence >= 0.1); // 最小置信度
}

/// 测试：成本预测（有历史数据）
#[tokio::test]
async fn test_project_costs_with_history() {
    let (manager, project_id) = create_test_manager().await;
    let (ms1_id, _ms2_id) = setup_milestones_with_goals(&manager, &project_id).await;

    // 记录一些历史执行结果
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    for (goal_id, duration, cost) in [("g1", 100.0, 5.0), ("g2", 120.0, 6.0), ("g3", 80.0, 4.0)] {
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

    // 标记为已完成
    for gid in ["g1", "g2", "g3"] {
        manager
            .update_goal_status_with_propagation(
                &project_id,
                &ms1_id,
                "ms1-ph1",
                gid,
                sisyphus_types::MilestoneStatus::Completed,
            )
            .await
            .unwrap();
    }

    // 预测：总目标 7，已完成 3
    let projection = manager.project_costs(&project_id, 7, 3).await.unwrap();

    assert_eq!(projection.total_goals, 7);
    assert_eq!(projection.completed_goals, 3);
    assert_eq!(projection.remaining_goals, 4);

    // 平均成本 = (5+6+4)/3 = 5.0
    assert!((projection.avg_cost_per_goal - 5.0).abs() < 0.01);
    // 平均时长 = (100+120+80)/3 = 100.0
    assert!((projection.avg_duration_per_goal_secs - 100.0).abs() < 0.01);
    // 剩余成本 = 5.0 * 4 = 20.0
    assert!((projection.projected_remaining_cost - 20.0).abs() < 0.01);
    // 剩余时长 = 100.0 * 4 = 400.0
    assert!((projection.projected_remaining_duration_secs - 400.0).abs() < 0.01);
    // 置信度 = 3/7
    assert!((projection.confidence - (3.0 / 7.0)).abs() < 0.01);
}

/// 测试：瓶颈识别（无历史数据）
#[tokio::test]
async fn test_find_bottlenecks_empty() {
    let (manager, project_id) = create_test_manager().await;
    let (_ms1_id, _ms2_id) = setup_milestones_with_goals(&manager, &project_id).await;

    let bottlenecks = manager.find_bottlenecks(&project_id).await.unwrap();
    assert!(bottlenecks.is_empty());
}

/// 测试：瓶颈识别（有失败历史）
#[tokio::test]
async fn test_find_bottlenecks_with_failures() {
    let (manager, project_id) = create_test_manager().await;
    let (ms1_id, _ms2_id) = setup_milestones_with_goals(&manager, &project_id).await;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // g1: 3 次失败（使用不同 goal_id 记录失败，因为 record_outcome 只保留最新）
    // 但我们可以通过多次调用 record_outcome 来覆盖，但最终只保留最后一个
    // 这里我们需要测试逻辑：如果同一个 goal 有多次失败记录（通过不同方式记录）
    // 实际上 record_outcome 只保留每个 goal 的最新结果
    // 我们改用不同的 goal 来测试：g1 有失败，g2 有成功

    // g1: 失败
    let outcome = ExecutionOutcome {
        goal_id: "g1".to_string(),
        project_id: project_id.clone(),
        milestone_id: ms1_id.clone(),
        phase_id: "ms1-ph1".to_string(),
        success: false,
        deviation_score: None,
        error: Some("error".to_string()),
        timestamp: now,
        actual_cost: 0.0,
        duration_secs: 10.0,
    };
    manager.record_outcome(&outcome).await.unwrap();

    // g1: 成功（覆盖之前的失败） - 这样 record_outcome 只会存成功
    // 所以这里我们不覆盖，而是用另一个 goal

    // g2: 正常成功
    let outcome = ExecutionOutcome {
        goal_id: "g2".to_string(),
        project_id: project_id.clone(),
        milestone_id: ms1_id.clone(),
        phase_id: "ms1-ph1".to_string(),
        success: true,
        deviation_score: None,
        error: None,
        timestamp: now,
        actual_cost: 1.0,
        duration_secs: 30.0,
    };
    manager.record_outcome(&outcome).await.unwrap();

    let bottlenecks = manager.find_bottlenecks(&project_id).await.unwrap();

    // g1 有 1 次失败 -> low severity
    // g2 正常 -> 不是瓶颈
    assert!(!bottlenecks.is_empty());
    assert_eq!(bottlenecks[0].goal_id, "g1");
    assert_eq!(bottlenecks[0].severity, "low");
    assert_eq!(bottlenecks[0].failure_count, 1);
}

/// 测试：并行化建议
#[tokio::test]
async fn test_suggest_parallel_execution() {
    let (manager, project_id) = create_test_manager().await;
    let (ms1_id, _ms2_id) = setup_milestones_with_goals(&manager, &project_id).await;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // 在同一个里程碑中成功完成多个目标（需要至少 4 个 outcomes 才会产生建议）
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

    // 标记为已完成
    for gid in ["g1", "g2", "g3", "g4"] {
        manager
            .update_goal_status_with_propagation(
                &project_id,
                &ms1_id,
                "ms1-ph1",
                gid,
                sisyphus_types::MilestoneStatus::Completed,
            )
            .await
            .unwrap();
    }

    let suggestions = manager
        .suggest_parallel_execution(&project_id)
        .await
        .unwrap();

    // ms1 有 4 个成功目标 -> 应该有并行化建议
    assert!(suggestions.iter().any(|s| s.goal_ids.len() >= 2));
    // 检查节省时间计算
    for s in &suggestions {
        assert!(s.estimated_time_savings_secs > 0.0);
        assert!(!s.rationale.is_empty());
    }
}

/// 测试：目标资源估算（有历史数据）
#[tokio::test]
async fn test_estimate_goal_resources_with_history() {
    let (manager, project_id) = create_test_manager().await;
    let (ms1_id, _ms2_id) = setup_milestones_with_goals(&manager, &project_id).await;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // record_outcome 只保留每个 goal 的最新结果
    // 所以我们需要创建多个 goal 来测试
    // g1: 1 次成功，时长 45s，成本 6
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

    // g2: 1 次成功，时长 90s，成本 10
    let outcome = ExecutionOutcome {
        goal_id: "g2".to_string(),
        project_id: project_id.clone(),
        milestone_id: ms1_id.clone(),
        phase_id: "ms1-ph1".to_string(),
        success: true,
        deviation_score: None,
        error: None,
        timestamp: now,
        actual_cost: 10.0,
        duration_secs: 90.0,
    };
    manager.record_outcome(&outcome).await.unwrap();

    let estimates = manager
        .estimate_goal_resources(
            &project_id,
            &["g1".to_string(), "g2".to_string(), "g3".to_string()],
        )
        .await
        .unwrap();

    assert_eq!(estimates.len(), 3);

    // g1 有 1 个样本
    let g1 = estimates.iter().find(|e| e.goal_id == "g1").unwrap();
    assert!((g1.predicted_duration_secs - 45.0).abs() < 0.01);
    assert!((g1.predicted_cost - 6.0).abs() < 0.01);
    assert_eq!(g1.historical_samples, 1);
    assert!(g1.confidence > 0.0);

    // g2 有 1 个样本
    let g2 = estimates.iter().find(|e| e.goal_id == "g2").unwrap();
    assert!((g2.predicted_duration_secs - 90.0).abs() < 0.01);
    assert!((g2.predicted_cost - 10.0).abs() < 0.01);
    assert_eq!(g2.historical_samples, 1);

    // g3 无样本 -> 使用全局平均
    let g3 = estimates.iter().find(|e| e.goal_id == "g3").unwrap();
    assert_eq!(g3.historical_samples, 0);
    assert!(g3.confidence < 0.5); // 低置信度
}

/// 测试：目标资源估算（无历史数据）
#[tokio::test]
async fn test_estimate_goal_resources_no_history() {
    let (manager, project_id) = create_test_manager().await;
    let (_ms1_id, _ms2_id) = setup_milestones_with_goals(&manager, &project_id).await;

    let estimates = manager
        .estimate_goal_resources(&project_id, &["g1".to_string(), "g2".to_string()])
        .await
        .unwrap();

    assert_eq!(estimates.len(), 2);
    for e in estimates {
        assert_eq!(e.historical_samples, 0);
        assert!(e.confidence < 0.5);
    }
}

/// 测试：Bottleneck 结构体序列化
#[tokio::test]
async fn test_bottleneck_serialization() {
    let bottleneck = Bottleneck {
        goal_id: "g1".to_string(),
        description: "Test bottleneck".to_string(),
        failure_count: 3,
        avg_duration_secs: 100.0,
        blocking_count: 2,
        severity: "high".to_string(),
    };

    let json = serde_json::to_string(&bottleneck).unwrap();
    let deserialized: Bottleneck = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.goal_id, "g1");
    assert_eq!(deserialized.severity, "high");
    assert_eq!(deserialized.failure_count, 3);
}

/// 测试：CostProjection 结构体序列化
#[tokio::test]
async fn test_cost_projection_serialization() {
    let proj = CostProjection {
        total_goals: 10,
        completed_goals: 3,
        remaining_goals: 7,
        avg_cost_per_goal: 5.5,
        avg_duration_per_goal_secs: 120.0,
        projected_remaining_cost: 38.5,
        projected_remaining_duration_secs: 840.0,
        confidence: 0.3,
    };

    let json = serde_json::to_string(&proj).unwrap();
    let deserialized: CostProjection = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.total_goals, 10);
    assert_eq!(deserialized.completed_goals, 3);
    assert!((deserialized.projected_remaining_cost - 38.5).abs() < 0.01);
}

/// 测试：ParallelizationOpportunity 结构体序列化
#[tokio::test]
async fn test_parallelization_opportunity_serialization() {
    let opp = ParallelizationOpportunity {
        goal_ids: vec!["g1".to_string(), "g2".to_string()],
        rationale: "Test rationale".to_string(),
        estimated_time_savings_secs: 45.0,
    };

    let json = serde_json::to_string(&opp).unwrap();
    let deserialized: ParallelizationOpportunity = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.goal_ids.len(), 2);
    assert!((deserialized.estimated_time_savings_secs - 45.0).abs() < 0.01);
}

/// 测试：GoalResourceEstimate 结构体序列化
#[tokio::test]
async fn test_goal_resource_estimate_serialization() {
    let est = GoalResourceEstimate {
        goal_id: "g1".to_string(),
        predicted_cost: 10.5,
        predicted_duration_secs: 200.0,
        confidence: 0.8,
        historical_samples: 4,
    };

    let json = serde_json::to_string(&est).unwrap();
    let deserialized: GoalResourceEstimate = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.goal_id, "g1");
    assert!((deserialized.predicted_cost - 10.5).abs() < 0.01);
    assert_eq!(deserialized.historical_samples, 4);
}
