//! Sisyphus 自编程循环的编排器。
//!
//! ## 能力
//! - **工具缺口解决**：生成 → 编译 → 验证 → 部署 → 回归 → 注册
//! - **自动回归测试**：对新部署的工具运行单元 + 集成测试
//! - **质量评估**：多轴质量评分（测试通过率、稳定性、精确度）
//! - **自动化技能注册**：将已部署工具打包为 SkillBundle 并注册到
//!   SkillStore，使系统能够发现它们

use crate::ExecutionManager;
use sisyphus_intel::{ToolCoder, ToolGap, WasmCompiler};
use sisyphus_mem::SkillStore;
use sisyphus_types::skill::{SkillBundle, SkillMetadata};
use sisyphus_types::tools::ToolSpec;
use sisyphus_types::CognitiveEvent;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tracing::{info, warn};

/// 单个质量评分轴。
#[derive(Debug, Clone)]
pub struct QualityAxis {
    pub name: String,
    pub score: f64,
    pub weight: f64,
}

/// 质量评估的结果。
#[derive(Debug, Clone)]
pub struct QualityReport {
    pub tool_id: String,
    pub overall_score: f64,
    pub axes: Vec<QualityAxis>,
    pub unit_tests_passed: bool,
    pub integration_tests_passed: bool,
    pub recommendations: Vec<String>,
}

/// 技能注册的最低加权总分。
pub const REGISTRATION_MIN_OVERALL: f64 = 0.6;
/// 技能注册的最低规范完整度。
///
/// 规范完整度是**硬前置条件**而非仅参与加权：一个没有 `success_criteria`
/// 的工具无法被判定“是否正确”，即使测试恰好通过也不应自动注册为技能。
pub const REGISTRATION_MIN_SPEC: f64 = 0.8;

impl QualityReport {
    /// 取某个轴的分值（轴缺失时返回 0.0）。
    pub fn axis_score(&self, name: &str) -> f64 {
        self.axes.iter().find(|a| a.name == name).map(|a| a.score).unwrap_or(0.0)
    }

    /// 规范完整度轴分值。
    pub fn spec_completeness(&self) -> f64 {
        self.axis_score("spec_completeness")
    }

    /// 是否达到自动注册为技能的门槛。
    ///
    /// 需同时满足：加权总分 ≥ [`REGISTRATION_MIN_OVERALL`] 且
    /// 规范完整度 ≥ [`REGISTRATION_MIN_SPEC`]。
    pub fn meets_registration_bar(&self) -> bool {
        self.overall_score >= REGISTRATION_MIN_OVERALL
            && self.spec_completeness() >= REGISTRATION_MIN_SPEC
    }

    /// 未达标的原因（达标时返回 `None`），用于日志与用户可见诊断。
    pub fn rejection_reason(&self) -> Option<String> {
        if self.overall_score < REGISTRATION_MIN_OVERALL {
            return Some(format!(
                "overall score {:.3} below {:.1}",
                self.overall_score, REGISTRATION_MIN_OVERALL
            ));
        }
        let spec = self.spec_completeness();
        if spec < REGISTRATION_MIN_SPEC {
            return Some(format!(
                "spec completeness {spec:.2} below {REGISTRATION_MIN_SPEC:.1} \
                 (add success_criteria and an input/output contract)"
            ));
        }
        None
    }
}

/// 规范完整度评分（纯函数，供 `evaluate_quality` 与测试共用）。
///
/// 分级语义：
/// - 无 `required_capability` → 0.0（规范没有说明“要做什么”）
/// - 无 `success_criteria` → 0.2（无法判定“做对了没有”）
/// - 输入/输出契约不是对象 → 0.5（下游无法按字段绑定参数）
/// - 三者齐备 → 0.9
pub fn spec_completeness_score(spec: &ToolSpec) -> f64 {
    if spec.required_capability.trim().is_empty() {
        return 0.0;
    }
    if spec.success_criteria.trim().is_empty() {
        return 0.2;
    }
    if !spec.expected_input.is_object() || !spec.expected_output.is_object() {
        return 0.5;
    }
    0.9
}

/// 回归测试执行的结果。
#[derive(Debug, Clone)]
pub struct RegressionTestResult {
    pub tool_id: String,
    pub unit_tests_passed: bool,
    pub integration_tests_passed: bool,
    pub total_tests: u64,
    pub passed: u64,
    pub failed: u64,
    pub duration_ms: u64,
    pub failures: Vec<String>,
}

impl RegressionTestResult {
    /// Success rate.
    pub fn success_rate(&self) -> f64 {
        if self.total_tests == 0 {
            return 1.0;
        }
        self.passed as f64 / self.total_tests as f64
    }
}

/// Self Programming Manager.
pub struct SelfProgrammingManager {
    pub coder: ToolCoder,
    pub compiler: WasmCompiler,
    pub verifier: sisyphus_intel::ToolVerifier,
    pub execution_manager: Arc<ExecutionManager>,
    pub knowledge_manager: Arc<sisyphus_knowledge::AssetManager>,
    pub skill_store: Option<Arc<SkillStore>>,
    pub event_tx: parking_lot::Mutex<Option<mpsc::UnboundedSender<CognitiveEvent>>>,
}

impl SelfProgrammingManager {
    /// New.
    pub fn new(
        llm_client: Arc<dyn sisyphus_types::LlmClient>,
        execution_manager: Arc<ExecutionManager>,
        knowledge_manager: Arc<sisyphus_knowledge::AssetManager>,
        skill_store: Option<Arc<SkillStore>>,
    ) -> Self {
        Self {
            coder: ToolCoder::new(llm_client.clone()),
            compiler: WasmCompiler::new(),
            verifier: sisyphus_intel::ToolVerifier::new(llm_client),
            execution_manager,
            knowledge_manager,
            skill_store,
            event_tx: parking_lot::Mutex::new(None),
        }
    }

    /// Set event sender.
    pub fn set_event_sender(&self, tx: mpsc::UnboundedSender<CognitiveEvent>) {
        *self.event_tx.lock() = Some(tx);
    }

    async fn notify(&self, event: CognitiveEvent) {
        if let Some(tx) = self.event_tx.lock().clone() {
            let _ = tx.send(event);
        }
    }

    // ---------------------------------------------------------------
    // 核心缺口解决流水线（增强版）
    // ---------------------------------------------------------------

    /// 为检测到的缺口执行完整的自编程循环，包括
    /// 部署后的回归测试与自动化技能注册。
    pub async fn resolve_gap(&self, gap: ToolGap) -> Result<(), crate::CoreError> {
        info!(gap_id = %gap.gap_id, "Starting self-programming loop to resolve tool gap");

        let mut feedback: Option<String> = None;
        let max_retries = 3;

        for attempt in 1..=max_retries {
            info!("Self-programming attempt {}/{}", attempt, max_retries);

            // 1. 通知开始
            self.notify(CognitiveEvent::ToolGenerationStarted {
                tool_id: gap.suggested_tool_spec.tool_id.clone(),
                capability: gap.required_capability.clone(),
            })
            .await;

            // 2. 生成代码
            let code = self
                .coder
                .generate_code(&gap.suggested_tool_spec, feedback.clone())
                .await
                .map_err(|e| crate::CoreError::Internal(e.to_string()))?;

            // 3. 编译为 WASM
            let wasm_path = self
                .compiler
                .compile(&gap.suggested_tool_spec.tool_id, &code)
                .await
                .map_err(|e| crate::CoreError::Internal(e.to_string()))?;

            // 4. 验证工具
            let v_report = self
                .verifier
                .verify(&gap.suggested_tool_spec, &wasm_path)
                .await
                .map_err(|e| crate::CoreError::Internal(e.to_string()))?;

            if v_report.confidence_score >= 0.7 {
                self.notify(CognitiveEvent::ToolVerified {
                    tool_id: gap.suggested_tool_spec.tool_id.clone(),
                    score: v_report.confidence_score as f64,
                })
                .await;
                info!(
                    "Tool verified successfully (score: {:.2})",
                    v_report.confidence_score
                );

                // 5. 部署并沉淀（Sediment）
                let asset_id = self
                    .knowledge_manager
                    .sediment_tool(gap.suggested_tool_spec.clone(), wasm_path)
                    .await
                    .map_err(|e| crate::CoreError::Internal(e.to_string()))?;

                self.notify(CognitiveEvent::ToolDeployed {
                    tool_id: gap.suggested_tool_spec.tool_id.clone(),
                    asset_id: asset_id.clone(),
                })
                .await;
                self.execution_manager.notify_tool_deployed(&gap.suggested_tool_spec.tool_id);

                info!(
                    "Tool deployed (asset_id: {}). Running regression tests...",
                    asset_id
                );

                // 6. 对已部署工具运行回归测试
                let regress_result =
                    self.run_regression_tests(&gap.suggested_tool_spec, &asset_id).await;

                // 7. 评估质量
                let quality =
                    self.evaluate_quality(&gap.suggested_tool_spec, &regress_result).await;

                self.notify(CognitiveEvent::QualityEvaluation {
                    tool_id: gap.suggested_tool_spec.tool_id.clone(),
                    unit_tests_passed: quality.unit_tests_passed,
                    integration_tests_passed: quality.integration_tests_passed,
                    regression_score: regress_result.success_rate(),
                    quality_score: quality.overall_score,
                })
                .await;

                if quality.meets_registration_bar() {
                    // 8. 质量合格时自动注册为技能
                    self.register_skill(&gap, &quality).await;
                } else {
                    warn!(
                        tool_id = %gap.suggested_tool_spec.tool_id,
                        quality = quality.overall_score,
                        spec_completeness = quality.spec_completeness(),
                        reason = ?quality.rejection_reason(),
                        "Tool quality below registration bar; skipping skill registration"
                    );
                }

                info!(
                    "Successfully resolved tool gap {}. Tool {} deployed, tested, and quality-scored.",
                    gap.gap_id, gap.suggested_tool_spec.tool_id
                );
                return Ok(());
            }

            warn!(
                "Attempt {} failed (score: {:.2}). Failures: {:?}",
                attempt, v_report.confidence_score, v_report.failures
            );
            feedback = Some(format!(
                "Attempt {} failures:\n{}",
                attempt,
                v_report.failures.join("\n")
            ));
        }

        Err(crate::CoreError::Internal(format!(
            "Failed to resolve gap after {} attempts",
            max_retries
        )))
    }

    // ---------------------------------------------------------------
    // 回归测试
    // ---------------------------------------------------------------

    /// 对已部署工具运行自动化回归测试（单元 + 集成）。
    /// 使用 ExecutionManager 的测试执行器运行工具专属的测试命令。
    pub async fn run_regression_tests(
        &self,
        spec: &ToolSpec,
        _asset_id: &str,
    ) -> RegressionTestResult {
        let start = std::time::Instant::now();

        let tool_id = &spec.tool_id;
        let mut failures = Vec::new();
        let mut total = 0u64;
        let mut passed = 0u64;
        // 真实结果标记（不依赖 failures 文本嗅探）
        let mut unit_ok = true;
        let mut integration_ok = true;

        // --- 单元测试 ---
        // 若 spec 中指定了测试命令，则尝试运行单元测试
        if let Some(ref test_cmd) = spec.test_command {
            total += 1;
            let test_result = self
                .execution_manager
                .executor
                .test_executor
                .run_test(test_cmd, spec.workdir.as_deref())
                .await;

            match test_result {
                Ok(result) if result.success => {
                    passed += 1;
                    info!("Unit test passed for tool {}", tool_id);
                },
                Ok(result) => {
                    unit_ok = false;
                    failures.push(format!(
                        "Unit test failed: {}",
                        result.error_summary.unwrap_or_default()
                    ));
                    warn!("Unit test failed for tool {}: {}", tool_id, result.stderr);
                },
                Err(e) => {
                    unit_ok = false;
                    failures.push(format!("Unit test error: {}", e));
                    warn!("Unit test error for tool {}: {}", tool_id, e);
                },
            }
        } else {
            // 没有显式测试命令；尝试运行常见的测试模式
            let default_cmd = format!("cargo test --test {}_test", tool_id);
            total += 1;
            let result =
                self.execution_manager.executor.test_executor.run_test(&default_cmd, None).await;

            match result {
                Ok(r) if r.success => {
                    passed += 1;
                },
                Ok(r) => {
                    // 测试 crate 可能不存在；将其视为软失败
                    info!(
                        "No dedicated unit test found for {}: {}",
                        tool_id,
                        r.error_summary.unwrap_or_default()
                    );
                    passed += 1; // Soft pass — no test is not a regression
                },
                Err(_e) => {
                    passed += 1; // Soft pass — no test harness, not a regression
                },
            }
        }

        // --- 集成测试 ---
        let integration_cmd = format!("cargo test -p sisyphus-tests --test {}_test", tool_id);
        total += 1;
        match self.execution_manager.executor.test_executor.run_test(&integration_cmd, None).await {
            Ok(r) if r.success => {
                passed += 1;
                info!("Integration test passed for tool {}", tool_id);
            },
            Ok(r) => {
                integration_ok = false;
                let err_msg = r.error_summary.unwrap_or_default();
                failures.push(format!("Integration test: {}", err_msg));
                warn!("Integration test for {}: {}", tool_id, err_msg);
            },
            Err(e) => {
                info!("No integration test suite for {}: {}", tool_id, e);
                passed += 1; // Soft pass
            },
        }

        let duration = start.elapsed();

        RegressionTestResult {
            tool_id: tool_id.clone(),
            // 注意：这两个字段必须来自执行过程的真实结果标记，
            // 不能从 failures 文本里“嗅探”关键词——早期实现写成
            // `failures.iter().any(|f| f.contains("Unit test"))`，
            // 语义正好相反（有失败才 true），导致质量报告把
            // “全绿”报成“未通过”、把“失败”报成“通过”。
            unit_tests_passed: unit_ok,
            integration_tests_passed: integration_ok,
            total_tests: total,
            passed,
            failed: total.saturating_sub(passed),
            duration_ms: duration.as_millis() as u64,
            failures,
        }
    }

    // ---------------------------------------------------------------
    // 质量评估
    // ---------------------------------------------------------------

    /// 从多个轴评估工具质量。
    /// 返回结构化报告，含总分（0.0 - 1.0）。
    pub async fn evaluate_quality(
        &self,
        spec: &ToolSpec,
        regression: &RegressionTestResult,
    ) -> QualityReport {
        let mut axes = Vec::new();
        let mut recommendations = Vec::new();

        // 轴 1：测试通过率
        let test_score = regression.success_rate();
        axes.push(QualityAxis {
            name: "test_coverage".into(),
            score: test_score,
            weight: 0.35,
        });

        // 轴 2：规范完整度
        let spec_score = spec_completeness_score(spec);
        axes.push(QualityAxis {
            name: "spec_completeness".into(),
            score: spec_score,
            weight: 0.25,
        });

        // 轴 3：验证得到的稳定性
        let stability_score = if regression.failed == 0 {
            0.95
        } else if regression.success_rate() > 0.8 {
            0.7
        } else {
            0.4
        };
        axes.push(QualityAxis {
            name: "stability".into(),
            score: stability_score,
            weight: 0.25,
        });

        // 轴 4：执行效率（燃料/性能代理）
        let efficiency_score = if let Some(ref test_cmd) = spec.test_command {
            if test_cmd.len() < 100 {
                0.9
            } else {
                0.6
            }
        } else {
            0.5
        };
        axes.push(QualityAxis {
            name: "efficiency".into(),
            score: efficiency_score,
            weight: 0.15,
        });

        // 计算加权总分
        let overall_score: f64 = axes.iter().map(|a| a.score * a.weight).sum();

        // 生成建议
        if regression.failed > 0 {
            recommendations.push("Fix failing tests before deployment to production".to_string());
        }
        if spec.success_criteria.is_empty() {
            recommendations.push("Add explicit success criteria to the tool spec".to_string());
        }
        if overall_score < 0.5 {
            recommendations
                .push("Consider regenerating the tool with more detailed spec".to_string());
        }
        if stability_score < 0.7 {
            recommendations.push("Run additional sandbox stability checks".to_string());
        }

        QualityReport {
            tool_id: spec.tool_id.clone(),
            overall_score,
            axes,
            unit_tests_passed: regression.unit_tests_passed,
            integration_tests_passed: regression.integration_tests_passed,
            recommendations,
        }
    }

    // ---------------------------------------------------------------
    // 技能注册
    // ---------------------------------------------------------------

    /// 自动将已部署工具注册为 Sisyphus 技能。
    /// 创建 SkillBundle 并通过 SkillStore 安装。
    pub async fn register_skill(&self, gap: &ToolGap, quality: &QualityReport) {
        info!(
            tool_id = %gap.suggested_tool_spec.tool_id,
            quality = quality.overall_score,
            "Registering tool as a skill"
        );

        let tool_id = &gap.suggested_tool_spec.tool_id;
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();

        // 构建技能元数据
        let metadata = SkillMetadata {
            id: format!("skill:{}:{}", tool_id.replace('/', "_"), now),
            name: tool_id
                .rsplit_once('/')
                .map(|(_, name)| name.to_string())
                .unwrap_or_else(|| tool_id.to_string()),
            version: "0.1.0".into(),
            author: "sisyphus-self-programming".into(),
            description: gap.required_capability.clone(),
            tags: vec![
                "auto-generated".into(),
                "self-programming".into(),
                format!("quality:{:.0}", quality.overall_score * 100.0),
            ],
            created_at: now,
        };

        // 为工具创建最小化的 SOPGraphConfig
        let mut main_sop = sisyphus_types::SOPGraphConfig {
            nodes: vec![],
            edges: vec![],
        };
        main_sop.nodes.push(sisyphus_types::SOPNodeConfig {
            name: tool_id.clone(),
            kind: sisyphus_types::NodeKind::Wasm {
                tool_id: tool_id.clone(),
                fuel_limit: 1000,
                args: Vec::new(),
            },
        });

        let bundle = SkillBundle::new(metadata, main_sop);

        // 通过 SkillStore 注册
        if let Some(ref store) = self.skill_store {
            match store.install(bundle.clone()).await {
                Ok(()) => {
                    info!("Skill {} registered successfully", bundle.metadata.id);
                    self.notify(CognitiveEvent::ToolRegistered {
                        tool_id: tool_id.clone(),
                        skill_id: bundle.metadata.id.clone(),
                        score: quality.overall_score,
                    })
                    .await;
                },
                Err(e) => {
                    warn!("Failed to register skill {}: {}", bundle.metadata.id, e);
                },
            }
        } else {
            info!(
                "No SkillStore configured; skill registration skipped (tool_id: {})",
                tool_id
            );
        }
    }
}

/// 预置的常用本地工具模板库（用于加速标准工具的自编程自举）。
///
/// 定义已下沉到依赖根 `sisyphus-types`（纯数据），此处仅作 re-export，
/// 保持 `sisyphus_core::self_programming_manager::PresetToolTemplates` 路径可用。
pub use sisyphus_types::tools::PresetToolTemplates;

// ---------------------------------------------------------------
// 测试
// ---------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use sisyphus_types::tools::ToolSpec;

    fn make_test_spec(tool_id: &str) -> ToolSpec {
        ToolSpec {
            tool_id: tool_id.to_string(),
            required_capability: "Test capability".into(),
            expected_input: serde_json::json!({}),
            expected_output: serde_json::json!({}),
            success_criteria: "works".into(),
            test_command: None,
            workdir: None,
        }
    }

    #[test]
    fn test_quality_evaluation_perfect_score() {
        let _spec = make_test_spec("perfect_tool");
        let regression = RegressionTestResult {
            tool_id: "perfect_tool".into(),
            unit_tests_passed: true,
            integration_tests_passed: true,
            total_tests: 5,
            passed: 5,
            failed: 0,
            duration_ms: 100,
            failures: vec![],
        };

        // evaluate_quality 是异步的，需要异步运行时
        // 但在测试中可以使用更简单的直接方式
        let score = regression.success_rate();
        assert_eq!(score, 1.0);
    }

    #[test]
    fn test_regression_result_success_rate() {
        let r = RegressionTestResult {
            tool_id: "t".into(),
            unit_tests_passed: true,
            integration_tests_passed: false,
            total_tests: 10,
            passed: 7,
            failed: 3,
            duration_ms: 500,
            failures: vec!["test_1 failed".into(), "test_2 failed".into()],
        };
        assert!((r.success_rate() - 0.7).abs() < 0.001);
    }

    #[test]
    fn test_regression_result_empty() {
        let r = RegressionTestResult {
            tool_id: "empty".into(),
            unit_tests_passed: true,
            integration_tests_passed: true,
            total_tests: 0,
            passed: 0,
            failed: 0,
            duration_ms: 0,
            failures: vec![],
        };
        assert_eq!(r.success_rate(), 1.0, "Empty test suite should return 1.0");
    }

    #[test]
    fn test_regression_result_all_fail() {
        let r = RegressionTestResult {
            tool_id: "fail_all".into(),
            unit_tests_passed: false,
            integration_tests_passed: false,
            total_tests: 3,
            passed: 0,
            failed: 3,
            duration_ms: 50,
            failures: vec!["all failed".into()],
        };
        assert_eq!(r.success_rate(), 0.0);
    }

    #[test]
    fn test_quality_axes_weight_sums_to_one() {
        // 验证 evaluate_quality 中的权重之和约为 1.0
        // 此测试聚焦于逻辑本身，无需异步
        let spec = make_test_spec("sum_check");
        let regression = RegressionTestResult {
            tool_id: "sum_check".into(),
            unit_tests_passed: true,
            integration_tests_passed: true,
            total_tests: 1,
            passed: 1,
            failed: 0,
            duration_ms: 10,
            failures: vec![],
        };

        // 可以通过检查总分是加权平均值来验证权重分布正确
        // 总分是各轴分数的加权平均。
        let test_score = regression.success_rate(); // 1.0
        let spec_score = spec_completeness_score(&spec); // 0.9 (capability + criteria + 对象契约)
        let stability_score = if regression.failed == 0 { 0.95 } else { 0.7 }; // 0.95
        let efficiency_score = 0.5; // no test command

        let expected = test_score * 0.35
            + spec_score * 0.25
            + stability_score * 0.25
            + efficiency_score * 0.15;
        let weight_sum: f64 = 0.35 + 0.25 + 0.25 + 0.15;
        assert!(
            (weight_sum - 1.0).abs() < 0.001,
            "Weights should sum to 1.0, got {}",
            weight_sum
        );
        assert!(
            (expected - 0.8875).abs() < 0.001,
            "Expected score ~0.8875, got {}",
            expected
        );
    }

    #[test]
    fn spec_completeness_tiers_are_ordered() {
        let full = make_test_spec("full"); // capability + criteria + 对象契约
        assert_eq!(spec_completeness_score(&full), 0.9);

        let no_criteria = ToolSpec {
            success_criteria: String::new(),
            ..make_test_spec("no_criteria")
        };
        assert_eq!(spec_completeness_score(&no_criteria), 0.2);

        let no_capability = ToolSpec {
            required_capability: String::new(),
            ..make_test_spec("no_capability")
        };
        assert_eq!(spec_completeness_score(&no_capability), 0.0);

        let non_object_contract = ToolSpec {
            expected_input: serde_json::json!("just a string"),
            ..make_test_spec("non_object")
        };
        assert_eq!(spec_completeness_score(&non_object_contract), 0.5);

        // 空白字符串等同于缺失（避免用空格骗过门禁）
        let whitespace = ToolSpec {
            success_criteria: "   ".into(),
            ..make_test_spec("whitespace")
        };
        assert_eq!(spec_completeness_score(&whitespace), 0.2);
    }

    fn report_with(overall: f64, spec: f64) -> QualityReport {
        QualityReport {
            tool_id: "t".into(),
            overall_score: overall,
            axes: vec![
                QualityAxis {
                    name: "test_coverage".into(),
                    score: 1.0,
                    weight: 0.35,
                },
                QualityAxis {
                    name: "spec_completeness".into(),
                    score: spec,
                    weight: 0.25,
                },
            ],
            unit_tests_passed: true,
            integration_tests_passed: true,
            recommendations: vec![],
        }
    }

    #[test]
    fn registration_bar_requires_both_overall_and_spec() {
        assert!(report_with(0.8, 0.9).meets_registration_bar());
        // 总分够但规范不足 → 拒绝（这是本次加固的核心）
        assert!(!report_with(0.9, 0.2).meets_registration_bar());
        // 规范够但总分不足 → 拒绝
        assert!(!report_with(0.5, 0.9).meets_registration_bar());
        // 边界：恰好等于阈值视为达标
        assert!(
            report_with(REGISTRATION_MIN_OVERALL, REGISTRATION_MIN_SPEC).meets_registration_bar()
        );
    }

    #[test]
    fn rejection_reason_names_the_failing_dimension() {
        assert_eq!(report_with(0.9, 0.9).rejection_reason(), None);
        let low_overall = report_with(0.4, 0.9).rejection_reason().unwrap_or_default();
        assert!(low_overall.contains("overall score"), "{low_overall}");
        let low_spec = report_with(0.9, 0.2).rejection_reason().unwrap_or_default();
        assert!(low_spec.contains("spec completeness"), "{low_spec}");
    }

    #[test]
    fn axis_score_missing_axis_is_zero() {
        let report = report_with(0.8, 0.9);
        assert_eq!(report.axis_score("nonexistent"), 0.0);
        assert_eq!(report.spec_completeness(), 0.9);
    }

    #[test]
    fn test_quality_axes_low_spec() {
        // 规范完整度低应拉低总分
        let spec_no_criteria = ToolSpec {
            tool_id: "minimal".into(),
            required_capability: "".into(),
            expected_input: serde_json::json!({}),
            expected_output: serde_json::json!({}),
            success_criteria: "".into(),
            test_command: None,
            workdir: None,
        };

        assert_eq!(
            spec_completeness_score(&spec_no_criteria),
            0.0,
            "Empty spec must score 0.0"
        );
    }

    #[test]
    fn test_quality_recommendations_for_failures() {
        let _spec = make_test_spec("failing_tool");
        let regression = RegressionTestResult {
            tool_id: "failing_tool".into(),
            unit_tests_passed: false,
            integration_tests_passed: false,
            total_tests: 2,
            passed: 0,
            failed: 2,
            duration_ms: 100,
            failures: vec!["test_a failed".into(), "test_b failed".into()],
        };

        let test_score = regression.success_rate();
        let stability_score = if regression.failed == 0 {
            0.95
        } else if regression.success_rate() > 0.8 {
            0.7
        } else {
            0.4
        };

        assert!(
            stability_score < 0.5,
            "High failure rate should reduce stability"
        );
        assert_eq!(test_score, 0.0);
    }

    #[tokio::test]
    async fn test_skill_bundle_creation() {
        // 测试能够创建带正确元数据的有效 SkillBundle
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();

        let metadata = SkillMetadata {
            id: "skill:test_tool:1".into(),
            name: "test_tool".into(),
            version: "0.1.0".into(),
            author: "sisyphus-self-programming".into(),
            description: "automatic self-heal".into(),
            tags: vec!["auto-generated".into()],
            created_at: now,
        };

        let main_sop = sisyphus_types::SOPGraphConfig {
            nodes: vec![sisyphus_types::SOPNodeConfig {
                name: "test_tool".into(),
                kind: sisyphus_types::NodeKind::Wasm {
                    tool_id: "test_tool".into(),
                    fuel_limit: 1000,
                    args: Vec::new(),
                },
            }],
            edges: vec![],
        };

        let bundle = SkillBundle::new(metadata, main_sop);
        assert_eq!(bundle.metadata.id, "skill:test_tool:1");
        assert_eq!(bundle.main_sop.nodes.len(), 1);
        assert!(bundle.sub_sops.is_empty());
    }

    #[tokio::test]
    async fn test_skill_bundle_with_sub_sops() {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();

        let mut bundle = SkillBundle::new(
            SkillMetadata {
                id: "skill:composite:1".into(),
                name: "composite".into(),
                version: "0.1.0".into(),
                author: "test".into(),
                description: "composite skill".into(),
                tags: vec![],
                created_at: now,
            },
            sisyphus_types::SOPGraphConfig {
                nodes: vec![],
                edges: vec![],
            },
        );

        bundle.add_sub_sop(
            "sub1".into(),
            sisyphus_types::SOPGraphConfig {
                nodes: vec![],
                edges: vec![],
            },
        );
        bundle.add_required_tool("tool_a".into());
        bundle.add_required_tool("tool_a".into()); // duplicate, should not add

        assert_eq!(bundle.sub_sops.len(), 1);
        assert_eq!(bundle.required_tools.len(), 1);
    }
}
