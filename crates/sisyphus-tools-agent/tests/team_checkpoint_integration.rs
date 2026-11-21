//! Integration test for team checkpoint persistence and resume.

use serde_json::json;
use sisyphus_tools_agent::team::{
    checkpoint::TeamCheckpoint, ConflictStats, TaskNode, TaskResult, TeamContext, TeamPlan,
};

#[tokio::test]
async fn test_team_checkpoint_persistence_and_resume() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let ckpt_path = tmp.path().join("team_checkpoint.json");

    // 1. Setup a plan
    let plan = TeamPlan {
        goal_id: "test_goal".into(),
        tasks: vec![
            TaskNode {
                id: "task1".into(),
                agent_id: "agent1".into(),
                tool_id: "tool1".into(),
                args: json!({}),
                depends_on: vec![],
                read_keys: vec![],
                write_keys: vec!["data".into()],
            },
            TaskNode {
                id: "task2".into(),
                agent_id: "agent1".into(),
                tool_id: "tool1".into(),
                args: json!({}),
                depends_on: vec!["task1".into()],
                read_keys: vec!["data".into()],
                write_keys: vec![],
            },
        ],
    };

    // 2. Setup context and initial state
    let ctx = TeamContext::new();
    ctx.write("data".into(), json!(42), "task1");
    let completed = vec![TaskResult {
        task_id: "task1".into(),
        agent_id: "agent1".into(),
        success: true,
        output: Some(json!({"result": 42})),
        error: None,
        duration_ms: 100,
    }];
    let stats = ConflictStats::default();

    // 3. Create and save checkpoint
    let ckpt = TeamCheckpoint::from_state("test_goal", plan, completed, stats, &ctx, 0);
    ckpt.save_to_file(&ckpt_path).expect("failed to save checkpoint");

    // 4. Load checkpoint
    let loaded = TeamCheckpoint::load_from_file(&ckpt_path).expect("failed to load checkpoint");
    assert_eq!(loaded.goal_id, "test_goal");
    assert_eq!(loaded.remaining_task_ids(), vec!["task2".to_string()]);

    // 5. Restore context and resume
    let restored_ctx = loaded.restore_context();
    assert_eq!(restored_ctx.read("data"), Some(json!(42)));

    // Ensure resume state is valid
    let resume = loaded.to_resume_state();
    assert_eq!(resume.completed_results.len(), 1);
    assert_eq!(resume.completed_results[0].task_id, "task1");
}
