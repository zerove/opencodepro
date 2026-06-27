use async_trait::async_trait;
use serde_json::json;
use std::sync::{Arc, RwLock};

use sisyphus_tools::{
    agent_registry::{AgentInfo, AgentRegistry},
    llm::DynamicLlmClient,
    metis::LlmClient,
    team::{TaskNode, TeamGoal, TeamOrchestrator},
};
use sisyphus_types::{AgentLlmOverride, Config, ToolExecutor};

// ── Mocks ──────────────────────────────────────────────────────────────────

struct JsonLlmClient(String);

#[async_trait]
impl LlmClient for JsonLlmClient {
    async fn chat(&self, _system: &str, _user: &str, _json_mode: bool) -> Result<String, String> {
        Ok(self.0.clone())
    }
}

struct MockToolExecutor;

#[async_trait]
impl ToolExecutor for MockToolExecutor {
    async fn execute(
        &self,
        tool_id: &str,
        _args: Vec<u8>,
        _fuel_limit: u64,
    ) -> Result<Vec<u8>, String> {
        // For the purpose of this E2E test, we accept any tool that starts with "custom/"
        if tool_id.starts_with("custom/") {
            Ok(json!({"result": "success"}).to_string().into_bytes())
        } else {
            Err(format!("Unknown tool: {}", tool_id))
        }
    }
}

fn make_executor() -> Arc<dyn ToolExecutor> {
    Arc::new(MockToolExecutor)
}

#[tokio::test]
async fn test_agent_model_routing_pipeline_e2e() {
    // Clear env var to ensure config values are used
    std::env::remove_var("GOOGLE_GENERATIVE_AI_API_KEY");

    // 1. Setup Config with per-agent overrides
    let mut config = Config::default();
    config.llm.api_key = "base-key".into();
    config.llm.endpoint = "http://base-endpoint".into();
    config.llm.model = "base-model".into();

    config.llm.agents.insert(
        "analyst".into(),
        AgentLlmOverride {
            model: "claude-sonnet-4".into(),
            endpoint: Some("https://custom.api.com/v1".into()),
            api_key: Some("custom-key".into()),
        },
    );

    let shared_config = Arc::new(RwLock::new(config));

    // 2. Create DynamicLlmClient
    let llm_client = Arc::new(DynamicLlmClient::new(shared_config));

    // 3. Create AgentRegistry and register agents
    let registry = Arc::new(AgentRegistry::new(llm_client.clone()));

    // Register custom agent with override
    registry.register(
        "analyst",
        AgentInfo {
            display_name: "Analyst".into(),
            description: "Custom analyst agent".into(),
            tool_prefixes: vec!["custom/".into()],
            max_concurrency: 2,
        },
    );

    // Register agent with NO override (should fallback)
    registry.register(
        "default-agent",
        AgentInfo {
            display_name: "Default Agent".into(),
            description: "Agent using base config".into(),
            tool_prefixes: vec!["default/".into()],
            max_concurrency: 2,
        },
    );

    // 4. Verify get_llm_params routing
    let (api_key, endpoint, model) = registry.get_llm_params("analyst");
    assert_eq!(api_key, "custom-key");
    assert_eq!(endpoint, "https://custom.api.com/v1");
    assert_eq!(model, "claude-sonnet-4");

    let (api_key_def, endpoint_def, model_def) = registry.get_llm_params("default-agent");
    assert_eq!(api_key_def, "base-key");
    assert_eq!(endpoint_def, "http://base-endpoint");
    assert_eq!(model_def, "base-model");

    let (api_key_none, endpoint_none, model_none) = registry.get_llm_params("nonexistent");
    assert_eq!(api_key_none, "base-key");
    assert_eq!(endpoint_none, "http://base-endpoint");
    assert_eq!(model_none, "base-model");

    // 5. Create TeamOrchestrator
    let executor = make_executor();

    // Mock decomposition response for the planner
    let decomposition_json = r#"{
        "tasks": [
            {
                "id": "task-1",
                "agent": "analyst",
                "tool_id": "custom/tool",
                "args": {"query": "test"},
                "depends_on": []
            }
        ]
    }"#;
    let planner_llm = Arc::new(JsonLlmClient(decomposition_json.to_string()));

    let orchestrator = TeamOrchestrator::new(registry.clone(), executor, Some(planner_llm));

    // 6. Run a TeamGoal and verify the pipeline
    let goal = TeamGoal {
        id: "e2e-goal".into(),
        objective: "Verify routing pipeline".into(),
        context: None,
    };

    let report = orchestrator
        .run_goal(goal)
        .await
        .expect("Goal execution should not fail");

    if !report.success {
        println!("Report failed. Task results: {:?}", report.task_results);
    }
    assert!(
        report.success,
        "Pipeline should succeed for registered agent and tool"
    );
    assert_eq!(report.task_results.len(), 1);
    assert_eq!(report.task_results[0].agent_id, "analyst");
    assert!(report.task_results[0].success);

    // 7. Test that unknown agent returns error through the team orchestrator
    let tasks_with_unknown = vec![TaskNode {
        id: "bad-task".into(),
        agent_id: "unknown-agent".into(),
        tool_id: "custom/tool".into(),
        args: json!({}),
        depends_on: vec![],
    }];

    let report_unknown = orchestrator
        .run(
            TeamGoal {
                id: "unknown-agent-goal".into(),
                objective: "Test unknown agent".into(),
                context: None,
            },
            tasks_with_unknown,
        )
        .await;

    assert!(!report_unknown.success, "Should fail for unknown agent");
    let failed_task = report_unknown
        .task_results
        .iter()
        .find(|r| !r.success)
        .expect("Should have a failed task");
    assert!(failed_task
        .error
        .as_ref()
        .unwrap()
        .contains("not registered"));
}
