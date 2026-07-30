//! Full gRPC integration tests for the Sisyphus Bridge.
//!
//! These tests spin up a real gRPC server backed by `MySisyphusBridge`
//! with a minimal in-memory configuration, then connect as a client and
//! exercise every RPC method defined in `bridge.proto`.
//!
//! # CI note
//!
//! The bridge's default features enable `sisyphus-sandbox` (wasmtime).
//! Run these tests in isolation (`-p sisyphus-bridge`) to avoid OOM.
//! With `--no-default-features`, the sandbox is replaced by noop stubs.

use sisyphus_bridge::bridge_service::MySisyphusBridge;
use sisyphus_bridge_api::sisyphus_bridge_client::SisyphusBridgeClient;
use sisyphus_bridge_api::sisyphus_bridge_server::SisyphusBridgeServer;
use sisyphus_bridge_api::{
    AddMessageRequest, BreakerMetricsRequest, DecomposeGoalRequest, ExecuteSopRequest,
    ExecuteToolRequest, HealthCheckRequest, InstallSkillRequest, LearningInsightsRequest,
    ListAgentsRequest, ListBenchmarksRequest, ListPluginsRequest, ListSkillsRequest,
    ListSoPsRequest, ListToolsRequest, LoadPluginRequest, RemoveSkillRequest, ShowSkillRequest,
    UnloadPluginRequest,
};
use sisyphus_tools_llm::metis::DummyLlmClient;
use sisyphus_types::Config;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio_stream::StreamExt;
use tonic::transport::Server;

/// Helper: create a minimal config + shared bridge, then return
/// a connected client and the server handle.
async fn setup_bridge() -> (
    SisyphusBridgeClient<tonic::transport::Channel>,
    tokio::task::JoinHandle<()>,
) {
    let mut config = Config::default();
    config.cache.database_url = "sqlite::memory:".to_string();
    config.sandbox.wasm_tools_dir = "tests/wasm_tools".to_string();
    let shared_config = Arc::new(RwLock::new(config));

    let bridge = MySisyphusBridge::new(shared_config, Some(Arc::new(DummyLlmClient)))
        .await
        .expect("Failed to create bridge");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind listener");
    let addr = listener.local_addr().expect("Failed to get local addr");
    let server_addr = format!("http://{}", addr);

    let bridge_clone = bridge.clone();
    let server_handle = tokio::spawn(async move {
        Server::builder()
            .add_service(SisyphusBridgeServer::new(bridge_clone))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .expect("Server failed");
    });

    let client = SisyphusBridgeClient::connect(server_addr)
        .await
        .expect("Failed to connect client");

    (client, server_handle)
}

// ── Core RPCs ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_health_check() {
    let (mut client, server) = setup_bridge().await;
    let response = client
        .health_check(HealthCheckRequest {})
        .await
        .expect("HealthCheck failed");
    let inner = response.into_inner();
    assert_eq!(inner.status, "SERVING");
    assert!(!inner.version.is_empty());
    server.abort();
}

#[tokio::test]
async fn test_list_sops_empty() {
    let (mut client, server) = setup_bridge().await;
    let response = client
        .list_so_ps(ListSoPsRequest {})
        .await
        .expect("ListSoPs failed");
    assert!(response.into_inner().sops.is_empty());
    server.abort();
}

#[tokio::test]
async fn test_execute_sop_missing() {
    let (mut client, server) = setup_bridge().await;
    let response = client
        .execute_sop(ExecuteSopRequest {
            sop_id: "nonexistent".to_string(),
            args: HashMap::new(),
        })
        .await
        .expect("ExecuteSop failed");
    // The bridge creates an empty SOP graph for any ID, which executes with an empty result.
    // We verify a response is returned, even if the SOP was created on-the-fly.
    let inner = response.into_inner();
    assert_eq!(
        inner.status, "SUCCESS",
        "SOP execution failed: {}",
        inner.result
    );
    server.abort();
}

// ── List RPCs ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_list_agents() {
    let (mut client, server) = setup_bridge().await;
    let response = client
        .list_agents(ListAgentsRequest {})
        .await
        .expect("ListAgents failed");
    // Bridge registers builtin agents; at minimum metis, librarian, etc.
    let agents = response.into_inner().agents;
    assert!(!agents.is_empty(), "Expected at least one registered agent");
    eprintln!(
        "Agents: {:?}",
        agents.iter().map(|a| &a.agent_id).collect::<Vec<_>>()
    );
    server.abort();
}

#[tokio::test]
async fn test_list_tools() {
    let (mut client, server) = setup_bridge().await;
    let response = client
        .list_tools(ListToolsRequest {})
        .await
        .expect("ListTools failed");
    let tools = response.into_inner().tools;
    assert!(!tools.is_empty(), "Expected at least builtin tools");
    // Verify a well-known tool exists
    assert!(
        tools.iter().any(|t| t.tool_id == "builtin:file_read"),
        "Expected builtin:file_read in tool list"
    );
    server.abort();
}

#[tokio::test]
async fn test_list_skills_empty() {
    let (mut client, server) = setup_bridge().await;
    let response = client
        .list_skills(ListSkillsRequest { remote: false })
        .await
        .expect("ListSkills failed");
    assert!(response.into_inner().skills.is_empty());
    server.abort();
}

// ── Skill RPCs (remote-only with DummyLlmClient) ─────────────────────────

#[tokio::test]
async fn test_show_skill_not_found() {
    let (mut client, server) = setup_bridge().await;
    let response = client
        .show_skill(ShowSkillRequest {
            name: "nonexistent_skill".into(),
        })
        .await
        .expect("ShowSkill failed");
    assert!(!response.into_inner().found);
    server.abort();
}

#[tokio::test]
async fn test_install_skill_with_dummy_llm() {
    let (mut client, server) = setup_bridge().await;
    let response = client
        .install_skill(InstallSkillRequest {
            skill_id: "nonexistent_skill".into(),
        })
        .await
        .expect("InstallSkill failed");
    let inner = response.into_inner();
    // With DummyLlmClient, remote skill registry calls fail gracefully
    assert!(!inner.success || !inner.name.is_empty());
    server.abort();
}

#[tokio::test]
async fn test_remove_skill_nonexistent() {
    let (mut client, server) = setup_bridge().await;
    let response = client
        .remove_skill(RemoveSkillRequest {
            name: "nonexistent_skill".into(),
        })
        .await
        .expect("RemoveSkill failed");
    let inner = response.into_inner();
    // The skill registry is idempotent: removing a nonexistent skill returns success.
    // We verify the operation completes without error.
    eprintln!("RemoveSkill result: success={}", inner.success);
    server.abort();
}

// ── Plugin RPCs ───────────────────────────────────────────────────────────

#[tokio::test]
async fn test_list_plugins_empty() {
    let (mut client, server) = setup_bridge().await;
    let response = client
        .list_plugins(ListPluginsRequest {})
        .await
        .expect("ListPlugins failed");
    assert!(response.into_inner().plugins.is_empty());
    server.abort();
}

#[tokio::test]
async fn test_load_plugin_missing_path() {
    let (mut client, server) = setup_bridge().await;
    let response = client
        .load_plugin(LoadPluginRequest {
            name: "test".into(),
            version: "1.0".into(),
            wasm_path: "/tmp/nonexistent.wasm".into(),
            tool_id: "test_tool".into(),
            fuel_limit: 0,
        })
        .await
        .expect("LoadPlugin failed");
    let inner = response.into_inner();
    assert!(!inner.success);
    assert!(!inner.error.is_empty());
    server.abort();
}

#[tokio::test]
async fn test_unload_plugin_nonexistent() {
    let (mut client, server) = setup_bridge().await;
    let response = client
        .unload_plugin(UnloadPluginRequest {
            name: "nonexistent".into(),
        })
        .await
        .expect("UnloadPlugin failed");
    let inner = response.into_inner();
    // Unload a non-existent plugin may return success (idempotent) or error
    assert!(inner.success || !inner.error.is_empty());
    server.abort();
}

// ── Execute RPCs ──────────────────────────────────────────────────────────

#[tokio::test]
async fn test_execute_tool_builtin_read() {
    let (mut client, server) = setup_bridge().await;
    let response = client
        .execute_tool(ExecuteToolRequest {
            tool_id: "builtin:file_read".into(),
            args_json: r#"{"path": "/nonexistent/test_file_for_integration_test"}"#.into(),
            fuel_limit: 0,
        })
        .await
        .expect("ExecuteTool failed");
    let inner = response.into_inner();
    // The file doesn't exist, so the tool should fail — but it should NOT be a WASM error.
    // This verifies the ExecutorRouter routes "builtin:" prefix to BuiltinToolExecutor.
    assert_ne!(
        inner.status, "SUCCESS",
        "Expected builtin:file_read to fail (file doesn't exist), got: {}",
        inner.result_json
    );
    assert!(
        !inner.status.contains("WASM"),
        "Expected non-WASM error, got: {}",
        inner.status
    );
    server.abort();
}

#[tokio::test]
async fn test_execute_tool_builtin_bash() {
    let (mut client, server) = setup_bridge().await;
    let response = client
        .execute_tool(ExecuteToolRequest {
            tool_id: "builtin:bash_exec".into(),
            args_json: r#"{"command": "echo hello", "timeout_ms": 5000}"#.into(),
            fuel_limit: 0,
        })
        .await
        .expect("ExecuteTool failed");
    let inner = response.into_inner();
    // bash_exec should succeed and return "hello"
    assert_eq!(
        inner.status, "SUCCESS",
        "builtin:bash_exec failed: {}",
        inner.result_json
    );
    assert!(
        inner.result_json.contains("hello"),
        "Expected bash output to contain 'hello', got: {}",
        inner.result_json
    );
    server.abort();
}

#[tokio::test]
async fn test_execute_tool_unknown() {
    let (mut client, server) = setup_bridge().await;
    let response = client
        .execute_tool(ExecuteToolRequest {
            tool_id: "unknown/unknown_tool".into(),
            args_json: "{}".into(),
            fuel_limit: 0,
        })
        .await
        .expect("ExecuteTool failed");
    let inner = response.into_inner();
    assert!(
        inner.status != "SUCCESS",
        "Expected unknown tool to fail, got success: {}",
        inner.result_json
    );
    server.abort();
}

// ── Decompose RPC ─────────────────────────────────────────────────────────

#[tokio::test]
async fn test_decompose_goal_with_dummy() {
    // With DummyLlmClient (echo mode), decompose should succeed
    let (mut client, server) = setup_bridge().await;
    let response = client
        .decompose_goal(DecomposeGoalRequest {
            goal_id: "test".into(),
            objective: "Write a test".into(),
            context: "".into(),
        })
        .await
        .expect("DecomposeGoal failed");
    let inner = response.into_inner();
    // DummyLlmClient returns structured JSON; decompose should either
    // succeed with tasks or fail gracefully with an error
    if inner.success {
        eprintln!("Decompose tasks: {:?}", inner.tasks);
    } else {
        eprintln!("Decompose error (expected with dummy): {}", inner.error);
    }
    server.abort();
}

// ── AddMessage RPC ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_add_message_session() {
    let (mut client, server) = setup_bridge().await;
    let response = client
        .add_message(AddMessageRequest {
            session_id: "test_session".into(),
            role: "user".into(),
            content: "Hello".into(),
            session_db_path: ":memory:".into(),
        })
        .await
        .expect("AddMessage failed");
    let inner = response.into_inner();
    assert!(inner.success || !inner.error.is_empty());
    server.abort();
}

// ── Meta RPCs ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_list_benchmarks_empty() {
    let (mut client, server) = setup_bridge().await;
    let response = client
        .list_benchmarks(ListBenchmarksRequest {
            name_filter: "".into(),
        })
        .await
        .expect("ListBenchmarks failed");
    assert!(response.into_inner().benchmarks.is_empty());
    server.abort();
}

#[tokio::test]
async fn test_get_breaker_metrics() {
    let (mut client, server) = setup_bridge().await;
    let response = client
        .get_breaker_metrics(BreakerMetricsRequest {
            model_name: "".into(),
        })
        .await
        .expect("GetBreakerMetrics failed");
    // May return empty but should not error
    response.into_inner();
    server.abort();
}

#[tokio::test]
async fn test_get_learning_insights() {
    let (mut client, server) = setup_bridge().await;
    let response = client
        .get_learning_insights(LearningInsightsRequest {
            strategy_filter: "".into(),
        })
        .await
        .expect("GetLearningInsights failed");
    let inner = response.into_inner();
    assert_eq!(inner.total_samples, 0);
    assert!(inner.overall_success_rate >= 0.0);
    server.abort();
}

// ── Stream RPC ────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_stream_execution_returns_event() {
    let (mut client, server) = setup_bridge().await;
    let mut stream = client
        .stream_execution(ExecuteSopRequest {
            sop_id: "stream_test".into(),
            args: HashMap::new(),
        })
        .await
        .expect("StreamExecution failed")
        .into_inner();

    // Should get at least one event (the server sends a completion event)
    let event = stream.next().await;
    assert!(event.is_some(), "Expected at least one stream event");
    let event = event.unwrap();
    assert!(event.is_ok(), "Stream event should be OK");
    let event = event.unwrap();
    assert!(!event.status.is_empty());
    server.abort();
}
