//! 回归测试结果标记的正确性（`run_regression_tests`）。
//!
//! 回归背景：早期实现把 `unit_tests_passed` 写成
//! `failures.iter().any(|f| f.contains("Unit test"))`，语义正好相反——
//! 有失败才 `true`、全绿反而 `false`，并把错误状态灌进
//! `CognitiveEvent::QualityEvaluation` 与 TUI 仪表盘。
//!
//! 本文件用可控的假测试执行器锁定正确语义：
//! - 全绿 → 两个标记都为 true
//! - 单元失败 → unit=false, integration=true
//! - 集成失败 → unit=true, integration=false
//! - 失败信息文本里出现 "Unit test" 字样不得影响标记（防回退到嗅探）

use async_trait::async_trait;
use sisyphus_core::self_programming_manager::SelfProgrammingManager;
use sisyphus_types::tools::ToolSpec;
use sisyphus_types::{TestExecutor, TestResult, ToolExecutor, TypesError};
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

/// 按命令内容决定成败的假测试执行器。
///
/// - 命令含 `unit-fail` → 单元测试失败
/// - 命令含 `integration-fail` → 集成测试失败
/// - 其它 → 成功
struct ScriptedTestExecutor;

#[async_trait]
impl TestExecutor for ScriptedTestExecutor {
    async fn run_test(
        &self,
        command: &str,
        _workdir: Option<&str>,
    ) -> Result<TestResult, TypesError> {
        let is_integration = command.contains("sisyphus-tests");
        let should_fail = if is_integration {
            command.contains("integration-fail")
        } else {
            command.contains("unit-fail")
        };
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

async fn manager() -> SelfProgrammingManager {
    let state_store = Arc::new(
        sisyphus_mem::MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
            .await
            .expect("state store"),
    );
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.expect("pool");
    let sop_store = Arc::new(sisyphus_mem::SopSqlStore::new(pool).await.expect("sop store"));
    let exec_manager = Arc::new(sisyphus_core::ExecutionManager::new(
        state_store,
        sop_store,
        Arc::new(NoopToolExecutor),
        Arc::new(ScriptedTestExecutor),
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
    SelfProgrammingManager::new(
        Arc::new(sisyphus_tools_llm::metis::DummyLlmClient),
        exec_manager,
        Arc::new(sisyphus_knowledge::AssetManager::new(
            ".omo/test_regression_flags".into(),
        )),
        None,
    )
}

fn spec_with_unit_cmd(cmd: &str) -> ToolSpec {
    ToolSpec {
        tool_id: "flag_probe".into(),
        required_capability: "probe regression flags".into(),
        expected_input: serde_json::json!({}),
        expected_output: serde_json::json!({}),
        success_criteria: "flags are reported truthfully".into(),
        test_command: Some(cmd.to_string()),
        workdir: None,
    }
}

#[tokio::test]
async fn all_green_run_reports_both_passed() {
    let m = manager().await;
    let r = m.run_regression_tests(&spec_with_unit_cmd("cargo test unit-ok"), "asset").await;
    assert!(
        r.failures.is_empty(),
        "unexpected failures: {:?}",
        r.failures
    );
    assert!(
        r.unit_tests_passed,
        "all-green run must report unit tests passed"
    );
    assert!(
        r.integration_tests_passed,
        "all-green run must report integration tests passed"
    );
    assert_eq!(r.passed, r.total_tests);
    assert_eq!(r.failed, 0);
}

#[tokio::test]
async fn unit_failure_flips_only_the_unit_flag() {
    let m = manager().await;
    let r = m.run_regression_tests(&spec_with_unit_cmd("cargo test unit-fail"), "asset").await;
    assert!(!r.unit_tests_passed, "unit failure must be reported");
    assert!(
        r.integration_tests_passed,
        "integration suite still green must stay true"
    );
    assert_eq!(r.failed, 1);
    assert_eq!(r.failures.len(), 1);
}

#[tokio::test]
async fn integration_failure_flips_only_the_integration_flag() {
    // 集成命令由 tool_id 拼装（`--test {tool_id}_test`），
    // 因此把 tool_id 设为 integration-fail 即可让假执行器失败。
    let m = manager().await;
    let mut spec = spec_with_unit_cmd("cargo test unit-ok");
    spec.tool_id = "integration-fail".into();
    let r = m.run_regression_tests(&spec, "asset").await;
    assert!(
        !r.integration_tests_passed,
        "integration failure must be reported"
    );
    assert!(r.unit_tests_passed, "unit suite still green must stay true");
    assert_eq!(r.failed, 1);
}

#[tokio::test]
async fn failure_text_containing_markers_does_not_flip_green_flags() {
    // 假执行器在失败时把 "Unit test"/"Integration test" 塞进摘要；
    // 全绿路径下摘要为空，但我们要证明标记不是靠文本判定的：
    // 单元失败时文本含两种标记，integration 标记仍必须为 true。
    let m = manager().await;
    let r = m.run_regression_tests(&spec_with_unit_cmd("cargo test unit-fail"), "asset").await;
    let joined = r.failures.join("\n");
    assert!(joined.contains("Unit test"));
    assert!(
        r.integration_tests_passed,
        "text containing 'Integration test' must not flip the integration flag"
    );
}

#[tokio::test]
async fn failed_count_never_underflows() {
    let m = manager().await;
    let r = m.run_regression_tests(&spec_with_unit_cmd("cargo test unit-ok"), "asset").await;
    assert_eq!(r.failed, r.total_tests.saturating_sub(r.passed));
    assert!(r.passed <= r.total_tests);
}
