//! Cross-Project State Isolation Integration Tests
//!
//! Tests that verify complete isolation between projects/workspaces at all storage layers:
//! - MfsStore: key prefixing for state (SOPs, sessions, todos, etc.)
//! - SopSqlStore: SOP isolation by project_id
//! - RoadmapSqlStore: milestone/phase/goal isolation by project_id
//! - WorkspaceManager: workspace switching and isolation

use sqlx::sqlite::SqlitePoolOptions;

use parking_lot::Mutex;
use sisyphus_core::SisyphusKernel;
use sisyphus_intel::{
    simulation::LlmLogicalSimulator, GoalDrivenSynthesizer, MonteCarloSimulator, SisyphusIntel,
    StandardEvolver, StandardReflector,
};
use sisyphus_mem::{MfsStore, SisyphusStore};
use sisyphus_tools_llm::{DummyEmbeddingClient, DummyLlmClient};
use sisyphus_types::{
    experience::ExperienceBank, experience::ExperienceTuple, StateStore, ToolExecutor, TypesError,
};
use std::sync::Arc;

// Mock implementations for testing
struct MockToolExecutor;

#[async_trait::async_trait]
impl ToolExecutor for MockToolExecutor {
    async fn execute(
        &self,
        _tool_id: &str,
        _input: Vec<u8>,
        _fuel: u64,
    ) -> Result<Vec<u8>, TypesError> {
        Ok(vec![])
    }
}

struct MockConsensus;

#[async_trait::async_trait]
impl sisyphus_core::Consensus for MockConsensus {
    async fn is_leader(&self) -> bool {
        true
    }
    async fn get_leader(&self) -> String {
        "node-1".into()
    }
    async fn propose(&self, _k: &str, _v: serde_json::Value) -> Result<(), TypesError> {
        Ok(())
    }
}

struct MockExperienceBank;

#[async_trait::async_trait]
impl sisyphus_types::experience::ExperienceBank for MockExperienceBank {
    async fn record(&self, _exp: ExperienceTuple) -> Result<(), TypesError> {
        Ok(())
    }
    async fn query_similar(
        &self,
        _query: sisyphus_types::experience::ExperienceQuery,
    ) -> Result<Vec<ExperienceTuple>, TypesError> {
        Ok(vec![])
    }
}

/// Setup a complete Sisyphus kernel with in-memory database for testing.
async fn setup_test_kernel() -> SisyphusKernel {
    let url = "sqlite::memory:";

    let store = SisyphusStore::new(url, &sisyphus_types::Config::default())
        .await
        .expect("Failed to init SisyphusStore");
    let state_store = store.state.clone();
    let sop_store = store.sop.clone();
    let roadmap_store = store.roadmap.clone();
    let vector_store = Arc::new(
        sisyphus_mem::VectorMemoryStore::new(store.pool()).await.expect("Vector store failed"),
    );

    let llm_client: Arc<dyn sisyphus_tools_llm::LlmClient> = Arc::new(DummyLlmClient);
    let embedding_client = Arc::new(DummyEmbeddingClient);
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(MockToolExecutor);

    let simulator = Arc::new(MonteCarloSimulator::with_state_store(
        100,
        state_store.clone(),
    ));
    let logical_simulator = Arc::new(LlmLogicalSimulator::new(llm_client.clone()));
    let reflector = Arc::new(StandardReflector);
    let evolver = Arc::new(StandardEvolver::new());
    let intel = Arc::new(SisyphusIntel::new(
        simulator,
        logical_simulator,
        reflector,
        evolver,
    ));

    let synthesizer = Arc::new(GoalDrivenSynthesizer::new(tool_executor.clone()));
    let experience_bank: Arc<dyn sisyphus_types::experience::ExperienceBank> =
        Arc::new(MockExperienceBank);
    let meta_learner = Arc::new(Mutex::new(
        sisyphus_tools_llm_tools::meta_learner::MetaLearner::new(".omo/meta_test"),
    ));
    let consensus: Arc<dyn sisyphus_core::Consensus> = Arc::new(MockConsensus);

    SisyphusKernel::new(
        state_store,        // 1. state_store
        sop_store,          // 2. sop_store
        vector_store,       // 3. vector_store
        experience_bank,    // 4. experience_bank
        tool_executor,      // 5. tool_executor
        intel,              // 6. intel
        llm_client.clone(), // 7. llm_client
        llm_client.clone(), // 8. fast_llm_client
        synthesizer,        // 9. synthesizer
        meta_learner,       // 10. meta_learner
        consensus,          // 11. consensus
        roadmap_store,      // 12. sql_store
        embedding_client,   // 13. embedding_client
    )
}

#[tokio::test]
async fn test_mfsstore_cross_project_key_prefixing() {
    // Test that MfsStore properly prefixes keys by project_id
    let store = MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
        .await
        .expect("Failed to init MfsStore");

    let project_a = "project_alpha";
    let project_b = "project_beta";
    let shared_key = "user_preference";
    let val_a = serde_json::json!({"theme": "dark", "lang": "rust"});
    let val_b = serde_json::json!({"theme": "light", "lang": "python"});

    // Project A sets value
    store.set_state_json(project_a, shared_key, val_a.clone()).await.expect("Set A failed");

    // Project B sets SAME key
    store.set_state_json(project_b, shared_key, val_b.clone()).await.expect("Set B failed");

    // Verify Project A reads its own value
    let result_a = store.get_state_json(project_a, shared_key).await.expect("Get A failed");
    assert_eq!(
        result_a,
        Some(val_a.clone()),
        "Project A should read its own value"
    );

    // Verify Project B reads its own value
    let result_b = store.get_state_json(project_b, shared_key).await.expect("Get B failed");
    assert_eq!(
        result_b,
        Some(val_b.clone()),
        "Project B should read its own value"
    );

    println!("MfsStore cross-project key isolation: ✓");
}

#[tokio::test]
async fn test_mfsstore_cross_project_multiple_keys() {
    // Test isolation with multiple keys per project
    let store = MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
        .await
        .expect("Failed to init MfsStore");

    let project_a = "project_alpha";
    let project_b = "project_beta";

    // Project A sets multiple keys
    store.set_state_json(project_a, "key1", serde_json::json!("a1")).await.expect("Set A1 failed");
    store.set_state_json(project_a, "key2", serde_json::json!("a2")).await.expect("Set A2 failed");
    store.set_state_json(project_a, "key3", serde_json::json!("a3")).await.expect("Set A3 failed");

    // Project B sets multiple keys (some same, some different)
    store.set_state_json(project_b, "key1", serde_json::json!("b1")).await.expect("Set B1 failed");
    store.set_state_json(project_b, "key2", serde_json::json!("b2")).await.expect("Set B2 failed");
    store.set_state_json(project_b, "key4", serde_json::json!("b4")).await.expect("Set B4 failed");

    // Verify all Project A keys
    assert_eq!(
        store.get_state_json(project_a, "key1").await.unwrap(),
        Some(serde_json::json!("a1"))
    );
    assert_eq!(
        store.get_state_json(project_a, "key2").await.unwrap(),
        Some(serde_json::json!("a2"))
    );
    assert_eq!(
        store.get_state_json(project_a, "key3").await.unwrap(),
        Some(serde_json::json!("a3"))
    );
    assert_eq!(
        store.get_state_json(project_a, "key4").await.unwrap(),
        None,
        "Project A should not have key4"
    );

    // Verify all Project B keys
    assert_eq!(
        store.get_state_json(project_b, "key1").await.unwrap(),
        Some(serde_json::json!("b1"))
    );
    assert_eq!(
        store.get_state_json(project_b, "key2").await.unwrap(),
        Some(serde_json::json!("b2"))
    );
    assert_eq!(
        store.get_state_json(project_b, "key3").await.unwrap(),
        None,
        "Project B should not have key3"
    );
    assert_eq!(
        store.get_state_json(project_b, "key4").await.unwrap(),
        Some(serde_json::json!("b4"))
    );

    println!("MfsStore multi-key cross-project isolation: ✓");
}

#[tokio::test]
async fn test_sop_sql_store_cross_project_isolation() {
    // Test that SopSqlStore isolates SOPs by project_id
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect("sqlite::memory:")
        .await
        .expect("Failed to create pool");

    let sop_store =
        sisyphus_mem::SopSqlStore::new(pool.clone()).await.expect("SopSqlStore init failed");

    let project_a = "project_alpha";
    let project_b = "project_beta";
    let sop_id = "shared_sop_id";

    let config_a = sisyphus_types::SOPGraphConfig {
        nodes: vec![sisyphus_types::SOPNodeConfig {
            name: "node_a".into(),
            kind: sisyphus_types::NodeKind::Wasm {
                tool_id: "tool_a".into(),
                fuel_limit: 100,
            },
        }],
        edges: vec![],
    };

    let config_b = sisyphus_types::SOPGraphConfig {
        nodes: vec![sisyphus_types::SOPNodeConfig {
            name: "node_b".into(),
            kind: sisyphus_types::NodeKind::Wasm {
                tool_id: "tool_b".into(),
                fuel_limit: 200,
            },
        }],
        edges: vec![],
    };

    // Save SOP for Project A
    sop_store
        .save_sop(
            sop_id,
            project_a,
            "SOP Alpha",
            Some("1.0".into()),
            &config_a,
        )
        .await
        .expect("Save SOP A failed");

    // Save SOP for Project B with SAME sop_id
    sop_store
        .save_sop(sop_id, project_b, "SOP Beta", Some("2.0".into()), &config_b)
        .await
        .expect("Save SOP B failed");

    // Load Project A's SOP
    let loaded_a = sop_store.load_sop(sop_id, project_a).await.expect("Load A failed");
    assert!(loaded_a.is_some(), "Project A should find its SOP");
    assert_eq!(
        loaded_a.unwrap().nodes[0].name,
        "node_a",
        "Project A should get its own SOP config"
    );

    // Load Project B's SOP
    let loaded_b = sop_store.load_sop(sop_id, project_b).await.expect("Load B failed");
    assert!(loaded_b.is_some(), "Project B should find its SOP");
    assert_eq!(
        loaded_b.unwrap().nodes[0].name,
        "node_b",
        "Project B should get its own SOP config"
    );

    // Each project lists only its own
    let list_a = sop_store.list_sops(Some(project_a)).await.expect("List A failed");
    let list_b = sop_store.list_sops(Some(project_b)).await.expect("List B failed");
    assert_eq!(list_a, vec![sop_id]);
    assert_eq!(list_b, vec![sop_id]);

    println!("SopSqlStore cross-project isolation: ✓");
}

#[tokio::test]
async fn test_roadmap_sql_store_cross_project_isolation() {
    // Test that RoadmapSqlStore isolates milestones/phases/goals by project_id
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect("sqlite::memory:")
        .await
        .expect("Failed to create pool");

    let roadmap_store = sisyphus_mem::RoadmapSqlStore::new(pool.clone())
        .await
        .expect("RoadmapSqlStore init failed");
    roadmap_store.init_schema().await.expect("Schema init failed");

    let project_a = "project_alpha";
    let project_b = "project_beta";

    // Create projects
    roadmap_store
        .insert_project(&sisyphus_types::Project {
            id: project_a.into(),
            name: "Alpha".into(),
            description: "Project Alpha".into(),
            roadmap: sisyphus_types::Roadmap { milestones: vec![] },
        })
        .await
        .expect("Insert project A failed");

    roadmap_store
        .insert_project(&sisyphus_types::Project {
            id: project_b.into(),
            name: "Beta".into(),
            description: "Project Beta".into(),
            roadmap: sisyphus_types::Roadmap { milestones: vec![] },
        })
        .await
        .expect("Insert project B failed");

    // Create milestone with SAME ID in both projects
    let milestone_id = "ms_1";
    roadmap_store
        .insert_milestone(milestone_id, project_a, "Milestone Alpha", "Pending")
        .await
        .expect("Insert MS A failed");
    roadmap_store
        .insert_milestone(milestone_id, project_b, "Milestone Beta", "InProgress")
        .await
        .expect("Insert MS B failed");

    // Verify isolation: get project A's milestone
    let proj_a = roadmap_store.get_project(project_a).await.expect("Get A failed").unwrap();
    assert_eq!(proj_a.roadmap.milestones.len(), 1);
    assert_eq!(proj_a.roadmap.milestones[0].name, "Milestone Alpha");
    assert_eq!(proj_a.roadmap.milestones[0].id, milestone_id);

    // Verify isolation: get project B's milestone
    let proj_b = roadmap_store.get_project(project_b).await.expect("Get B failed").unwrap();
    assert_eq!(proj_b.roadmap.milestones.len(), 1);
    assert_eq!(proj_b.roadmap.milestones[0].name, "Milestone Beta");
    assert_eq!(proj_b.roadmap.milestones[0].id, milestone_id);

    // Add phases with same ID to different milestones/projects
    let phase_id = "phase_1";
    roadmap_store
        .insert_phase(project_a, phase_id, milestone_id, "Phase A", None)
        .await
        .expect("Insert phase A failed");
    roadmap_store
        .insert_phase(project_b, phase_id, milestone_id, "Phase B", None)
        .await
        .expect("Insert phase B failed");

    // Add goals with same ID to different phases/projects
    let goal_id = "goal_1";
    roadmap_store
        .insert_goal(project_a, goal_id, phase_id, "Goal Alpha", "Pending")
        .await
        .expect("Insert goal A failed");
    roadmap_store
        .insert_goal(project_b, goal_id, phase_id, "Goal Beta", "Completed")
        .await
        .expect("Insert goal B failed");

    // Verify complete isolation by loading full projects
    let proj_a_full =
        roadmap_store.get_project(project_a).await.expect("Get A full failed").unwrap();
    let proj_b_full =
        roadmap_store.get_project(project_b).await.expect("Get B full failed").unwrap();

    assert_eq!(
        proj_a_full.roadmap.milestones[0].phases[0].goals[0].description,
        "Goal Alpha"
    );
    assert_eq!(
        proj_b_full.roadmap.milestones[0].phases[0].goals[0].description,
        "Goal Beta"
    );
    assert_eq!(
        proj_a_full.roadmap.milestones[0].phases[0].goals[0].goal_id,
        goal_id
    );
    assert_eq!(
        proj_b_full.roadmap.milestones[0].phases[0].goals[0].goal_id,
        goal_id
    );

    // Progress calculation is per-project
    let progress_a =
        roadmap_store.calculate_project_progress(project_a).await.expect("Progress A failed");
    let progress_b =
        roadmap_store.calculate_project_progress(project_b).await.expect("Progress B failed");
    assert_eq!(progress_a, 0.0); // Pending
    assert_eq!(progress_b, 1.0); // Completed

    println!("RoadmapSqlStore cross-project isolation: ✓");
}

#[tokio::test]
async fn test_workspace_manager_isolation() {
    // Test WorkspaceManager properly isolates workspaces
    let store = MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
        .await
        .expect("Failed to init MfsStore");
    let state_store: Arc<dyn StateStore> = Arc::new(store);

    let manager = sisyphus_tools_agent::workspace_manager::WorkspaceManager::new(state_store);
    manager.init().await.expect("WorkspaceManager init failed");

    // Create two workspaces
    let ws1 = manager
        .create_workspace(
            "Workspace Alpha".into(),
            "proj_alpha".into(),
            "/home/user/alpha".into(),
        )
        .await
        .expect("Create WS1 failed");

    let ws2 = manager
        .create_workspace(
            "Workspace Beta".into(),
            "proj_beta".into(),
            "/home/user/beta".into(),
        )
        .await
        .expect("Create WS2 failed");

    assert_ne!(ws1.id, ws2.id, "Workspace IDs should be unique");
    assert_eq!(ws1.project_id, "proj_alpha");
    assert_eq!(ws2.project_id, "proj_beta");

    // List workspaces
    let all = manager.list_workspaces().await;
    assert_eq!(all.len(), 2);

    // Switch to workspace 1
    manager.switch_workspace(&ws1.id).await.expect("Switch to WS1 failed");
    let current = manager.get_current().await;
    assert!(current.is_some(), "Should have current workspace");
    let current_ws = current.unwrap();
    assert_eq!(current_ws.id, ws1.id);
    assert_eq!(current_ws.project_id, "proj_alpha");

    // Switch to workspace 2
    manager.switch_workspace(&ws2.id).await.expect("Switch to WS2 failed");
    let current = manager.get_current().await;
    assert!(current.is_some(), "Should have current workspace");
    let current_ws = current.unwrap();
    assert_eq!(current_ws.id, ws2.id);
    assert_eq!(current_ws.project_id, "proj_beta");

    println!("WorkspaceManager isolation: ✓");
}

#[tokio::test]
async fn test_full_kernel_cross_project_isolation() {
    // End-to-end test with full SisyphusKernel
    let kernel_alpha = setup_test_kernel().await;
    let kernel_beta = setup_test_kernel().await;

    // Create SOPs with same ID in different projects
    let sop_id = "shared_sop";

    let mut graph_alpha = sisyphus_core::SOPGraph::new();
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(MockToolExecutor);
    graph_alpha.add_node(Arc::new(sisyphus_types::WasmSOPNode {
        name: "alpha_node".into(),
        tool_id: "tool_alpha".into(),
        fuel_limit: 100,
        executor: tool_executor.clone(),
    }));

    let mut graph_beta = sisyphus_core::SOPGraph::new();
    graph_beta.add_node(Arc::new(sisyphus_types::WasmSOPNode {
        name: "beta_node".into(),
        tool_id: "tool_beta".into(),
        fuel_limit: 100,
        executor: tool_executor,
    }));

    // Save to respective kernels
    kernel_alpha
        .save_sop(sop_id, "proj_alpha", &Arc::new(graph_alpha))
        .await
        .expect("Save alpha failed");
    kernel_beta
        .save_sop(sop_id, "proj_beta", &Arc::new(graph_beta))
        .await
        .expect("Save beta failed");

    // Load back - should get correct project's SOP
    let _loaded_alpha =
        kernel_alpha.load_sop(sop_id, "proj_alpha").await.expect("Load alpha failed");
    let _loaded_beta = kernel_beta.load_sop(sop_id, "proj_beta").await.expect("Load beta failed");

    // Cross-access should return error (SopNotFound) since they don't share project_id
    let cross_alpha = kernel_alpha.load_sop(sop_id, "proj_beta").await;
    let cross_beta = kernel_beta.load_sop(sop_id, "proj_alpha").await;

    // Both should return error since they don't own the other project's SOP
    assert!(
        cross_alpha.is_err(),
        "Cross-project access should return error"
    );
    assert!(
        cross_beta.is_err(),
        "Cross-project access should return error"
    );

    println!("Full kernel cross-project isolation: ✓");
}

#[tokio::test]
async fn test_session_todo_state_isolation() {
    // Test that sessions and todos are isolated per project via MfsStore key prefixing
    let store = MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
        .await
        .expect("Failed to init MfsStore");

    let project_a = "proj_a";
    let project_b = "proj_b";

    // Session state for project A
    let session_a = serde_json::json!({
        "session_id": "sess_a",
        "messages": [{"role": "user", "content": "Hello from A"}]
    });
    store
        .set_state_json(project_a, "session:sess_a", session_a.clone())
        .await
        .expect("Set session A failed");

    // Session state for project B with SAME key
    let session_b = serde_json::json!({
        "session_id": "sess_b",
        "messages": [{"role": "user", "content": "Hello from B"}]
    });
    store
        .set_state_json(project_b, "session:sess_a", session_b.clone())
        .await
        .expect("Set session B failed");

    // Verify isolation
    let loaded_a = store.get_state_json(project_a, "session:sess_a").await.expect("Get A failed");
    let loaded_b = store.get_state_json(project_b, "session:sess_a").await.expect("Get B failed");

    assert_eq!(loaded_a, Some(session_a), "Project A session isolated");
    assert_eq!(loaded_b, Some(session_b), "Project B session isolated");

    // Todo state isolation
    let todos_a = serde_json::json!([{"id": "1", "title": "Task A", "status": "pending"}]);
    let todos_b = serde_json::json!([{"id": "1", "title": "Task B", "status": "done"}]);

    store
        .set_state_json(project_a, "todos:main", todos_a.clone())
        .await
        .expect("Set todos A failed");
    store
        .set_state_json(project_b, "todos:main", todos_b.clone())
        .await
        .expect("Set todos B failed");

    let loaded_todos_a =
        store.get_state_json(project_a, "todos:main").await.expect("Get todos A failed");
    let loaded_todos_b =
        store.get_state_json(project_b, "todos:main").await.expect("Get todos B failed");

    assert_eq!(loaded_todos_a, Some(todos_a), "Project A todos isolated");
    assert_eq!(loaded_todos_b, Some(todos_b), "Project B todos isolated");

    println!("Session/Todo state isolation: ✓");
}

#[tokio::test]
async fn test_experience_bank_cross_project_isolation() {
    // Test that experience bank stores/retrieves per project
    let store = Arc::new(
        MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
            .await
            .expect("Failed to init MfsStore"),
    );
    let vector_store = Arc::new(
        sisyphus_mem::VectorMemoryStore::new(store.pool()).await.expect("Vector store failed"),
    );
    let embedding_client = Arc::new(DummyEmbeddingClient);

    let exp_bank = sisyphus_mem::experience_bank::VectorExperienceBank::new(
        Arc::clone(&store),
        vector_store,
        embedding_client,
    )
    .await
    .expect("Exp bank init failed");

    let project_a = "proj_a";
    let project_b = "proj_b";

    // Record experience for project A
    exp_bank
        .record(sisyphus_types::experience::ExperienceTuple {
            context_text: "How to implement auth in Rust".into(),
            context_hash: "hash_a".into(),
            action_id: "sop:auth_impl".into(),
            project_id: Some(project_a.into()),
            outcome_success: true,
            deviation_score: 0.1,
            cost: 100,
            timestamp: 1000,
            embedding: None,
            visual_embedding: None,
            insight: None,
        })
        .await
        .expect("Record A failed");

    // Record experience for project B (same action_id but different project)
    exp_bank
        .record(sisyphus_types::experience::ExperienceTuple {
            context_text: "How to implement auth in Python".into(),
            context_hash: "hash_b".into(),
            action_id: "sop:auth_impl".into(),
            project_id: Some(project_b.into()),
            outcome_success: false,
            deviation_score: 0.8,
            cost: 200,
            timestamp: 2000,
            embedding: None,
            visual_embedding: None,
            insight: None,
        })
        .await
        .expect("Record B failed");

    // Query for project A - should only get A's experiences
    let query_a = sisyphus_types::experience::ExperienceQuery {
        query_text: "auth".into(),
        context_hash: "hash_a".into(),
        project_id: Some(project_a.into()),
        action_id: None,
        visual_embedding: None,
        limit: 10,
    };
    let results_a = exp_bank.query_similar(query_a).await.expect("Query A failed");
    assert_eq!(
        results_a.len(),
        1,
        "Project A should only see its own experiences"
    );
    assert_eq!(results_a[0].project_id, Some(project_a.into()));
    assert!(results_a[0].outcome_success);

    // Query for project B - should only get B's experiences
    let query_b = sisyphus_types::experience::ExperienceQuery {
        query_text: "auth".into(),
        context_hash: "hash_b".into(),
        project_id: Some(project_b.into()),
        action_id: None,
        visual_embedding: None,
        limit: 10,
    };
    let results_b = exp_bank.query_similar(query_b).await.expect("Query B failed");
    assert_eq!(
        results_b.len(),
        1,
        "Project B should only see its own experiences"
    );
    assert_eq!(results_b[0].project_id, Some(project_b.into()));
    assert!(!results_b[0].outcome_success);

    println!("Experience bank cross-project isolation: ✓");
}

#[tokio::test]
async fn test_state_persistence_across_workspace_switch() {
    // Test that state persists correctly when switching workspaces
    let store = MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
        .await
        .expect("Failed to init MfsStore");
    let state_store: Arc<dyn StateStore> = Arc::new(store);

    let manager =
        sisyphus_tools_agent::workspace_manager::WorkspaceManager::new(state_store.clone());
    manager.init().await.expect("WorkspaceManager init failed");

    // Create workspace
    let ws = manager
        .create_workspace(
            "Test Workspace".into(),
            "test_project".into(),
            "/test/path".into(),
        )
        .await
        .expect("Create workspace failed");

    manager.switch_workspace(&ws.id).await.expect("Switch failed");

    // Set some state in the workspace's project
    let project_id = &ws.project_id;
    state_store
        .set_state_json(
            project_id,
            "session_data",
            serde_json::json!({"key": "value"}),
        )
        .await
        .expect("Set state failed");

    // Create another workspace
    let ws2 = manager
        .create_workspace(
            "Test Workspace 2".into(),
            "test_project_2".into(),
            "/test/path2".into(),
        )
        .await
        .expect("Create workspace 2 failed");

    manager.switch_workspace(&ws2.id).await.expect("Switch to ws2 failed");

    // Set different state for project 2
    state_store
        .set_state_json(
            &ws2.project_id,
            "session_data",
            serde_json::json!({"key": "different"}),
        )
        .await
        .expect("Set state 2 failed");

    // Switch back to first workspace and verify state persisted
    manager.switch_workspace(&ws.id).await.expect("Switch back failed");
    let state1 =
        state_store.get_state_json(project_id, "session_data").await.expect("Get state 1 failed");
    assert_eq!(state1, Some(serde_json::json!({"key": "value"})));

    // Switch to second workspace and verify its state
    manager.switch_workspace(&ws2.id).await.expect("Switch to ws2 again failed");
    let state2 = state_store
        .get_state_json(&ws2.project_id, "session_data")
        .await
        .expect("Get state 2 failed");
    assert_eq!(state2, Some(serde_json::json!({"key": "different"})));

    println!("State persistence across workspace switches: ✓");
}

#[tokio::test]
async fn test_concurrent_projects_no_interference() {
    // Test that concurrent operations on different projects don't interfere
    // (SOP save/load level; execute_goal requires a real LLM)
    let kernel = Arc::new(setup_test_kernel().await);

    let mut handles = vec![];

    for i in 0..5 {
        let kernel = Arc::clone(&kernel);
        let project_id = format!("project_{}", i);
        let sop_id = format!("sop_{}", i);

        let handle = tokio::spawn(async move {
            let mut graph = sisyphus_core::SOPGraph::new();
            let tool_executor: Arc<dyn ToolExecutor> = Arc::new(MockToolExecutor);
            graph.add_node(Arc::new(sisyphus_types::WasmSOPNode {
                name: "node".into(),
                tool_id: format!("tool_{}", i),
                fuel_limit: 100,
                executor: tool_executor,
            }));

            kernel.save_sop(&sop_id, &project_id, &Arc::new(graph)).await.expect("Save SOP failed");

            // Verify we can load it back
            let _loaded = kernel.load_sop(&sop_id, &project_id).await.expect("Load SOP failed");

            project_id
        });
        handles.push(handle);
    }

    // Wait for all to complete
    let mut results = Vec::new();
    for handle in handles {
        let proj = handle.await.expect("Task panicked");
        results.push(proj);
    }

    assert_eq!(results.len(), 5);

    println!("Concurrent multi-project SOP save/load: ✓");
}
