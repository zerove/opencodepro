use serde_json::json;
use sisyphus_tools_core::{
    feedback::FeedbackStore,
    hook::{hooks_from_skill, HookDef, HookPhase, HookRegistry},
};
use sisyphus_tools_executor::executor::BuiltinToolExecutor;
use sisyphus_tools_llm::LlmClient;
use sisyphus_tools_session::skill::SkillLoader;
use sisyphus_types::ToolExecutor;
use std::sync::Arc;
use tempfile::tempdir;

struct DummyLlmClient;

#[async_trait::async_trait]
impl LlmClient for DummyLlmClient {
    async fn generate(&self, prompt: &str) -> Result<String, sisyphus_types::TypesError> {
        self.chat("", prompt, false).await
    }

    async fn chat(
        &self,
        _system: &str,
        user: &str,
        _json_mode: bool,
    ) -> Result<String, sisyphus_types::TypesError> {
        Ok(format!(
            r#"{{"ambiguities": [], "implicit_requirements": [], "summary": "Dummy response for: {}", "actionable": true}}"#,
            user
        ))
    }

    fn get_params_for(&self, _agent_id: &str) -> (String, String, String) {
        (
            "dummy-key".into(),
            "http://dummy:8080".into(),
            "dummy-model".into(),
        )
    }

    fn breaker(&self) -> sisyphus_tools_llm::circuit_breaker::CircuitBreaker {
        sisyphus_tools_llm::circuit_breaker::CircuitBreaker::default()
    }
}

async fn setup_executor(registry: HookRegistry) -> BuiltinToolExecutor {
    let dir = tempdir().unwrap();
    let _feedback_store = Arc::new(FeedbackStore::new(dir.path()));

    BuiltinToolExecutor {
        registry,
        llm_client: Arc::new(DummyLlmClient),
        feedback_store: Arc::new(sisyphus_tools_core::feedback::FeedbackStore::default_dir()),
        bridge_client: None,
    }
}

#[tokio::test]
async fn test_hook_allow() {
    let mut registry = HookRegistry::new();
    registry.register(HookDef {
        name: "allow-hook".into(),
        phase: HookPhase::Before,
        tool_filter: Some("file_read".into()),
        effect_type: "allow".into(),
        effect_payload: "".into(),
        description: "Allow file read".into(),
        source_skill: None,
    });

    let executor = setup_executor(registry).await;
    let args = json!({"path": "Cargo.toml"}).to_string().into_bytes();

    let result = executor.execute("file_read", args, 0).await;
    assert!(result.is_ok(), "Tool should be allowed to execute");
}

#[tokio::test]
async fn test_hook_deny() {
    let mut registry = HookRegistry::new();
    let deny_reason = "Security policy: no file reads allowed";
    registry.register(HookDef {
        name: "deny-hook".into(),
        phase: HookPhase::Before,
        tool_filter: Some("file_read".into()),
        effect_type: "deny".into(),
        effect_payload: deny_reason.into(),
        description: "Deny file read".into(),
        source_skill: None,
    });

    let executor = setup_executor(registry).await;
    let args = json!({"path": "Cargo.toml"}).to_string().into_bytes();

    let result = executor.execute("file_read", args, 0).await;
    assert!(result.is_err(), "Tool should be blocked by hook");
    assert!(
        result.unwrap_err().to_string().contains(deny_reason),
        "Error message should contain the deny reason"
    );
}

#[tokio::test]
async fn test_hook_inject() {
    let mut registry = HookRegistry::new();
    let instructions = "Always check for sensitive data before reading";
    registry.register(HookDef {
        name: "inject-hook".into(),
        phase: HookPhase::Before,
        tool_filter: Some("file_read".into()),
        effect_type: "inject".into(),
        effect_payload: instructions.into(),
        description: "Inject instructions".into(),
        source_skill: None,
    });

    let executor = setup_executor(registry).await;
    let args = json!({"path": "Cargo.toml"}).to_string().into_bytes();

    let result = executor.execute("file_read", args, 0).await;
    assert!(
        result.is_ok(),
        "Tool should still execute when instructions are injected"
    );
}

#[tokio::test]
async fn test_skill_integration() {
    let dir = tempdir().unwrap();
    let skill_path = dir.path().join("test_skill.md");
    let skill_content = r#"---
name: test-skill
description: A skill with a hook
triggers:
  - tools: ["file_read"]
    description: block-read-hook
    hook_phase: "before"
    hook_effect: "deny"
    hook_message: "Blocked by skill integration"
---
Instructions body
"#;
    std::fs::write(&skill_path, skill_content).unwrap();

    let loader = SkillLoader::new().with_dir(dir.path().to_path_buf());
    let skills = loader.load_all();
    let skill = skills.iter().find(|s| s.name == "test-skill").expect("Skill should be loaded");

    // Convert skill triggers to hooks
    let hooks = hooks_from_skill(&skill.name, &skill.triggers, &skill.instructions);

    let mut registry = HookRegistry::new();
    registry.register_all(hooks);

    let executor = setup_executor(registry).await;
    let args = json!({"path": "Cargo.toml"}).to_string().into_bytes();

    let result = executor.execute("file_read", args, 0).await;
    assert!(
        result.is_err(),
        "Tool should be blocked by skill-defined hook"
    );
    assert!(result.unwrap_err().to_string().contains("Blocked by skill integration"));
}
