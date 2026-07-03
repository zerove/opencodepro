use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use sisyphus_core::{SisyphusKernel, CoreError, SOPGraph, Consensus};
use sisyphus_intel::{
    SisyphusIntel, StandardReflector, StandardEvolver, 
    StrategicSopSynthesizer, GoalDrivenSynthesizer,
    MonteCarloSimulator,
};
use sisyphus_types::{ToolExecutor, ExecutionContext, WorkMode, ExecutionEvent, Milestone, Phase, GoalRef, MilestoneStatus, SOPNodeConfig, NodeKind, SOPGraphConfig};
use sisyphus_mem::MfsStore;
use sisyphus_tools_llm::metis::LlmClient;
use sisyphus_tools_llm_tools::meta_learner::MetaLearner;

// --- Mocks ---

struct MockLlmClient;
#[async_trait::async_trait]
impl LlmClient for MockLlmClient {
    async fn generate(&self, prompt: &str) -> Result<String, String> {
        if prompt.contains("Robustness") {
            Ok(r#"{"nodes": [{"id": "n1", "name": "robust_node", "tool_id": "fast_tool", "args": {}}], "edges": [], "start_node": "n1"}"#.to_string())
        } else if prompt.contains("Efficiency") {
            Ok(r#"{"nodes": [{"id": "n1", "name": "efficient_node", "tool_id": "fast_tool", "args": {}}], "edges": [], "start_node": "n1"}"#.to_string())
        } else {
            Ok(r#"{"nodes": [{"id": "n1", "name": "standard_node", "tool_id": "fast_tool", "args": {}}], "edges": [], "start_node": "n1"}"#.to_string())
        }
    }

    async fn chat(&self, system: &str, _user: &str, _stream: bool) -> Result<String, String> {
        if system.contains("senior engineering director") {
            Ok(r#"{
                "health_assessment": "The project is at critical risk due to repeated failures in core infrastructure access.",
                "trajectory": "Stagnating",
                "risk_factors": ["Critical resource unavailable", "Blocking structural failure"],
                "recommendations": [
                    {
                        "action": "Replace Secure-Vault tool",
                        "rationale": "The current vault access tool is consistently failing with ResourceUnavailable",
                        "priority": "high",
                        "target_milestone": "milestone-1"
                    }
                ],
                "confidence": 0.95
            }"#.to_string())
        } else {
            Ok("Mock chat response".into())
        }
    }
}

struct MockExecutor;
#[async_trait::async_trait]
impl ToolExecutor for MockExecutor {
    async fn execute(&self, _id: &str, _args: Vec<u8>, _fuel: u64) -> Result<Vec<u8>, sisyphus_types::TypesError> {
        Ok(b"success".to_vec())
    }
}

struct MockConsensus;
#[async_trait::async_trait]
impl Consensus for MockConsensus {
    async fn is_leader(&self) -> bool { true }
    async fn get_leader(&self) -> String { "node-1".into() }
    async fn propose(&self, _k: &str, _v: serde_json::Value) -> Result<(), String> { Ok(()) }
}

struct FailureExecutor {
    fail_tool_id: String,
}

#[async_trait::async_trait]
impl ToolExecutor for FailureExecutor {
    async fn execute(&self, id: &str, _args: Vec<u8>, _fuel: u64) -> Result<Vec<u8>, sisyphus_types::TypesError> {
        if id == self.fail_tool_id {
            Err(sisyphus_types::TypesError::NodeExecutionError("ResourceUnavailable: Secure-Vault is offline".into()))
        } else {
            Ok(b"success".to_vec())
        }
    }
}

struct FileSystemExecutor {
    base_path: std::path::PathBuf,
}

impl FileSystemExecutor {
    fn new(path: std::path::PathBuf) -> Self {
        Self { base_path: path }
    }
}

#[async_trait::async_trait]
impl ToolExecutor for FileSystemExecutor {
    async fn execute(&self, id: &str, args: Vec<u8>, _fuel: u64) -> Result<Vec<u8>, sisyphus_types::TypesError> {
        let args_str = String::from_utf8_lossy(&args);
        match id {
            "fs:read" => {
                let path = self.base_path.join(args_str.trim());
                tokio::fs::read(path).await.map_err(|e| sisyphus_types::TypesError::NodeExecutionError(e.to_string()))
            }
            "fs:write" => {
                let parts: Vec<&str> = args_str.splitn(2, '|').collect();
                if parts.len() < 2 {
                    return Err(sisyphus_types::TypesError::NodeExecutionError("Invalid args for fs:write. Expected 'filename|content'".into()));
                }
                let path = self.base_path.join(parts[0]);
                tokio::fs::write(path, parts[1]).await.map_err(|e| sisyphus_types::TypesError::NodeExecutionError(e.to_string()))?;
                Ok(b"written".to_vec())
            }
            "fs:list" => {
                let mut files = Vec::new();
                let mut entries = tokio::fs::read_dir(&self.base_path).await.map_err(|e| sisyphus_types::TypesError::NodeExecutionError(e.to_string()))?;
                while let Some(entry) = entries.next_entry().await.map_err(|e| sisyphus_types::TypesError::NodeExecutionError(e.to_string()))? {
                    files.push(entry.file_name().to_string_lossy().into_owned());
                }
                Ok(files.join(", ").into_bytes())
            }
            _ => Err(sisyphus_types::TypesError::NodeExecutionError(format!("Unsupported tool id: {}", id))),
        }
    }
}

#[tokio::test]
async fn test_full_system_integration_flow() {
    println!("🚀 Starting Full-System Integration Test...");

    let store = Arc::new(MfsStore::new("sqlite::memory:").await.unwrap());
    let tool_exec = Arc::new(MockExecutor);
    let llm = Arc::new(MockLlmClient);
    let simulator = Arc::new(MonteCarloSimulator::new(10));
    let intel = Arc::new(SisyphusIntel::new(simulator.clone(), Arc::new(StandardReflector), Arc::new(StandardEvolver::new())));
    
    let temp_dir = tempfile::tempdir().unwrap();
    let ml_path = temp_dir.path().join("ml_dir");
    let meta_learner = Arc::new(Mutex::new(MetaLearner::new(ml_path.to_str().unwrap())));
    
    let inner_syn = Arc::new(GoalDrivenSynthesizer::new(tool_exec.clone()));
    let syn = Arc::new(StrategicSopSynthesizer::new(inner_syn, simulator.clone()));
    
    let kernel = SisyphusKernel::new(
        store,
        tool_exec,
        intel,
        llm.clone(),
        syn,
        meta_learner.clone(),
        Arc::new(MockConsensus),
    ).with_strategic_reflector(llm.clone());

    let project_id = "project-ai-agent";
    let milestone_id = "milestone-infra";
    let phase_id = "phase-1";
    let goal_id = "goal-llm-connector";
    let goal_text = "Implement LLM Connector";

    kernel.roadmap_manager.create_project(project_id, "Build Simple AI Agent", "Infrastructure Setup").await.unwrap();
    let milestone = Milestone {
        id: milestone_id.to_string(),
        name: "Core Infrastructure".into(),
        phases: vec![Phase {
            id: phase_id.to_string(),
            name: "Initialization".into(),
            goals: vec![GoalRef {
                goal_id: goal_id.to_string(),
                description: goal_text.to_string(),
                status: MilestoneStatus::Pending,
            }],
            depends_on: vec![],
        }],
        status: MilestoneStatus::Pending,
    };
    kernel.roadmap_manager.add_milestone(project_id, milestone).await.unwrap();

    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let (res, report, _health) = kernel.execute_and_record(
        goal_text,
        "test_sop_id",
        project_id,
        milestone_id,
        phase_id,
        goal_id,
        Some(event_tx),
    ).await.unwrap();

    assert!(res.to_string().contains("success"));
    let mut event_count = 0;
    while let Ok(_) = event_rx.try_recv() {
        event_count += 1;
    }
    assert!(event_count > 0);

    {
        let mut ml = meta_learner.lock().unwrap();
        for _ in 0..5 {
            ml.record_outcome(sisyphus_types::MetaLearningSample {
                context: serde_json::json!({ "goal": goal_text }),
                strategy: "Robustness".into(),
                success: true,
                duration_secs: 1.0,
                timestamp: 0,
            }).unwrap();
        }
    }

    let (_, report2) = kernel.execute_goal(goal_text, "test_sop_id_2", None).await.unwrap();
    assert!(report2.contains("Robustness") || report2.contains("Strategic selection"));

    let reflection = kernel.strategic_reflect(project_id).await;
    assert!(reflection.is_some());

    println!("✅ Full-System Integration Test Passed!");
}

#[tokio::test]
async fn test_complex_strategic_goal() {
    println!("🚀 Starting Complex Strategic Goal Test...");

    let store = Arc::new(MfsStore::new("sqlite::memory:").await.unwrap());
    let tool_exec = Arc::new(MockExecutor);
    let llm = Arc::new(MockLlmClient);
    let simulator = Arc::new(MonteCarloSimulator::new(10));
    let intel = Arc::new(SisyphusIntel::new(simulator.clone(), Arc::new(StandardReflector), Arc::new(StandardEvolver::new())));
    
    let temp_dir = tempfile::tempdir().unwrap();
    let ml_path = temp_dir.path().join("ml_complex");
    let meta_learner = Arc::new(Mutex::new(MetaLearner::new(ml_path.to_str().unwrap())));
    
    let inner_syn = Arc::new(GoalDrivenSynthesizer::new(tool_exec.clone()));
    let syn = Arc::new(StrategicSopSynthesizer::new(inner_syn, simulator.clone()));
    
    let kernel = SisyphusKernel::new(
        store,
        tool_exec,
        intel,
        llm.clone(),
        syn,
        meta_learner.clone(),
        Arc::new(MockConsensus),
    ).with_strategic_reflector(llm.clone());

    let complex_goal = "Fetch API specs, process rate limiting rules, validate security, and save configuration";
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let (res, report) = kernel.execute_goal(complex_goal, "complex_sop_id", Some(event_tx)).await.unwrap();

    assert!(res.to_string().contains("success"));
    assert!(report.contains("Linear Pipeline"));

    let mut completed_nodes = Vec::new();
    while let Ok(event) = event_rx.try_recv() {
        if let ExecutionEvent::NodeCompleted { node_name, .. } = event {
            completed_nodes.push(node_name);
        }
    }

    let plan_part = report.split(" | ").next().unwrap_or("");
    let pipeline_desc = plan_part.rsplit_once(": ").map(|(_, desc)| desc.trim()).unwrap_or("");
    let clean_desc = pipeline_desc.replace("Linear Pipeline: ", "");
    let expected_nodes: Vec<String> = clean_desc
        .split(" -> ")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    
    assert_eq!(completed_nodes.len(), expected_nodes.len());
    for (actual, expected) in completed_nodes.iter().zip(expected_nodes.iter()) {
        assert_eq!(actual, expected);
    }

    println!("✅ Complex Strategic Goal Test Passed!");
}

#[tokio::test]
async fn test_strategic_reflection_loop() {
    println!("🚀 Starting Strategic Reflection Loop Test...");

    let store = Arc::new(MfsStore::new("sqlite::memory:").await.unwrap());
    let fail_tool_id = "secure_vault_tool";
    let tool_exec = Arc::new(FailureExecutor { fail_tool_id: fail_tool_id.to_string() });
    let llm = Arc::new(MockLlmClient);
    let simulator = Arc::new(MonteCarloSimulator::new(10));
    let intel = Arc::new(SisyphusIntel::new(simulator.clone(), Arc::new(StandardReflector), Arc::new(StandardEvolver::new())));
    
    let temp_dir = tempfile::tempdir().unwrap();
    let ml_path = temp_dir.path().join("ml_reflect");
    let meta_learner = Arc::new(Mutex::new(MetaLearner::new(ml_path.to_str().unwrap())));
    
    let inner_syn = Arc::new(GoalDrivenSynthesizer::new(tool_exec.clone()));
    let syn = Arc::new(StrategicSopSynthesizer::new(inner_syn, simulator.clone()));
    
    let kernel = SisyphusKernel::new(
        store,
        tool_exec,
        intel,
        llm.clone(),
        syn,
        meta_learner.clone(),
        Arc::new(MockConsensus),
    ).with_strategic_reflector(llm.clone());

    let project_id = "project-trap";
    let milestone_id = "milestone-1";
    let phase_id = "phase-1";
    let goal_id = "goal-vault-access";
    let goal_text = "Access secure vault and retrieve keys";

    kernel.roadmap_manager.create_project(project_id, "Trap Project", "Test structural failure").await.unwrap();
    let milestone = Milestone {
        id: milestone_id.to_string(),
        name: "Initial Access".into(),
        phases: vec![Phase {
            id: phase_id.to_string(),
            name: "Phase 1".into(),
            goals: vec![GoalRef {
                goal_id: goal_id.to_string(),
                description: goal_text.to_string(),
                status: MilestoneStatus::Pending,
            }],
            depends_on: vec![],
        }],
        status: MilestoneStatus::Pending,
    };
    kernel.roadmap_manager.add_milestone(project_id, milestone).await.unwrap();

    for _ in 0..3 {
        let graph = SOPGraph::from_config(&SOPGraphConfig {
            nodes: vec![SOPNodeConfig {
                name: "vault_node".into(),
                kind: NodeKind::Wasm {
                    tool_id: fail_tool_id.into(),
                    fuel_limit: 1_000_000,
                },
            }],
            edges: vec![],
        }, kernel.tool_executor.clone()).unwrap();
        let _ = kernel.execute_with_cognition(&graph, "trap_sop", None, None).await;
    }

    let reflection = kernel.strategic_reflect(project_id).await;
    assert!(reflection.is_some());
    let reflect = reflection.unwrap();
    assert!(!reflect.recommendations.is_empty());
    assert!(reflect.confidence > 0.0);

    println!("✅ Strategic Reflection Loop Test Passed!");
}

#[tokio::test]
async fn test_real_world_fs_loop() {
    println!("🚀 Starting Real-World FS Loop Test...");

    let temp_dir = tempfile::tempdir().unwrap();
    let fs_base = temp_dir.path().to_path_buf();
    let tool_exec = Arc::new(FileSystemExecutor::new(fs_base.clone()));
    
    let store = Arc::new(MfsStore::new("sqlite::memory:").await.unwrap());
    let llm = Arc::new(MockLlmClient);
    let simulator = Arc::new(MonteCarloSimulator::new(10));
    let intel = Arc::new(SisyphusIntel::new(simulator.clone(), Arc::new(StandardReflector), Arc::new(StandardEvolver::new())));
    
    let ml_temp = tempfile::tempdir().unwrap();
    let meta_learner = Arc::new(Mutex::new(MetaLearner::new(ml_temp.path().to_str().unwrap())));
    
    let inner_syn = Arc::new(GoalDrivenSynthesizer::new(tool_exec.clone()));
    let syn = Arc::new(StrategicSopSynthesizer::new(inner_syn, simulator.clone()));
    
    let kernel = SisyphusKernel::new(
        store,
        tool_exec,
        intel,
        llm.clone(),
        syn,
        meta_learner.clone(),
        Arc::new(MockConsensus),
    ).with_strategic_reflector(llm.clone());

    println!("Sub-test A: Happy Path (file exists)...");
    tokio::fs::write(fs_base.join("version.txt"), "1.0").await.unwrap();
    
    let graph = SOPGraph::from_config(&SOPGraphConfig {
        nodes: vec![
            SOPNodeConfig {
                name: "read_v".into(),
                kind: NodeKind::Wasm { tool_id: "fs:read".into(), fuel_limit: 1_000_000 },
            },
            SOPNodeConfig {
                name: "write_v".into(),
                kind: NodeKind::Wasm { tool_id: "fs:write".into(), fuel_limit: 1_000_000 },
            },
        ],
        edges: vec![(0, 1)],
    }, kernel.tool_executor.clone()).unwrap();

    let (res, report, _) = kernel.execute_with_cognition(&graph, "fs_sop", None, None).await.unwrap();
    println!("Happy path result: {}, Report: {}", res, report);
    
    println!("Sub-test B: Failure Path (file missing)...");
    tokio::fs::remove_file(fs_base.join("version.txt")).await.unwrap();
    
    let (res_fail, report_fail, _) = kernel.execute_with_cognition(&graph, "fs_sop_fail", None, None).await.unwrap();
    println!("Failure path result: {}, Report: {}", res_fail, report_fail);
    
    assert!(report_fail.contains("deviation") || report_fail.contains("error"), 
            "The cognitive loop should have detected a deviation due to missing file");

    println!("✅ Real-World FS Loop Test Passed!");
}
