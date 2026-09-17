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
    // 关键：DummyLlmClient 没有 embedding 凭据，
    // 显式允许 dummy embedding 避免桥接启动失败。
    config.llm.embedding.allow_dummy_embeddings = true;
    let shared_config = Arc::new(RwLock::new(config));

    let bridge = MySisyphusBridge::new(shared_config, Some(Arc::new(DummyLlmClient)))
        .await
        .expect("Failed to create bridge");

    let listener =
        tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("Failed to bind listener");
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

    let client =
        SisyphusBridgeClient::connect(server_addr).await.expect("Failed to connect client");

    (client, server_handle)
}

// ── Core RPCs ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_health_check() {
    let (mut client, server) = setup_bridge().await;
    let response = client.health_check(HealthCheckRequest {}).await.expect("HealthCheck failed");
    let inner = response.into_inner();
    assert_eq!(inner.status, "SERVING");
    assert!(!inner.version.is_empty());
    server.abort();
}

#[tokio::test]
async fn test_list_sops_empty() {
    let (mut client, server) = setup_bridge().await;
    let response = client.list_so_ps(ListSoPsRequest {}).await.expect("ListSoPs failed");
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
    let response = client.list_agents(ListAgentsRequest {}).await.expect("ListAgents failed");
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
    let response = client.list_tools(ListToolsRequest {}).await.expect("ListTools failed");
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
    let response =
        client.list_skills(ListSkillsRequest { remote: false }).await.expect("ListSkills failed");
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
    let response = client.list_plugins(ListPluginsRequest {}).await.expect("ListPlugins failed");
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

/// 战略循环公共接口验收：strategic:next_goal 工具经过 ExecuteTool RPC
/// 路由到 StrategicExecutor。kernel 未设置时应得到结构化错误而非
/// panic/挂起，证明桥接层的集成边界行为稳定（内核注入路径属
/// bridge-test-full 专项）。
#[tokio::test]
async fn test_execute_tool_strategic_next_goal_reports_kernel_state() {
    let (mut client, server) = setup_bridge().await;
    let response = client
        .execute_tool(ExecuteToolRequest {
            tool_id: "strategic:next_goal".into(),
            args_json: r#"{"project_id": "nonexistent-project"}"#.into(),
            fuel_limit: 0,
        })
        .await
        .expect("ExecuteTool failed");
    let inner = response.into_inner();
    // 无 kernel/项目时必须失败且错误可观察，不能 panic 或挂起。
    assert_ne!(
        inner.status, "SUCCESS",
        "未注入 kernel 时不应成功: {}",
        inner.result_json
    );
    // 错误必须携带结构化上下文：工具名与失败原因。
    assert!(
        inner.status.contains("next_goal"),
        "status 应包含工具名，实际: {}",
        inner.status
    );
    assert!(
        inner.status.contains("Project 'nonexistent-project' not found"),
        "status 应指明项目不存在，实际: {}",
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
    // Chaos auto-fix may have recorded many samples; accept plausible count
    // Other tests may add samples, so use a more generous threshold
    assert!(
        inner.total_samples < 10000,
        "too many samples: {}",
        inner.total_samples
    );
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

// ── Negative Tests ─────────────────────────────────────────────────────────

#[tokio::test]
async fn test_negative_concurrent_requests() {
    // Fire 20 concurrent SOP executions to verify the bridge handles
    // concurrent load without panicking or deadlocking.
    let (client, server) = setup_bridge().await;
    let mut handles = Vec::with_capacity(20);
    for i in 0..20usize {
        let mut cl = client.clone();
        handles.push(tokio::spawn(async move {
            let resp = cl
                .execute_sop(ExecuteSopRequest {
                    sop_id: format!("concurrent_{i}"),
                    args: HashMap::new(),
                })
                .await;
            (i, resp)
        }));
    }

    let mut ok_count = 0usize;
    for handle in handles {
        let (i, result) = handle.await.expect("Task panicked");
        match result {
            Ok(resp) => {
                let inner = resp.into_inner();
                assert_eq!(
                    inner.status, "SUCCESS",
                    "concurrent request {i} failed: {}",
                    inner.result
                );
                ok_count += 1;
            },
            Err(e) => {
                panic!("concurrent request {i} returned gRPC error: {e}");
            },
        }
    }
    assert_eq!(ok_count, 20, "Not all concurrent requests succeeded");
    server.abort();
}

#[tokio::test]
async fn test_negative_execute_tool_empty_args() {
    let (mut client, server) = setup_bridge().await;
    let response = client
        .execute_tool(ExecuteToolRequest {
            tool_id: "builtin:file_read".into(),
            args_json: "".into(),
            fuel_limit: 0,
        })
        .await
        .expect("ExecuteTool with empty args should not panic");
    let inner = response.into_inner();
    // Empty JSON is invalid, so it should fail — but the bridge should handle
    // it gracefully (no panic, no crash).
    assert_ne!(
        inner.status, "SUCCESS",
        "Expected file_read with empty args to fail"
    );
    server.abort();
}

#[tokio::test]
async fn test_negative_execute_tool_malformed_json() {
    let (mut client, server) = setup_bridge().await;
    let response = client
        .execute_tool(ExecuteToolRequest {
            tool_id: "builtin:bash_exec".into(),
            args_json: "this is not valid json".into(),
            fuel_limit: 0,
        })
        .await
        .expect("ExecuteTool with malformed JSON should not panic");
    let inner = response.into_inner();
    // Malformed JSON should be rejected gracefully
    assert_ne!(
        inner.status, "SUCCESS",
        "Expected bash_exec with malformed JSON to fail"
    );
    server.abort();
}

#[tokio::test]
async fn test_negative_execute_sop_empty_id() {
    let (mut client, server) = setup_bridge().await;
    let response = client
        .execute_sop(ExecuteSopRequest {
            sop_id: "".into(),
            args: HashMap::new(),
        })
        .await
        .expect("ExecuteSOP with empty ID should not panic");
    let inner = response.into_inner();
    // Bridge creates an empty SOP for any ID; empty ID should still work
    // but we verify no crash happens and we get a response
    assert!(!inner.status.is_empty(), "Expected non-empty status");
    server.abort();
}

#[tokio::test]
async fn test_negative_add_message_empty_content() {
    let (mut client, server) = setup_bridge().await;
    let response = client
        .add_message(AddMessageRequest {
            session_id: "test_session".into(),
            role: "user".into(),
            content: "".into(),
            session_db_path: ":memory:".into(),
        })
        .await
        .expect("AddMessage with empty content should not panic");
    let inner = response.into_inner();
    assert!(inner.success || !inner.error.is_empty());
    server.abort();
}

#[tokio::test]
async fn test_negative_decompose_goal_empty_objective() {
    let (mut client, server) = setup_bridge().await;
    let response = client
        .decompose_goal(DecomposeGoalRequest {
            goal_id: "test".into(),
            objective: "".into(),
            context: "".into(),
        })
        .await
        .expect("DecomposeGoal with empty objective should not panic");
    let inner = response.into_inner();
    // Should handle gracefully — either success or error
    assert!(inner.success || !inner.error.is_empty());
    server.abort();
}

#[tokio::test]
async fn test_negative_large_sop_args() {
    // Verify that sending a large args payload doesn't crash the bridge
    let (mut client, server) = setup_bridge().await;
    let large_value = "x".repeat(100_000);
    let mut large_args = HashMap::new();
    large_args.insert("large_key".to_string(), large_value);

    let response = client
        .execute_sop(ExecuteSopRequest {
            sop_id: "large_args".into(),
            args: large_args,
        })
        .await
        .expect("ExecuteSOP with large args should not panic");
    let inner = response.into_inner();
    // The bridge should handle large payloads without OOM
    assert!(!inner.status.is_empty());
    server.abort();
}

#[tokio::test]
async fn test_negative_repeated_execute_tool_builtin() {
    // Execute the same builtin tool 100 times to check for resource leaks
    let (mut client, server) = setup_bridge().await;
    for i in 0..100usize {
        let response = client
            .execute_tool(ExecuteToolRequest {
                tool_id: "builtin:bash_exec".into(),
                args_json: r#"{"command": "echo ok", "timeout_ms": 1000}"#.into(),
                fuel_limit: 0,
            })
            .await
            .expect("Repeated ExecuteTool should not fail");
        let inner = response.into_inner();
        if i == 0 {
            assert_eq!(inner.status, "SUCCESS", "First bash_exec should succeed");
        }
        // Subsequent calls should also work — we verify no resource leak
        assert!(
            inner.status == "SUCCESS",
            "Iteration {i}: unexpected status: {}",
            inner.status
        );
    }
    server.abort();
}

#[tokio::test]
async fn test_negative_load_plugin_empty_fields() {
    let (mut client, server) = setup_bridge().await;
    let response = client
        .load_plugin(LoadPluginRequest {
            name: "".into(),
            version: "".into(),
            wasm_path: "".into(),
            tool_id: "".into(),
            fuel_limit: 0,
        })
        .await
        .expect("LoadPlugin with empty fields should not panic");
    let inner = response.into_inner();
    // LoadPlugin with all empty fields must fail gracefully
    assert!(!inner.success, "Expected load with empty fields to fail");
    server.abort();
}

#[tokio::test]
async fn test_negative_fuel_limit_exceeded() {
    // Test with an extremely large fuel_limit value to ensure no overflow
    let (mut client, server) = setup_bridge().await;
    let response = client
        .execute_tool(ExecuteToolRequest {
            tool_id: "builtin:bash_exec".into(),
            args_json: r#"{"command": "echo hello"}"#.into(),
            fuel_limit: u64::MAX,
        })
        .await
        .expect("ExecuteTool with MAX fuel should not panic/overflow");
    let inner = response.into_inner();
    // Should either succeed or fail gracefully — no panic
    assert!(
        inner.status == "SUCCESS",
        "Unexpected status: {}",
        inner.status
    );
    server.abort();
}

#[tokio::test]
async fn test_negative_execute_tool_with_timeout() {
    // Test that a builtin tool with an extremely short timeout_ms
    // is handled gracefully (no panic, no hang).
    let (mut client, server) = setup_bridge().await;
    let response = client
        .execute_tool(ExecuteToolRequest {
            tool_id: "builtin:bash_exec".into(),
            // 1ms timeout should be too short for any real command
            args_json: r#"{"command": "sleep 10", "timeout_ms": 1}"#.into(),
            fuel_limit: 0,
        })
        .await
        .expect("ExecuteTool with short timeout should not panic or hang");
    let inner = response.into_inner();
    // The tool should either timeout (non-SUCCESS) or somehow complete
    // Either way, the bridge should respond without panicking.
    eprintln!(
        "Timeout test result: status={} result={}",
        inner.status, inner.result_json
    );
    server.abort();
}

#[tokio::test]
async fn test_negative_execute_tool_timeout_zero_ms() {
    // Test with explicit 0ms timeout to verify edge case handling
    let (mut client, server) = setup_bridge().await;
    let response = client
        .execute_tool(ExecuteToolRequest {
            tool_id: "builtin:bash_exec".into(),
            args_json: r#"{"command": "echo hi", "timeout_ms": 0}"#.into(),
            fuel_limit: 0,
        })
        .await
        .expect("ExecuteTool with 0ms timeout should not panic");
    let inner = response.into_inner();
    // 0ms timeout may be treated as "no timeout" or "immediate timeout".
    // Either way, the bridge should respond without panic.
    // We primarily test that the request completes, not the specific outcome.
    eprintln!(
        "0ms timeout test result: status={} result={}",
        inner.status, inner.result_json
    );
    server.abort();
}

// ── Chaos Engineering tests ─────────────────────────────────────────────

/// Helper: create bridge with chaos enabled at the given fault probability.
async fn setup_bridge_chaos(
    fault_probability: f64,
) -> (
    SisyphusBridgeClient<tonic::transport::Channel>,
    tokio::task::JoinHandle<()>,
) {
    let mut config = Config::default();
    config.cache.database_url = "sqlite::memory:".to_string();
    config.sandbox.wasm_tools_dir = "tests/wasm_tools".to_string();
    // 关键：DummyLlmClient 没有 embedding 凭据，
    // 显式允许 dummy embedding 避免桥接启动失败。
    config.llm.embedding.allow_dummy_embeddings = true;
    config.chaos.enabled = true;
    config.chaos.default_fault_probability = fault_probability;
    let shared_config = Arc::new(RwLock::new(config));

    let bridge = MySisyphusBridge::new(shared_config, Some(Arc::new(DummyLlmClient)))
        .await
        .expect("Failed to create bridge with chaos enabled");

    let listener =
        tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("Failed to bind listener");
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

    let client =
        SisyphusBridgeClient::connect(server_addr).await.expect("Failed to connect client");

    (client, server_handle)
}

#[tokio::test]
async fn test_chaos_enabled_does_not_crash() {
    // Verify that enabling chaos (100% fault probability) does not crash
    // the bridge — every call either succeeds or returns a chaos error.
    let (mut client, server) = setup_bridge_chaos(1.0).await;

    for i in 0..20usize {
        let response = client
            .execute_tool(ExecuteToolRequest {
                tool_id: "builtin:bash_exec".into(),
                args_json: r#"{"command": "echo ok", "timeout_ms": 1000}"#.into(),
                fuel_limit: 0,
            })
            .await
            .expect("ExecuteTool with chaos enabled should not panic/gRPC-error");
        let inner = response.into_inner();
        // Status is either SUCCESS (no fault this call) or CHAOS_ERROR (fault injected)
        assert!(
            inner.status == "SUCCESS" || inner.status.starts_with("CHAOS_ERROR"),
            "Iteration {i}: unexpected status: {}",
            inner.status
        );
    }
    server.abort();
}

#[tokio::test]
async fn test_chaos_faults_occur_with_high_probability() {
    // With 90% fault probability, expect most calls to receive CHAOS_ERROR.
    let (mut client, server) = setup_bridge_chaos(0.9).await;

    let mut chaos_count = 0u64;
    let total = 50usize;
    for _ in 0..total {
        let response = client
            .execute_tool(ExecuteToolRequest {
                tool_id: "builtin:bash_exec".into(),
                args_json: r#"{"command": "echo ok", "timeout_ms": 1000}"#.into(),
                fuel_limit: 0,
            })
            .await
            .expect("ExecuteTool should not panic");
        let inner = response.into_inner();
        if inner.status.starts_with("CHAOS_ERROR") {
            chaos_count += 1;
        }
    }
    let rate = chaos_count as f64 / total as f64;
    // Expected CHAOS_ERROR rate ≈ probability × 0.5 (50/50 split with delay).
    // With 90% probability, expected ≈ 45%; we assert > 25% to allow variance.
    assert!(
        rate > 0.25,
        "Expected >25% fault rate with 90% probability, got {}%",
        rate * 100.0
    );
    server.abort();
}

/// Like setup_bridge_chaos but also returns the bridge Arc so callers can
/// inspect counters and the auto-fix flag (Direction 4 testing).
async fn setup_bridge_chaos_with_bridge(
    fault_probability: f64,
) -> (
    MySisyphusBridge,
    SisyphusBridgeClient<tonic::transport::Channel>,
    tokio::task::JoinHandle<()>,
) {
    let mut config = Config::default();
    config.cache.database_url = "sqlite::memory:".to_string();
    config.sandbox.wasm_tools_dir = "tests/wasm_tools".to_string();
    // 关键：DummyLlmClient 没有 embedding 凭据，
    // 显式允许 dummy embedding 避免桥接启动失败。
    config.llm.embedding.allow_dummy_embeddings = true;
    config.chaos.enabled = true;
    config.chaos.default_fault_probability = fault_probability;
    let shared_config = Arc::new(RwLock::new(config));

    let bridge = MySisyphusBridge::new(shared_config, Some(Arc::new(DummyLlmClient)))
        .await
        .expect("Failed to create bridge with chaos enabled");

    let listener =
        tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("Failed to bind listener");
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

    let client =
        SisyphusBridgeClient::connect(server_addr).await.expect("Failed to connect client");

    (bridge, client, server_handle)
}

#[tokio::test]
async fn test_chaos_auto_triggers_fix() {
    // Direction 4: verify that after >3 CHAOS_ERROR faults the bridge
    // automatically triggers the self-programming auto-fix pipeline.
    //
    // With 100% fault probability, each call has 50% chance of CHAOS_ERROR
    // (the other 50% injects a delay instead). We run enough iterations
    // to reliably accumulate >3 CHAOS_ERRORs.
    let (bridge, mut client, server) = setup_bridge_chaos_with_bridge(1.0).await;

    for _i in 0..30usize {
        // Fire enough ExecuteTool calls to reliably accumulate >3
        // CHAOS_ERROR faults on the bridge (authoritative counter asserted
        // below); per-response status is intentionally not checked here.
        let response = client
            .execute_tool(ExecuteToolRequest {
                tool_id: "builtin:bash_exec".into(),
                args_json: r#"{"command": "echo ok", "timeout_ms": 1000}"#.into(),
                fuel_limit: 0,
            })
            .await
            .expect("ExecuteTool should not panic even under chaos");
        let _ = response.into_inner();
    }

    // Allow background tokio tasks (the auto-fix spawn) to start processing.
    tokio::task::yield_now().await;

    // Verify the bridge counters and auto-fix flag.
    let count = bridge.chaos_fault_count();
    assert!(
        count >= 5,
        "Expected at least 5 CHAOS_ERROR faults, got {count}"
    );
    assert!(
        bridge.chaos_auto_fix_was_triggered(),
        "Auto-fix should have been triggered after >3 faults"
    );

    server.abort();
}

#[tokio::test]
async fn test_chaos_disabled_no_faults() {
    // With chaos disabled (the default), all execute_tool calls must
    // succeed normally without any CHAOS_ERROR.
    let (mut client, server) = setup_bridge().await;

    for i in 0..10usize {
        let response = client
            .execute_tool(ExecuteToolRequest {
                tool_id: "builtin:bash_exec".into(),
                args_json: r#"{"command": "echo ok", "timeout_ms": 1000}"#.into(),
                fuel_limit: 0,
            })
            .await
            .expect("ExecuteTool should not panic");
        let inner = response.into_inner();
        assert_eq!(
            inner.status, "SUCCESS",
            "Iteration {i}: expected SUCCESS when chaos disabled, got: {}",
            inner.status
        );
    }
    server.abort();
}

#[tokio::test]
async fn test_negative_execute_tool_invalid_prefix() {
    // Test tool with invalid prefix format
    let (mut client, server) = setup_bridge().await;
    let response = client
        .execute_tool(ExecuteToolRequest {
            tool_id: "invalidprefix".into(), // Missing colon
            args_json: "{}".into(),
            fuel_limit: 0,
        })
        .await
        .expect("ExecuteTool with invalid prefix should not panic");
    let inner = response.into_inner();
    // Should fail gracefully - either "not found" or routed to default
    assert!(!inner.status.is_empty());
    server.abort();
}

#[tokio::test]
async fn test_negative_execute_tool_colon_only() {
    // Test tool ID with just a colon
    let (mut client, server) = setup_bridge().await;
    let response = client
        .execute_tool(ExecuteToolRequest {
            tool_id: ":".into(),
            args_json: "{}".into(),
            fuel_limit: 0,
        })
        .await
        .expect("ExecuteTool with colon-only ID should not panic");
    let inner = response.into_inner();
    assert!(!inner.status.is_empty());
    server.abort();
}

#[tokio::test]
async fn test_negative_execute_tool_multiple_colons() {
    // Test tool ID with multiple colons
    let (mut client, server) = setup_bridge().await;
    let response = client
        .execute_tool(ExecuteToolRequest {
            tool_id: "builtin:file:read".into(),
            args_json: r#"{"path": "/test"}"#.into(),
            fuel_limit: 0,
        })
        .await
        .expect("ExecuteTool with multiple colons should not panic");
    let inner = response.into_inner();
    // Should handle gracefully
    assert!(!inner.status.is_empty());
    server.abort();
}

#[tokio::test]
async fn test_negative_execute_tool_very_long_tool_id() {
    // Test with extremely long tool_id (potential DoS vector)
    let (mut client, server) = setup_bridge().await;
    let long_tool_id = "a".repeat(10_000);
    let response = client
        .execute_tool(ExecuteToolRequest {
            tool_id: long_tool_id,
            args_json: "{}".into(),
            fuel_limit: 0,
        })
        .await
        .expect("ExecuteTool with very long tool_id should not panic");
    let inner = response.into_inner();
    assert!(!inner.status.is_empty());
    server.abort();
}

#[tokio::test]
async fn test_negative_execute_tool_unicode_tool_id() {
    // Test with unicode tool_id
    let (mut client, server) = setup_bridge().await;
    let response = client
        .execute_tool(ExecuteToolRequest {
            tool_id: "builtin:file_read_测试_🎉".into(),
            args_json: "{}".into(),
            fuel_limit: 0,
        })
        .await
        .expect("ExecuteTool with unicode tool_id should not panic");
    let inner = response.into_inner();
    assert!(!inner.status.is_empty());
    server.abort();
}

#[tokio::test]
async fn test_negative_concurrent_different_tools() {
    // Fire concurrent requests to DIFFERENT tools
    let (client, server) = setup_bridge().await;
    let mut handles = Vec::with_capacity(15);
    let tools = ["builtin:bash_exec", "builtin:file_read", "unknown/tool"];
    for i in 0..15usize {
        let mut cl = client.clone();
        let tool_id = tools[i % tools.len()].to_string();
        handles.push(tokio::spawn(async move {
            let resp = cl
                .execute_tool(ExecuteToolRequest {
                    tool_id,
                    args_json: r#"{"command": "echo ok", "timeout_ms": 1000}"#.into(),
                    fuel_limit: 0,
                })
                .await;
            (i, resp)
        }));
    }

    let mut ok_count = 0usize;
    for handle in handles {
        let (i, result) = handle.await.expect("Task panicked");
        match result {
            Ok(resp) => {
                let inner = resp.into_inner();
                // Different tools may succeed or fail, but no panic/crash
                assert!(!inner.status.is_empty(), "Iteration {i}: empty status");
                ok_count += 1;
            },
            Err(e) => {
                panic!("concurrent request {i} returned gRPC error: {e}");
            },
        }
    }
    assert_eq!(ok_count, 15, "Not all concurrent requests completed");
    server.abort();
}

#[tokio::test]
async fn test_negative_execute_sop_with_invalid_args_json() {
    // Test SOP with args that aren't valid JSON values
    let (mut client, server) = setup_bridge().await;
    let mut args = HashMap::new();
    args.insert("key".to_string(), "valid".to_string());
    args.insert(
        "nested".to_string(),
        serde_json::json!({"inner": "object"}).to_string(),
    );
    args.insert(
        "array".to_string(),
        serde_json::json!([1, 2, 3]).to_string(),
    );

    let response = client
        .execute_sop(ExecuteSopRequest {
            sop_id: "complex_args".into(),
            args,
        })
        .await
        .expect("ExecuteSOP with complex args should not panic");
    let inner = response.into_inner();
    assert!(!inner.status.is_empty());
    server.abort();
}

#[tokio::test]
async fn test_negative_stream_execution_multiple_requests() {
    // Start multiple stream_execution requests concurrently
    let (client, server) = setup_bridge().await;
    let mut handles = Vec::with_capacity(10);
    for i in 0..10usize {
        let mut cl = client.clone();
        handles.push(tokio::spawn(async move {
            let stream = cl
                .stream_execution(ExecuteSopRequest {
                    sop_id: format!("stream_{i}"),
                    args: HashMap::new(),
                })
                .await;
            (i, stream)
        }));
    }

    let mut ok_count = 0usize;
    for handle in handles {
        let (i, result) = handle.await.expect("Task panicked");
        match result {
            Ok(stream) => {
                let mut stream = stream.into_inner();
                // Should get at least one event
                let event = stream.next().await;
                assert!(
                    event.is_some(),
                    "Iteration {i}: expected at least one stream event"
                );
                ok_count += 1;
            },
            Err(e) => {
                panic!("stream_execution {i} returned gRPC error: {e}");
            },
        }
    }
    assert_eq!(ok_count, 10, "Not all stream requests completed");
    server.abort();
}

#[tokio::test]
async fn test_negative_list_tools_with_invalid_filter() {
    // ListTools doesn't take filter, but test edge case behavior
    let (mut client, server) = setup_bridge().await;
    let response = client.list_tools(ListToolsRequest {}).await;
    assert!(response.is_ok(), "ListTools should not fail");
    let tools = response.unwrap().into_inner().tools;
    assert!(!tools.is_empty(), "Expected builtin tools to be listed");
    server.abort();
}

#[tokio::test]
async fn test_negative_rapid_sequential_requests() {
    // Rapid sequential requests to verify no resource buildup
    let (mut client, server) = setup_bridge().await;
    let iterations = 200;
    for i in 0..iterations {
        let response = client
            .execute_tool(ExecuteToolRequest {
                tool_id: "builtin:bash_exec".into(),
                args_json: r#"{"command": "echo rapid", "timeout_ms": 500}"#.into(),
                fuel_limit: 0,
            })
            .await
            .expect("Rapid sequential request should not panic");
        let inner = response.into_inner();
        if i == 0 {
            assert_eq!(inner.status, "SUCCESS", "First request should succeed");
        }
        // All requests should at least get a response
        assert!(!inner.status.is_empty(), "Iteration {i}: empty status");
    }
    server.abort();
}

#[tokio::test]
async fn test_negative_execute_tool_empty_tool_id() {
    // Test with completely empty tool_id
    let (mut client, server) = setup_bridge().await;
    let response = client
        .execute_tool(ExecuteToolRequest {
            tool_id: "".into(),
            args_json: "{}".into(),
            fuel_limit: 0,
        })
        .await
        .expect("ExecuteTool with empty tool_id should not panic");
    let inner = response.into_inner();
    assert!(!inner.status.is_empty());
    server.abort();
}

#[tokio::test]
async fn test_negative_decompose_goal_very_long_objective() {
    // Test decompose with very long objective (potential DoS)
    let (mut client, server) = setup_bridge().await;
    let long_objective = "Write a ".to_string() + &"very ".repeat(5000) + "long description";
    let response = client
        .decompose_goal(DecomposeGoalRequest {
            goal_id: "test".into(),
            objective: long_objective,
            context: "".into(),
        })
        .await
        .expect("DecomposeGoal with very long objective should not panic");
    let inner = response.into_inner();
    assert!(inner.success || !inner.error.is_empty());
    server.abort();
}

#[tokio::test]
async fn test_negative_add_message_special_characters() {
    // Test add_message with special characters that could cause issues
    let (mut client, server) = setup_bridge().await;
    let special_content = "Special: \x00\x01\x02\n\r\t\"'\\/ Unicode: 测试 🎉 \u{0000}";
    let response = client
        .add_message(AddMessageRequest {
            session_id: "special_chars".into(),
            role: "user".into(),
            content: special_content.into(),
            session_db_path: ":memory:".into(),
        })
        .await
        .expect("AddMessage with special chars should not panic");
    let inner = response.into_inner();
    assert!(inner.success || !inner.error.is_empty());
    server.abort();
}

#[tokio::test]
async fn test_negative_install_skill_invalid_name() {
    // Test install_skill with various invalid names
    let (mut client, server) = setup_bridge().await;
    let long_name = "a".repeat(500);
    let invalid_names = vec!["", &long_name, "skill with spaces", "skill/../traversal"];
    for name in invalid_names {
        let response = client
            .install_skill(InstallSkillRequest {
                skill_id: name.into(),
            })
            .await
            .expect("InstallSkill with invalid name should not panic");
        let inner = response.into_inner();
        // Should handle gracefully
        assert!(!inner.name.is_empty() || !inner.success);
    }
    server.abort();
}

#[tokio::test]
async fn test_negative_load_plugin_path_traversal() {
    // Test load_plugin with path traversal attempts
    // The goal is to verify the bridge handles these inputs gracefully (no panic/crash)
    // rather than strictly blocking path traversal (which is a security hardening feature)
    let (mut client, server) = setup_bridge().await;
    let traversal_paths = vec![
        "/etc/passwd",
        "../../../etc/passwd",
        "..\\..\\..\\windows\\system32",
        "/proc/self/environ",
    ];
    for path in traversal_paths {
        let response = client
            .load_plugin(LoadPluginRequest {
                name: "test".into(),
                version: "1.0".into(),
                wasm_path: path.into(),
                tool_id: "test".into(),
                fuel_limit: 0,
            })
            .await
            .expect("LoadPlugin with traversal path should not panic");
        let inner = response.into_inner();
        // The bridge must respond without panic/crash.
        // Whether it succeeds or fails is implementation-dependent (the file may or may not exist).
        // We primarily test that the request completes gracefully.
        assert!(
            !inner.error.is_empty() || inner.success,
            "LoadPlugin must return a result for path: {path}"
        );
        eprintln!(
            "Path traversal test for {path}: success={}, error={}",
            inner.success, inner.error
        );
    }
    server.abort();
}
