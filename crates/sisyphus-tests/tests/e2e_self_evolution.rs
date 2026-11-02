//! 自编程端到端测试：真实编译 + 真实沙箱执行 + 真实质量评估。
//!
//! 与旧的 e2e 测试的关键区别：
//! - 生成的代码按真实契约写（`pub fn run(input: &str) -> Result<String, String>`）；
//! - `WasmCompiler` 真的调用 rustc 产出 wasm；
//! - 探针通过 `WasmToolExecutor` 真的在 wasmtime 里执行一次；
//! - 断言产物落盘、事件顺序、以及工具确实可被沙箱调用。
//!
//! 如果 rustc 的 wasm 目标没装，测试会跳过（并说明原因）——那是环境缺失，
//! 不是代码缺陷。

use async_trait::async_trait;
use sisyphus_core::execution_manager::ExecutionManager;
use sisyphus_core::SelfProgrammingManager;
use sisyphus_intel::ToolGap;
use sisyphus_knowledge::AssetManager;
use sisyphus_mem::SisyphusStore;
use sisyphus_tools_llm::{circuit_breaker::CircuitBreaker, LlmClient};
use sisyphus_types::errors::TypesError;
use sisyphus_types::TestResult;
use sisyphus_types::{node::TestExecutor, tools::ToolSpec, CognitiveEvent};
use std::sync::Arc;
use tokio::sync::mpsc;

/// 按系统提示分派的假 LLM：代码生成返回实现，语义审计返回满分。
struct ProgrammableLlmClient {
    code: String,
}

#[async_trait]
impl LlmClient for ProgrammableLlmClient {
    async fn chat(
        &self,
        system: &str,
        prompt: &str,
        _stream: bool,
    ) -> Result<String, sisyphus_types::TypesError> {
        if system.contains("Tool Auditor") {
            return Ok("0.95".into());
        }
        if system.contains("Rust Engineer") {
            // 证明提示词真的把规范喂给了模型（而不是硬编码模板）。
            assert!(
                prompt.contains("SymmetryAnalysis"),
                "the coder prompt must carry the capability from the spec"
            );
            return Ok(format!("```rust\n{}\n```", self.code));
        }
        Ok("unexpected agent".into())
    }

    async fn generate(&self, _prompt: &str) -> Result<String, sisyphus_types::TypesError> {
        Ok("Generic generation".into())
    }

    fn get_params_for(&self, _agent_id: &str) -> (String, String, String) {
        (
            "test_key".into(),
            "http://test:8080".into(),
            "test-model".into(),
        )
    }

    fn breaker(&self) -> CircuitBreaker {
        CircuitBreaker::default()
    }
}

/// 不执行任何测试命令的占位测试执行器（spec 未声明 test_command）。
struct NoopTestExecutor;

#[async_trait]
impl TestExecutor for NoopTestExecutor {
    async fn run_test(
        &self,
        _command: &str,
        _workdir: Option<&str>,
    ) -> Result<TestResult, TypesError> {
        Ok(TestResult {
            success: true,
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
            duration_ms: 0,
            error_summary: None,
        })
    }
}

/// 生成代码：std-only，入口签名符合契约。
const GENERATED_CODE: &str = r#"
pub fn run(input: &str) -> Result<String, String> {
    if input.is_empty() {
        return Err("empty input".to_string());
    }
    let score = 0.95_f64;
    Ok(format!(
        "{{\"status\":\"success\",\"symmetry_score\":{score},\"input_len\":{}}}",
        input.len()
    ))
}
"#;

#[tokio::test]
async fn test_self_evolution_e2e() {
    if !sisyphus_intel::wasm_compiler::wasm_target_installed().await {
        eprintln!("skipping e2e_self_evolution: wasm32-unknown-unknown is not installed");
        return;
    }

    // 1. LLM：生成代码 + 语义审计满分
    let llm_client = Arc::new(ProgrammableLlmClient {
        code: GENERATED_CODE.to_string(),
    });

    // 2. 持久化
    let store = SisyphusStore::new("sqlite::memory:", &sisyphus_types::Config::default())
        .await
        .expect("Store init failed");

    // 3. 真实的 WASM 执行器，工具目录指向临时目录
    let tools_dir = tempfile::tempdir().expect("tools dir");
    let wasm_executor = Arc::new(
        sisyphus_sandbox::WasmToolExecutor::with_config(Some(tools_dir.path().to_path_buf()), None)
            .expect("wasm executor"),
    );

    let execution_manager = Arc::new(ExecutionManager::new(
        store.state.clone(),
        store.sop.clone(),
        wasm_executor,
        Arc::new(NoopTestExecutor),
        Arc::new(sisyphus_core::intervention_manager::InterventionManager::new()),
        Arc::new(
            sisyphus_core::internalization_manager::InternalizationManager::new(
                llm_client.clone(),
                Arc::new(AssetManager::new(std::path::PathBuf::from("test_assets"))),
            ),
        ),
        Some(Arc::new(sisyphus_visual::VisualPerceptionManager::new(
            Arc::new(sisyphus_visual::SisyphusVisualProvider::new(
                llm_client.clone(),
            )),
            llm_client.clone(),
        ))),
    ));

    let asset_manager = Arc::new(AssetManager::new(std::path::PathBuf::from("test_assets")));

    let sp_manager = SelfProgrammingManager::new(
        llm_client.clone(),
        execution_manager.clone(),
        asset_manager.clone(),
        None,
    );
    // 编译产物必须落在沙箱读取的同一个目录，探针才能看到它。
    sp_manager.set_tools_dir(tools_dir.path());

    // 4. 工具缺口
    let gap = ToolGap {
        gap_id: "gap_123".into(),
        failing_node: "node_123".into(),
        required_capability: "SymmetryAnalysis".into(),
        suggested_tool_spec: ToolSpec {
            tool_id: "symmetry_analyzer".into(),
            required_capability: "SymmetryAnalysis".into(),
            expected_input: serde_json::json!({"object": "X"}),
            expected_output: serde_json::json!({"status": "string", "symmetry_score": "number"}),
            success_criteria: "Score between 0 and 1".into(),
            test_command: None,
            workdir: None,
        },
    };

    // 5. 事件流
    let (tx, mut rx) = mpsc::unbounded_channel::<CognitiveEvent>();
    sp_manager.set_event_sender(tx);

    // 6. 执行自编程循环
    let resolve_handle = tokio::spawn(async move { sp_manager.resolve_gap(gap).await });

    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        let done = matches!(event, CognitiveEvent::ToolDeployed { .. });
        events.push(event);
        if done {
            break;
        }
    }

    resolve_handle.await.expect("Join error").expect("Self-programming failed");

    assert!(
        events.iter().any(|e| matches!(e, CognitiveEvent::ToolGenerationStarted { .. })),
        "Should start generation"
    );
    assert!(
        events.iter().any(|e| matches!(e, CognitiveEvent::ToolVerified { .. })),
        "Should verify tool"
    );
    assert!(
        events.iter().any(|e| matches!(e, CognitiveEvent::ToolDeployed { .. })),
        "Should deploy tool"
    );

    // 7. 产物必须真实存在且能被沙箱再次调用
    let artifact = tools_dir.path().join("symmetry_analyzer.wasm");
    assert!(
        artifact.exists(),
        "compiled artifact missing at {}",
        artifact.display()
    );

    let bytes = std::fs::read(&artifact).expect("artifact readable");
    let info = sisyphus_intel::inspect_wasm(&bytes).expect("artifact satisfies the ABI");
    assert!(info.has_export("memory", sisyphus_intel::ExportKind::Memory));
    assert!(info.has_export("execute", sisyphus_intel::ExportKind::Func));

    let out = execution_manager
        .tool_router
        .default
        .execute(
            "symmetry_analyzer",
            br#"{"object":"X"}"#.to_vec(),
            1_000_000,
        )
        .await
        .expect("deployed tool must be callable");
    let text = String::from_utf8_lossy(&out);
    assert!(
        text.contains("symmetry_score"),
        "unexpected tool output: {text}"
    );

    println!("E2E Self-Evolution verified successfully!");
}
