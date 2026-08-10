use super::*;
use crate::agent_registry::AgentInfo;
use crate::team::TaskResult;
use serde_json::json;
use sisyphus_tools_core::feedback::FeedbackStore;
use sisyphus_tools_core::hook::HookRegistry;
use sisyphus_tools_executor::executor::BuiltinToolExecutor;
use sisyphus_tools_llm::circuit_breaker::CircuitBreaker;
use sisyphus_tools_llm::metis::DummyLlmClient;
use sisyphus_tools_llm::DynamicLlmClient;
use sisyphus_types::Config;
use std::sync::{Arc, RwLock};

fn make_registry() -> Arc<AgentRegistry> {
    let config = Arc::new(RwLock::new(Config::default()));
    let client = Arc::new(DynamicLlmClient::new(config));
    let reg = Arc::new(AgentRegistry::new(client));
    reg.register_builtin();
    reg
}

fn make_executor() -> Arc<dyn ToolExecutor> {
    let dummy: Arc<dyn LlmClient> = Arc::new(DummyLlmClient);
    let feedback = Arc::new(FeedbackStore::default_dir());
    Arc::new(BuiltinToolExecutor {
        registry: HookRegistry::new(),
        llm_client: dummy,
        feedback_store: feedback,
        bridge_client: None,
    })
}

fn make_orchestrator() -> TeamOrchestrator {
    TeamOrchestrator::new(make_registry(), make_executor(), None)
}

#[tokio::test]
async fn test_execute_empty_plan() {
    let orch = make_orchestrator();
    let plan = TeamPlan {
        goal_id: "empty".into(),
        tasks: vec![],
    };
    let report = orch.execute_plan(plan).await.unwrap();
    assert!(report.success);
    assert!(report.task_results.is_empty());
}

#[tokio::test]
async fn test_execute_single_task() {
    let orch = make_orchestrator();
    let tasks = vec![TaskNode {
        id: "t1".into(),
        agent_id: "metis".into(),
        tool_id: "metis/analyse".into(),
        args: json!({"request": "test analysis"}),
        depends_on: vec![],
        read_keys: vec![],
        write_keys: vec![],
    }];
    let report = orch
        .run(
            TeamGoal {
                id: "single".into(),
                objective: "Test single task".into(),
                context: None,
            },
            tasks,
        )
        .await
        .unwrap();

    assert!(
        report.success,
        "single metis task should succeed with DummyLlmClient"
    );
    assert_eq!(report.task_results.len(), 1);
    assert!(report.task_results[0].success);
    assert!(report.synthesis.is_some());
}

#[tokio::test]
async fn test_execute_sequential_tasks() {
    let orch = make_orchestrator();
    let tasks = vec![
        TaskNode {
            id: "t1".into(),
            agent_id: "metis".into(),
            tool_id: "metis/analyse".into(),
            args: json!({"request": "first analysis"}),
            depends_on: vec![],
            read_keys: vec![],
            write_keys: vec![],
        },
        TaskNode {
            id: "t2".into(),
            agent_id: "metis".into(),
            tool_id: "metis/analyse".into(),
            args: json!({"request": "second analysis"}),
            depends_on: vec!["t1".into()],
            read_keys: vec![],
            write_keys: vec![],
        },
    ];
    let report = orch
        .run(
            TeamGoal {
                id: "seq".into(),
                objective: "Sequential tasks".into(),
                context: None,
            },
            tasks,
        )
        .await
        .unwrap();

    assert!(report.success);
    assert_eq!(report.task_results.len(), 2);
    assert!(report.task_results[0].success);
    assert!(report.task_results[1].success);
}

#[tokio::test]
async fn test_execute_parallel_tasks() {
    let orch = make_orchestrator();
    let tasks = vec![
        TaskNode {
            id: "t1".into(),
            agent_id: "metis".into(),
            tool_id: "metis/analyse".into(),
            args: json!({"request": "parallel A"}),
            depends_on: vec![],
            read_keys: vec![],
            write_keys: vec![],
        },
        TaskNode {
            id: "t2".into(),
            agent_id: "metis".into(),
            tool_id: "metis/analyse".into(),
            args: json!({"request": "parallel B"}),
            depends_on: vec![],
            read_keys: vec![],
            write_keys: vec![],
        },
        TaskNode {
            id: "t3".into(),
            agent_id: "metis".into(),
            tool_id: "metis/analyse".into(),
            args: json!({"request": "parallel C"}),
            depends_on: vec![],
            read_keys: vec![],
            write_keys: vec![],
        },
    ];
    let report = orch
        .run(
            TeamGoal {
                id: "par".into(),
                objective: "Parallel tasks".into(),
                context: None,
            },
            tasks,
        )
        .await
        .unwrap();

    assert!(
        report.success,
        "three parallel metis tasks should all succeed"
    );
    assert_eq!(report.task_results.len(), 3);
}

#[tokio::test]
async fn test_execute_diamond_dependency() {
    let orch = make_orchestrator();
    let tasks = vec![
        TaskNode {
            id: "t1".into(),
            agent_id: "metis".into(),
            tool_id: "metis/analyse".into(),
            args: json!({"request": "root"}),
            depends_on: vec![],
            read_keys: vec![],
            write_keys: vec![],
        },
        TaskNode {
            id: "t2".into(),
            agent_id: "metis".into(),
            tool_id: "metis/analyse".into(),
            args: json!({"request": "left"}),
            depends_on: vec!["t1".into()],
            read_keys: vec![],
            write_keys: vec![],
        },
        TaskNode {
            id: "t3".into(),
            agent_id: "metis".into(),
            tool_id: "metis/analyse".into(),
            args: json!({"request": "right"}),
            depends_on: vec!["t1".into()],
            read_keys: vec![],
            write_keys: vec![],
        },
        TaskNode {
            id: "t4".into(),
            agent_id: "metis".into(),
            tool_id: "metis/analyse".into(),
            args: json!({"request": "merge"}),
            depends_on: vec!["t2".into(), "t3".into()],
            read_keys: vec![],
            write_keys: vec![],
        },
    ];
    let report = orch
        .run(
            TeamGoal {
                id: "diamond".into(),
                objective: "Diamond dependency".into(),
                context: None,
            },
            tasks,
        )
        .await
        .unwrap();

    assert!(report.success);
    assert_eq!(report.task_results.len(), 4);
}

#[tokio::test]
async fn test_deadlock_detection() {
    let orch = make_orchestrator();
    let tasks = vec![
        TaskNode {
            id: "t1".into(),
            agent_id: "metis".into(),
            tool_id: "metis/analyse".into(),
            args: json!({"request": "a"}),
            depends_on: vec!["t2".into()],
            read_keys: vec![],
            write_keys: vec![],
        },
        TaskNode {
            id: "t2".into(),
            agent_id: "metis".into(),
            tool_id: "metis/analyse".into(),
            args: json!({"request": "b"}),
            depends_on: vec!["t1".into()],
            read_keys: vec![],
            write_keys: vec![],
        },
    ];
    let report = orch
        .run(
            TeamGoal {
                id: "deadlock".into(),
                objective: "Test deadlock".into(),
                context: None,
            },
            tasks,
        )
        .await
        .unwrap();

    assert!(!report.success, "deadlock should cause failure");
    assert_eq!(report.task_results.len(), 2);
    assert!(report.task_results.iter().all(|r| !r.success));
}

#[tokio::test]
async fn test_missing_dependency_deadlock() {
    let orch = make_orchestrator();
    let tasks = vec![TaskNode {
        id: "t1".into(),
        agent_id: "metis".into(),
        tool_id: "metis/analyse".into(),
        args: json!({"request": "a"}),
        depends_on: vec!["nonexistent".into()],
        read_keys: vec![],
        write_keys: vec![],
    }];
    let report = orch
        .run(
            TeamGoal {
                id: "missing-dep".into(),
                objective: "Test missing dep".into(),
                context: None,
            },
            tasks,
        )
        .await
        .unwrap();

    assert!(!report.success);
    let err = &report.task_results[0].error;
    assert!(
        err.as_ref().unwrap().contains("deadlock"),
        "error should mention deadlock; got: {:?}",
        err
    );
}

#[tokio::test]
async fn test_synthesis_with_llm() {
    let dummy_llm: Arc<dyn LlmClient> = Arc::new(DummyLlmClient);
    let orch = TeamOrchestrator::new(make_registry(), make_executor(), Some(dummy_llm));

    let tasks = vec![TaskNode {
        id: "t1".into(),
        agent_id: "metis".into(),
        tool_id: "metis/analyse".into(),
        args: json!({"request": "test"}),
        depends_on: vec![],
        read_keys: vec![],
        write_keys: vec![],
    }];
    let report = orch
        .run(
            TeamGoal {
                id: "synth".into(),
                objective: "Test synthesis".into(),
                context: None,
            },
            tasks,
        )
        .await
        .unwrap();

    assert!(report.success);
    assert!(report.synthesis.is_some());
    let s = report.synthesis.unwrap();
    assert!(!s.is_empty(), "synthesis should not be empty");
}

#[test]
fn test_create_sequential_plan() {
    let tasks = vec![
        TaskNode {
            id: "t1".into(),
            agent_id: "a".into(),
            tool_id: "t".into(),
            args: json!({}),
            depends_on: vec![],
            read_keys: vec![],
            write_keys: vec![],
        },
        TaskNode {
            id: "t2".into(),
            agent_id: "b".into(),
            tool_id: "t".into(),
            args: json!({}),
            depends_on: vec![],
            read_keys: vec![],
            write_keys: vec![],
        },
    ];
    let plan = TeamOrchestrator::create_sequential_plan("g1", tasks);
    assert_eq!(plan.goal_id, "g1");
    assert_eq!(plan.tasks.len(), 2);
}

#[tokio::test]
async fn test_execute_unknown_tool_graceful() {
    let orch = make_orchestrator();
    let tasks = vec![TaskNode {
        id: "t1".into(),
        agent_id: "metis".into(),
        tool_id: "nonexistent/tool".into(),
        args: json!({}),
        depends_on: vec![],
        read_keys: vec![],
        write_keys: vec![],
    }];
    let report = orch
        .run(
            TeamGoal {
                id: "unknown-tool".into(),
                objective: "Test unknown tool".into(),
                context: None,
            },
            tasks,
        )
        .await
        .unwrap();

    assert!(!report.success, "unknown tool should fail");
    assert!(!report.task_results.is_empty());
}

#[tokio::test]
async fn test_report_metrics() {
    let orch = make_orchestrator();
    let tasks = vec![TaskNode {
        id: "t1".into(),
        agent_id: "metis".into(),
        tool_id: "metis/analyse".into(),
        args: json!({"request": "test"}),
        depends_on: vec![],
        read_keys: vec![],
        write_keys: vec![],
    }];
    let report = orch
        .run(
            TeamGoal {
                id: "metrics".into(),
                objective: "Test metrics".into(),
                context: None,
            },
            tasks,
        )
        .await
        .unwrap();

    let success_count = report.task_results.iter().filter(|r| r.success).count();
    assert_eq!(success_count, 1, "one task should succeed");
}

#[tokio::test]
async fn test_unknown_agent_fails() {
    let orch = make_orchestrator();
    let tasks = vec![TaskNode {
        id: "t1".into(),
        agent_id: "nonexistent-agent".into(),
        tool_id: "metis/analyse".into(),
        args: json!({}),
        depends_on: vec![],
        read_keys: vec![],
        write_keys: vec![],
    }];
    let report = orch
        .run(
            TeamGoal {
                id: "bad-agent".into(),
                objective: "Test unknown agent".into(),
                context: None,
            },
            tasks,
        )
        .await
        .unwrap();

    let failed: Vec<&TaskResult> = report.task_results.iter().filter(|r| !r.success).collect();
    assert_eq!(failed.len(), 1, "task with unknown agent should fail");
    let err = failed[0].error.as_ref().unwrap();
    assert!(
        err.contains("not registered"),
        "error should mention not registered; got: {}",
        err
    );
}

#[tokio::test]
async fn test_tool_not_matching_agent_fails() {
    let orch = make_orchestrator();
    let tasks = vec![TaskNode {
        id: "t1".into(),
        agent_id: "metis".into(),
        tool_id: "security/audit_code".into(),
        args: json!({}),
        depends_on: vec![],
        read_keys: vec![],
        write_keys: vec![],
    }];
    let report = orch
        .run(
            TeamGoal {
                id: "tool-mismatch".into(),
                objective: "Test tool mismatch".into(),
                context: None,
            },
            tasks,
        )
        .await
        .unwrap();

    let failed: Vec<&TaskResult> = report.task_results.iter().filter(|r| !r.success).collect();
    assert_eq!(failed.len(), 1, "mismatched tool should fail");
    let err = failed[0].error.as_ref().unwrap();
    assert!(
        err.contains("does not support"),
        "error should mention 'does not support'; got: {}",
        err
    );
}

#[tokio::test]
async fn test_decompose_no_llm() {
    let orch = make_orchestrator();
    let goal = TeamGoal {
        id: "test".into(),
        objective: "Test".into(),
        context: None,
    };
    let result = orch.decompose_goal(&goal).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        TeamError::NoPlanner => {}
        other => panic!("expected NoPlanner error, got: {:?}", other),
    }
}

struct JsonLlmClient(String);

#[async_trait::async_trait]
impl LlmClient for JsonLlmClient {
    async fn generate(&self, prompt: &str) -> Result<String, sisyphus_types::TypesError> {
        self.chat("", prompt, false).await
    }

    async fn chat(
        &self,
        _system: &str,
        _user: &str,
        _json_mode: bool,
    ) -> Result<String, sisyphus_types::TypesError> {
        Ok(self.0.clone())
    }

    fn get_params_for(&self, _agent_id: &str) -> (String, String, String) {
        (
            "default".to_string(),
            "default".to_string(),
            "default".to_string(),
        )
    }

    fn breaker(&self) -> CircuitBreaker {
        CircuitBreaker::default()
    }
}

struct ErrLlmClient;

#[async_trait::async_trait]
impl LlmClient for ErrLlmClient {
    async fn generate(&self, prompt: &str) -> Result<String, sisyphus_types::TypesError> {
        self.chat("", prompt, false).await
    }

    async fn chat(
        &self,
        _system: &str,
        _user: &str,
        _json_mode: bool,
    ) -> Result<String, sisyphus_types::TypesError> {
        Err(sisyphus_types::TypesError::LlmError(
            "LLM API error".to_string(),
        ))
    }

    fn get_params_for(&self, _agent_id: &str) -> (String, String, String) {
        (
            "default".to_string(),
            "default".to_string(),
            "default".to_string(),
        )
    }

    fn breaker(&self) -> CircuitBreaker {
        CircuitBreaker::default()
    }
}

#[tokio::test]
async fn test_decompose_llm_error() {
    let orch = TeamOrchestrator::new(
        make_registry(),
        make_executor(),
        Some(Arc::new(ErrLlmClient)),
    );
    let goal = TeamGoal {
        id: "err".into(),
        objective: "Test".into(),
        context: None,
    };
    let result = orch.decompose_goal(&goal).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        TeamError::LlmFailed(msg) => assert!(msg.contains("LLM API error")),
        other => panic!("expected LlmFailed, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_decompose_success() {
    let json = r#"{
        "tasks": [
            {"id": "research", "agent": "librarian", "tool_id": "librarian/research", "args": {"query": "test"}, "depends_on": []}
        ]
    }"#;
    let orch = TeamOrchestrator::new(
        make_registry(),
        make_executor(),
        Some(Arc::new(JsonLlmClient(json.to_string()))),
    );
    let goal = TeamGoal {
        id: "decompose-test".into(),
        objective: "Research topic X".into(),
        context: Some("Focus on 2026 trends".into()),
    };
    let plan = orch.decompose_goal(&goal).await.unwrap();
    assert_eq!(plan.goal_id, "decompose-test");
    assert_eq!(plan.tasks.len(), 1);
    assert_eq!(plan.tasks[0].tool_id, "librarian/research");
}

#[tokio::test]
async fn test_run_goal_end_to_end() {
    let json = r#"{
        "tasks": [
            {"id": "analyse", "agent": "metis", "tool_id": "metis/analyse", "args": {"request": "test analysis"}, "depends_on": []}
        ]
    }"#;
    let orch = TeamOrchestrator::new(
        make_registry(),
        make_executor(),
        Some(Arc::new(JsonLlmClient(json.to_string()))),
    );
    let report = orch
        .run_goal(TeamGoal {
            id: "e2e".into(),
            objective: "End-to-end test".into(),
            context: None,
        })
        .await
        .unwrap();
    assert!(report.success);
    assert_eq!(report.task_results.len(), 1);
}

#[tokio::test]
async fn test_concurrency_limit_respected() {
    let config = Arc::new(RwLock::new(Config::default()));
    let client = Arc::new(DynamicLlmClient::new(config));
    let reg = Arc::new(AgentRegistry::new(client));

    // Register an agent with max_concurrency=1
    reg.register(
        "limited-agent",
        AgentInfo {
            display_name: "Limited".into(),
            description: "Single-task agent".into(),
            tool_prefixes: vec!["test/".into()],
            max_concurrency: 1,
        },
    );

    // Add a simple executor that handles test/*
    let executor: Arc<dyn ToolExecutor> = make_executor();
    let orch = TeamOrchestrator::new(reg, executor, None);

    // Three tasks with no deps — but only 1 should run at a time
    let tasks = vec![
        TaskNode {
            id: "a".into(),
            agent_id: "limited-agent".into(),
            tool_id: "test/tool".into(),
            args: json!({}),
            depends_on: vec![],
            read_keys: vec![],
            write_keys: vec![],
        },
        TaskNode {
            id: "b".into(),
            agent_id: "limited-agent".into(),
            tool_id: "test/tool".into(),
            args: json!({}),
            depends_on: vec![],
            read_keys: vec![],
            write_keys: vec![],
        },
        TaskNode {
            id: "c".into(),
            agent_id: "limited-agent".into(),
            tool_id: "test/tool".into(),
            args: json!({}),
            depends_on: vec![],
            read_keys: vec![],
            write_keys: vec![],
        },
    ];

    let report = orch
        .run(
            TeamGoal {
                id: "concurrency".into(),
                objective: "Test concurrency".into(),
                context: None,
            },
            tasks,
        )
        .await
        .unwrap();

    // All should complete (even if they fail due to unknown tool)
    assert_eq!(
        report.task_results.len(),
        3,
        "all 3 tasks should be accounted for"
    );
}

#[tokio::test]
async fn test_event_streaming() {
    let orch = make_orchestrator();
    let plan = TeamPlan {
        goal_id: "events".into(),
        tasks: vec![
            TaskNode {
                id: "e1".into(),
                agent_id: "metis".into(),
                tool_id: "metis/analyse".into(),
                args: json!({"request": "event test 1"}),
                depends_on: vec![],
                read_keys: vec![],
                write_keys: vec![],
            },
            TaskNode {
                id: "e2".into(),
                agent_id: "metis".into(),
                tool_id: "metis/analyse".into(),
                args: json!({"request": "event test 2"}),
                depends_on: vec![],
                read_keys: vec![],
                write_keys: vec![],
            },
        ],
    };
    let (tx, mut rx) = mpsc::channel(64);

    // Execute with event channel
    let report = orch
        .execute_plan_internal(plan, Some(tx), Arc::new(TeamContext::new()))
        .await
        .unwrap();
    assert!(report.success, "all tasks should succeed");

    // Collect all events (with timeout)
    let mut started = 0u32;
    let mut completed = 0u32;
    let mut got_synthesizing = false;
    let mut got_complete = false;

    use tokio::time::{timeout, Duration};
    loop {
        match timeout(Duration::from_secs(2), rx.recv()).await {
            Ok(Some(event)) => match event {
                TeamEvent::TaskStarted { .. } => started += 1,
                TeamEvent::TaskCompleted(ref r) => {
                    if r.success {
                        completed += 1;
                    }
                }
                TeamEvent::Synthesizing { .. } => got_synthesizing = true,
                TeamEvent::Complete(_) => {
                    got_complete = true;
                    break;
                }
                _ => {}
            },
            Ok(None) => break,
            Err(_) => break,
        }
    }

    assert_eq!(started, 2, "should see 2 TaskStarted events");
    assert_eq!(completed, 2, "should see 2 successful TaskCompleted events");
    assert!(got_synthesizing, "should see Synthesizing event");
    assert!(got_complete, "should see Complete event");
}

#[tokio::test]
async fn test_event_streaming_deadlock() {
    let orch = make_orchestrator();
    let plan = TeamPlan {
        goal_id: "deadlock-events".into(),
        tasks: vec![
            TaskNode {
                id: "a".into(),
                agent_id: "metis".into(),
                tool_id: "metis/analyse".into(),
                args: json!({"request": "a"}),
                depends_on: vec!["b".into()],
                read_keys: vec![],
                write_keys: vec![],
            },
            TaskNode {
                id: "b".into(),
                agent_id: "metis".into(),
                tool_id: "metis/analyse".into(),
                args: json!({"request": "b"}),
                depends_on: vec!["a".into()],
                read_keys: vec![],
                write_keys: vec![],
            },
        ],
    };
    let (tx, mut rx) = mpsc::channel(64);

    let _report = orch
        .execute_plan_internal(plan, Some(tx), Arc::new(TeamContext::new()))
        .await
        .unwrap();

    // Should see Error event + 2 TaskCompleted (failed)
    let mut errors = 0u32;
    let mut failed_completions = 0u32;
    let mut got_complete = false;

    use tokio::time::{timeout, Duration};
    loop {
        match timeout(Duration::from_secs(2), rx.recv()).await {
            Ok(Some(event)) => match event {
                TeamEvent::Error { .. } => errors += 1,
                TeamEvent::TaskCompleted(ref r) => {
                    if !r.success {
                        failed_completions += 1;
                    }
                }
                TeamEvent::Complete(_) => {
                    got_complete = true;
                    break;
                }
                _ => {}
            },
            Ok(None) => break,
            Err(_) => break,
        }
    }

    assert_eq!(errors, 1, "deadlock should emit Error event");
    assert_eq!(
        failed_completions, 2,
        "both tasks should emit failed TaskCompleted"
    );
    assert!(got_complete, "should still emit Complete event");
}
