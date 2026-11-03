//! 自编程闭环的**重试/反馈回灌**路径（真实编译 + 真实沙箱探针）。
//!
//! 与 `e2e_self_evolution.rs`（第一次生成就通过）互补：这里第一轮故意返回
//! 语法错误的 Rust，验证 [`SelfProgrammingManager::resolve_gap`] 会：
//!
//! 1. 用真实 rustc 编译失败并捕获 stderr；
//! 2. 把失败原因作为 feedback 回灌给 coder（提示词出现
//!    `PREVIOUS ATTEMPT FAILED` + `failed to compile`）；
//! 3. 第二轮生成合法代码后编译、静态/语义校验、沙箱探针全部通过并部署。
//!
//! 该测试取代了原先 `sisyphus-core/tests/self_programming_refinement_test.rs`
//! 中挂在 `broken_tests` feature 后面、已无法编译的旧版本（其 mock 返回
//! `fn main()`，与当前 `pub fn run` 契约不符，且从未真正调用 rustc）。
//!
//! 如果 rustc 的 wasm 目标没装，测试会跳过并说明原因——那是环境缺失，
//! 不是代码缺陷（与 e2e 测试一致）。

use async_trait::async_trait;
use sisyphus_core::execution_manager::ExecutionManager;
use sisyphus_core::SelfProgrammingManager;
use sisyphus_intel::ToolGap;
use sisyphus_knowledge::AssetManager;
use sisyphus_mem::SisyphusStore;
use sisyphus_tools_llm::{circuit_breaker::CircuitBreaker, LlmClient};
use sisyphus_types::errors::TypesError;
use sisyphus_types::node::TestExecutor;
use sisyphus_types::tools::ToolSpec;
use sisyphus_types::TestResult;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// 第一轮返回语法错误的代码，之后返回合法实现；语义审计一律满分。
struct RetryLlmClient {
    coder_calls: AtomicUsize,
    /// 每次 coder 调用的 user prompt（含被回灌的 feedback）。
    coder_prompts: Mutex<Vec<String>>,
}

#[async_trait]
impl LlmClient for RetryLlmClient {
    async fn chat(
        &self,
        system: &str,
        prompt: &str,
        _json_mode: bool,
    ) -> Result<String, TypesError> {
        if system.contains("Tool Auditor") {
            return Ok("0.95".into());
        }
        if system.contains("Rust Engineer") {
            let call = self.coder_calls.fetch_add(1, Ordering::SeqCst);
            self.coder_prompts.lock().expect("coder_prompts mutex").push(prompt.to_string());
            if call == 0 {
                // 缺分号：rustc 必报语法错误，用于验证 stderr 回灌。
                return Ok("```rust\n\
                     pub fn run(input: &str) -> Result<String, String> {\n\
                         let len = input.len()\n\
                         Ok(format!(\"{{\\\"len\\\":{len}}}\"))\n\
                     }\n\
                     ```"
                .into());
            }
            return Ok(format!("```rust\n{GENERATED_CODE}\n```"));
        }
        Ok("unexpected agent".into())
    }

    async fn generate(&self, _prompt: &str) -> Result<String, TypesError> {
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

/// 第二轮生成的合法工具：std-only，入口符合 `pub fn run(input: &str)` 契约。
const GENERATED_CODE: &str = r#"
pub fn run(input: &str) -> Result<String, String> {
    let len = input.len();
    Ok(format!("{{\"len\":{len}}}"))
}
"#;

/// spec 未声明 `test_command`，回归阶段用不到真实命令执行。
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

#[tokio::test]
async fn test_self_programming_retries_after_compile_failure() {
    if !sisyphus_intel::wasm_compiler::wasm_target_installed().await {
        eprintln!("skipping self_prog_retry: wasm32-unknown-unknown is not installed");
        return;
    }

    let llm_client = Arc::new(RetryLlmClient {
        coder_calls: AtomicUsize::new(0),
        coder_prompts: Mutex::new(Vec::new()),
    });

    let store = SisyphusStore::new("sqlite::memory:", &sisyphus_types::Config::default())
        .await
        .expect("Store init failed");

    // 编译产物与沙箱读取必须是同一个目录，探针才能看到本轮产物。
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

    let sp_manager = SelfProgrammingManager::new(
        llm_client.clone(),
        execution_manager.clone(),
        Arc::new(AssetManager::new(std::path::PathBuf::from("test_assets"))),
        None,
    );
    sp_manager.set_tools_dir(tools_dir.path());

    let gap = ToolGap {
        gap_id: "gap_retry".into(),
        failing_node: "node_retry".into(),
        required_capability: "RetryAnalysis".into(),
        suggested_tool_spec: ToolSpec {
            tool_id: "retry_probe_tool".into(),
            required_capability: "RetryAnalysis".into(),
            expected_input: serde_json::json!({"object": "X"}),
            expected_output: serde_json::json!({"status": "string", "symmetry_score": "number"}),
            success_criteria: "Score between 0 and 1".into(),
            test_command: None,
            workdir: None,
        },
    };

    let result = sp_manager.resolve_gap(gap).await;
    assert!(
        result.is_ok(),
        "retry loop must recover after the first compile failure: {result:?}"
    );

    // 1) 生成恰好两次（首次编译失败 → 反馈回灌 → 修复）。
    let calls = llm_client.coder_calls.load(Ordering::SeqCst);
    assert_eq!(
        calls, 2,
        "coder must be called twice (broken attempt + fixed attempt)"
    );

    // 2) 第二次提示词必须携带上一轮的 rustc stderr（证明反馈真的回灌了，
    //    而不是重试时把同样的提示词再发一遍）。
    // 克隆出来再断言：避免 MutexGuard 跨越后面的 await 点。
    let prompts: Vec<String> =
        llm_client.coder_prompts.lock().expect("coder_prompts mutex").clone();
    assert_eq!(prompts.len(), 2, "one captured prompt per coder call");
    assert!(
        prompts[0].contains("SPECIFICATION"),
        "first prompt should be the plain generation request"
    );
    assert!(
        !prompts[0].contains("PREVIOUS ATTEMPT FAILED"),
        "first attempt must not carry retry feedback"
    );
    assert!(
        prompts[1].contains("PREVIOUS ATTEMPT FAILED"),
        "second prompt must mark the previous attempt as failed"
    );
    assert!(
        prompts[1].contains("failed to compile"),
        "second prompt must carry the rustc failure reason, got: {}",
        prompts[1]
    );
    drop(prompts);

    // 3) 修复后的产物真实落盘且能被沙箱调用。
    let artifact = tools_dir.path().join("retry_probe_tool.wasm");
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
        .execute("retry_probe_tool", br#"{"object":"X"}"#.to_vec(), 1_000_000)
        .await
        .expect("deployed tool must be callable");
    let text = String::from_utf8_lossy(&out);
    assert!(text.contains("\"len\""), "unexpected tool output: {text}");
}
