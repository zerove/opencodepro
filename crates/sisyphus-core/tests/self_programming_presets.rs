//! 集成测试：预置工具模板 (`PresetToolTemplates`) 与自编程质量门禁的联动。
//!
//! 覆盖：
//! - 预置模板可解析为完整 `ToolSpec`，且能直接构成 `ToolGap`
//! - 干净回归结果下，预置模板的加权质量分必须越过 0.6 的技能注册门禁
//! - 预置模板缺少成功标准时，质量分必须跌破门禁（防止“空规格”被注册）
//! - 预置模板的 `test_command` 非空（回归阶段有明确入口）

use async_trait::async_trait;
use sisyphus_core::self_programming_manager::{
    PresetToolTemplates, ProbeOutcome, RegressionTestResult, SelfProgrammingManager,
};
use sisyphus_intel::ToolGap;
use sisyphus_types::tools::ToolSpec;
use sisyphus_types::{TestExecutor, TestResult, ToolExecutor, TypesError};
use std::sync::Arc;

/// 不执行任何工具的占位执行器（本测试只验证质量评估，不真正跑工具）。
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

/// 不真正跑测试命令的占位测试执行器。
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

async fn build_manager() -> SelfProgrammingManager {
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
        Arc::new(NoopTestExecutor),
        Arc::new(sisyphus_core::intervention_manager::InterventionManager::new()),
        Arc::new(
            sisyphus_core::internalization_manager::InternalizationManager::new(
                Arc::new(sisyphus_tools_llm::metis::DummyLlmClient),
                Arc::new(sisyphus_knowledge::AssetManager::new(
                    ".omo/test_presets_knowledge".into(),
                )),
            ),
        ),
        None,
    ));
    SelfProgrammingManager::new(
        Arc::new(sisyphus_tools_llm::metis::DummyLlmClient),
        exec_manager,
        Arc::new(sisyphus_knowledge::AssetManager::new(
            ".omo/test_presets_knowledge".into(),
        )),
        None,
    )
}

fn clean_regression(tool_id: &str) -> RegressionTestResult {
    RegressionTestResult {
        tool_id: tool_id.to_string(),
        harness_available: true,
        unit_tests_passed: true,
        total_tests: 2,
        passed: 2,
        failed: 0,
        duration_ms: 10,
        failures: vec![],
    }
}

/// 沙箱探针通过的占位结论（本文件只验证质量评估，不真的跑工具）。
fn probe_passed() -> ProbeOutcome {
    ProbeOutcome::Passed {
        output_len: 64,
        fuel_consumed: 1_200,
    }
}

#[test]
fn every_preset_has_a_test_command() {
    for spec in PresetToolTemplates::all() {
        let cmd = spec.test_command.as_deref().unwrap_or("");
        assert!(
            !cmd.trim().is_empty(),
            "{} must declare a test_command",
            spec.tool_id
        );
    }
}

#[test]
fn preset_can_be_wrapped_into_a_tool_gap() {
    let spec = PresetToolTemplates::get_preset("local/log_analyzer").expect("preset");
    let gap = ToolGap {
        gap_id: "preset_gap".into(),
        required_capability: spec.required_capability.clone(),
        failing_node: "analyze_logs".into(),
        suggested_tool_spec: spec.clone(),
    };
    assert_eq!(gap.suggested_tool_spec.tool_id, "local/log_analyzer");
    assert_eq!(gap.required_capability, spec.required_capability);
}

#[tokio::test]
async fn preset_passes_quality_gate_with_clean_regression() {
    let manager = build_manager().await;
    for spec in PresetToolTemplates::all() {
        let regression = clean_regression(&spec.tool_id);
        let report = manager.evaluate_quality(&spec, &regression, &probe_passed()).await;
        assert!(
            report.overall_score >= 0.6,
            "{} scored {:.3}, below the 0.6 skill-registration gate",
            spec.tool_id,
            report.overall_score
        );
        assert!(report.unit_tests_passed);
        assert!(
            report.integration_tests_passed,
            "the probe passed, so integration must be true"
        );
        assert!(
            report.blockers.is_empty(),
            "unexpected blockers: {:?}",
            report.blockers
        );
        assert!(
            report.recommendations.is_empty(),
            "{} should need no remediation: {:?}",
            spec.tool_id,
            report.recommendations
        );
    }
}

#[tokio::test]
async fn preset_without_success_criteria_falls_below_gate() {
    let manager = build_manager().await;
    let spec = PresetToolTemplates::get_preset("local/json_formatter").expect("preset");
    let hollow = ToolSpec {
        success_criteria: String::new(),
        ..spec
    };
    let report = manager
        .evaluate_quality(&hollow, &clean_regression(&hollow.tool_id), &probe_passed())
        .await;
    // 缺少成功标准的工具无法被判定“是否正确”，即使测试全绿也不得自动注册。
    assert!(
        !report.meets_registration_bar(),
        "hollow spec must not clear the registration bar (overall {:.3}, spec {:.2})",
        report.overall_score,
        report.spec_completeness()
    );
    assert!(report.rejection_reason().is_some_and(|r| r.contains("spec completeness")));
    assert!(report.recommendations.iter().any(|r| r.contains("success criteria")));
}

#[tokio::test]
async fn preset_with_clean_regression_clears_registration_bar() {
    let manager = build_manager().await;
    for spec in PresetToolTemplates::all() {
        let report = manager
            .evaluate_quality(&spec, &clean_regression(&spec.tool_id), &probe_passed())
            .await;
        assert!(
            report.meets_registration_bar(),
            "{} must clear the bar: overall {:.3}, spec {:.2}",
            spec.tool_id,
            report.overall_score,
            report.spec_completeness()
        );
        assert_eq!(report.rejection_reason(), None);
    }
}

#[tokio::test]
async fn failing_regression_lowers_preset_quality() {
    let manager = build_manager().await;
    let spec = PresetToolTemplates::get_preset("local/log_analyzer").expect("preset");
    let failing = RegressionTestResult {
        tool_id: spec.tool_id.clone(),
        harness_available: true,
        unit_tests_passed: false,
        total_tests: 2,
        passed: 0,
        failed: 2,
        duration_ms: 10,
        failures: vec!["unit failed".into(), "integration failed".into()],
    };
    let report = manager.evaluate_quality(&spec, &failing, &probe_passed()).await;
    // 探针能通过，但声明的测试全红 → 硬失败，不得自动注册。
    assert!(
        !report.meets_registration_bar(),
        "score {:.3}",
        report.overall_score
    );
    assert!(!report.unit_tests_passed);
    assert!(
        !report.blockers.is_empty(),
        "failing tests must be a blocker"
    );
    assert!(report.recommendations.iter().any(|r| r.contains("Fix failing tests")));
}

#[test]
fn execution_event_import_is_used() {
    // 保持 import 与 workspace 类型编译一致性（无行为断言）。
    let _ = std::mem::size_of::<sisyphus_types::ExecutionEvent>();
}
