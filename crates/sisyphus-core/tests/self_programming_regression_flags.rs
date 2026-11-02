//! 回归测试结果标记的正确性（`run_regression_tests`）。
//!
//! 回归背景一：早期实现把 `unit_tests_passed` 写成
//! `failures.iter().any(|f| f.contains("Unit test"))`，语义正好相反——
//! 有失败才 `true`、全绿反而 `false`，并把错误状态灌进
//! `CognitiveEvent::QualityEvaluation` 与 TUI 仪表盘。
//!
//! 回归背景二：没有 `test_command` 时旧实现自己拼一个
//! `cargo test --test {tool_id}_test`，跑不通还记成通过（"soft pass"）。
//! 于是「没有测试」与「测试全绿」在质量报告里无法区分。
//!
//! 本文件用可控的假测试执行器锁定正确语义：
//! - 声明了命令且通过 → `harness_available` + `unit_tests_passed` 都为 true
//! - 声明了命令但失败 → `unit_tests_passed = false`
//! - 没声明命令 → `harness_available = false`，且**不执行任何命令**
//! - 失败信息文本里出现 "Unit test" 字样不得影响标记（防回退到嗅探）

use async_trait::async_trait;
use sisyphus_core::self_programming_manager::SelfProgrammingManager;
use sisyphus_types::tools::ToolSpec;
use sisyphus_types::{TestExecutor, TestResult, ToolExecutor, TypesError};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct NoopToolExecutor;

#[async_trait]
impl ToolExecutor for NoopToolExecutor {
    async fn execute(
        &self,
        _tool_id: &str,
        _args: Vec<u8>,
        _fuel_limit: u64,
    ) -> Result<Vec<u8>, TypesError> {
        Ok(Vec::new())
    }
}

/// 按命令内容决定成败的假测试执行器，并记录调用次数。
///
/// - 命令含 `fail` → 失败
/// - 其它 → 成功
struct ScriptedTestExecutor {
    calls: AtomicUsize,
}

#[async_trait]
impl TestExecutor for ScriptedTestExecutor {
    async fn run_test(
        &self,
        command: &str,
        _workdir: Option<&str>,
    ) -> Result<TestResult, TypesError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let should_fail = command.contains("fail");
        // 注意：失败摘要刻意包含 "Unit test"/"Integration test" 字样，
        // 用来证明标记不依赖文本嗅探。
        Ok(TestResult {
            success: !should_fail,
            stdout: String::new(),
            stderr: if should_fail {
                "boom".into()
            } else {
                String::new()
            },
            exit_code: if should_fail { 1 } else { 0 },
            duration_ms: 1,
            error_summary: if should_fail {
                Some("Unit test and Integration test markers in text".into())
            } else {
                None
            },
        })
    }
}

async fn manager() -> (SelfProgrammingManager, Arc<ScriptedTestExecutor>) {
    let state_store = Arc::new(
        sisyphus_mem::MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
            .await
            .expect("state store"),
    );
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.expect("pool");
    let sop_store = Arc::new(sisyphus_mem::SopSqlStore::new(pool).await.expect("sop store"));
    let test_executor = Arc::new(ScriptedTestExecutor {
        calls: AtomicUsize::new(0),
    });
    let exec_manager = Arc::new(sisyphus_core::ExecutionManager::new(
        state_store,
        sop_store,
        Arc::new(NoopToolExecutor),
        test_executor.clone(),
        Arc::new(sisyphus_core::intervention_manager::InterventionManager::new()),
        Arc::new(
            sisyphus_core::internalization_manager::InternalizationManager::new(
                Arc::new(sisyphus_tools_llm::metis::DummyLlmClient),
                Arc::new(sisyphus_knowledge::AssetManager::new(
                    ".omo/test_regression_flags".into(),
                )),
            ),
        ),
        None,
    ));
    let manager = SelfProgrammingManager::new(
        Arc::new(sisyphus_tools_llm::metis::DummyLlmClient),
        exec_manager,
        Arc::new(sisyphus_knowledge::AssetManager::new(
            ".omo/test_regression_flags".into(),
        )),
        None,
    );
    (manager, test_executor)
}

fn spec_with_cmd(cmd: Option<&str>) -> ToolSpec {
    ToolSpec {
        tool_id: "flag_probe".into(),
        required_capability: "probe regression flags".into(),
        expected_input: serde_json::json!({}),
        expected_output: serde_json::json!({}),
        success_criteria: "flags are reported truthfully".into(),
        test_command: cmd.map(str::to_string),
        workdir: None,
    }
}

#[tokio::test]
async fn green_command_reports_harness_and_pass() {
    let (m, _calls) = manager().await;
    let r = m.run_regression_tests(&spec_with_cmd(Some("cargo test ok")), "asset").await;

    assert!(
        r.harness_available,
        "a declared command must be reported as available"
    );
    assert!(
        r.failures.is_empty(),
        "unexpected failures: {:?}",
        r.failures
    );
    assert!(
        r.unit_tests_passed,
        "all-green run must report the suite passed"
    );
    assert_eq!(r.total_tests, 1);
    assert_eq!(r.passed, r.total_tests);
    assert_eq!(r.failed, 0);
}

#[tokio::test]
async fn failing_command_is_reported_truthfully() {
    let (m, _calls) = manager().await;
    let r = m.run_regression_tests(&spec_with_cmd(Some("cargo test fail")), "asset").await;

    assert!(r.harness_available);
    assert!(
        !r.unit_tests_passed,
        "a failing command must be reported as failed"
    );
    assert_eq!(r.failed, 1);
    assert_eq!(r.failures.len(), 1);
}

#[tokio::test]
async fn missing_command_runs_nothing_and_reports_no_harness() {
    let (m, calls) = manager().await;
    let r = m.run_regression_tests(&spec_with_cmd(None), "asset").await;

    assert!(!r.harness_available, "no declared command means no harness");
    assert!(
        !r.unit_tests_passed,
        "no tests is not the same as passing tests"
    );
    assert_eq!(r.total_tests, 0);
    assert_eq!(r.passed, 0);
    assert_eq!(r.failed, 0);
    assert_eq!(
        calls.calls.load(Ordering::SeqCst),
        0,
        "the runner must not invent a command when none is declared"
    );
    assert!(r.failures.iter().any(|f| f.contains("No test_command")));
}

#[tokio::test]
async fn empty_command_is_treated_as_missing() {
    let (m, calls) = manager().await;
    let r = m.run_regression_tests(&spec_with_cmd(Some("   ")), "asset").await;

    assert!(!r.harness_available);
    assert_eq!(calls.calls.load(Ordering::SeqCst), 0);
    assert!(r.failures.iter().any(|f| f.contains("empty")));
}

#[tokio::test]
async fn failure_text_containing_markers_does_not_flip_green_flags() {
    // 假执行器在失败时把 "Unit test"/"Integration test" 塞进摘要；
    // 我们要证明标记不是靠文本判定的。
    let (m, _calls) = manager().await;
    let r = m.run_regression_tests(&spec_with_cmd(Some("cargo test fail")), "asset").await;
    let joined = r.failures.join("\n");
    assert!(joined.contains("Unit test"));
    assert!(
        !r.unit_tests_passed,
        "the flag must come from the exit status, not the text"
    );
}

#[tokio::test]
async fn failed_count_never_underflows() {
    let (m, _calls) = manager().await;
    let r = m.run_regression_tests(&spec_with_cmd(Some("cargo test ok")), "asset").await;
    assert_eq!(r.failed, r.total_tests.saturating_sub(r.passed));
    assert!(r.passed <= r.total_tests);
}
