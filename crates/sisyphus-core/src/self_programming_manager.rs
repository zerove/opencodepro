//! Sisyphus 自编程循环的编排器。
//!
//! ## 能力
//! - **工具缺口解决**：生成 → 真实编译 → 静态/语义验证 → 行为探针 → 部署 → 回归 → 注册
//! - **自动回归测试**：只运行 spec 里声明的测试命令，没有就如实报告「无 harness」
//! - **质量评估**：多轴质量评分（行为探针、测试通过率、规范完整度、稳定性、效率）
//! - **自动化技能注册**：将已部署工具打包为 SkillBundle 并注册到
//!   SkillStore，使系统能够发现它们
//!
//! ## 为什么每一步都必须有真实证据
//! 早期实现里编译是 mock、`unit_tests_passed` 恒为 `true`、回归失败被
//! 「soft pass」吞掉，于是自编程循环永远成功——即使产物根本不能被沙箱加载。
//! 现在每道关卡都要求可验证的证据：rustc 真的产出 wasm、模块真的导出
//! `memory` + `execute`、沙箱真的执行一次并返回可用载荷。任何一步拿不到
//! 证据，就当作失败并重试，绝不算通过。

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

/// 行为探针允许消耗的燃料上限。
pub const PROBE_FUEL_LIMIT: u64 = 1_000_000;

/// 行为探针的结论。
///
/// 「拿不到证据」与「证据表明失败」是两件事，必须分开：
/// 前者说明环境没有能力验证（例如默认执行器不是 WASM 沙箱），
/// 后者说明工具真的跑不通。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// 沙箱真实执行成功，返回了非空、非错误载荷。
    Passed {
        /// 输出字节数。
        output_len: usize,
        /// 沙箱报告的燃料消耗（0 表示执行器未计量）。
        fuel_consumed: u64,
    },
    /// 沙箱调用失败，或返回了错误载荷。
    Failed {
        /// 面向人的失败原因。
        reason: String,
    },
    /// 没有能证明「真的在沙箱里跑过」的执行器，无法验证行为。
    Unavailable {
        /// 为什么无法验证。
        reason: String,
    },
}

impl ProbeOutcome {
    /// 是否拿到了「工具可用」的正面证据。
    pub fn is_passed(&self) -> bool {
        matches!(self, Self::Passed { .. })
    }

    /// 单行描述，用于日志与回灌给代码生成器。
    pub fn describe(&self) -> String {
        match self {
            Self::Passed {
                output_len,
                fuel_consumed,
            } => {
                format!("probe passed ({output_len} bytes, {fuel_consumed} fuel)")
            },
            Self::Failed { reason } => format!("probe failed: {reason}"),
            Self::Unavailable { reason } => format!("probe unavailable: {reason}"),
        }
    }
}

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
    /// 阻止自动注册的硬失败（行为探针失败、声明的测试失败）。
    ///
    /// 加权总分可以「用其他轴补回来」，但这两类失败是**证据层面的否定**，
    /// 不允许被规范完整度或效率分抵消。
    pub blockers: Vec<String>,
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
    /// 需同时满足：没有硬失败（[`QualityReport::blockers`] 为空）、
    /// 加权总分 ≥ [`REGISTRATION_MIN_OVERALL`]、
    /// 规范完整度 ≥ [`REGISTRATION_MIN_SPEC`]。
    pub fn meets_registration_bar(&self) -> bool {
        self.blockers.is_empty()
            && self.overall_score >= REGISTRATION_MIN_OVERALL
            && self.spec_completeness() >= REGISTRATION_MIN_SPEC
    }

    /// 未达标的原因（达标时返回 `None`），用于日志与用户可见诊断。
    pub fn rejection_reason(&self) -> Option<String> {
        if !self.blockers.is_empty() {
            return Some(format!("hard failure: {}", self.blockers.join("; ")));
        }
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
///
/// 只有 `spec.test_command` 声明的命令会真的被执行；没有声明时
/// `harness_available = false`，`total_tests = 0`。**不存在**「没测试算通过」
/// 这种状态——那正是旧实现让质量门禁形同虚设的原因。
#[derive(Debug, Clone)]
pub struct RegressionTestResult {
    pub tool_id: String,
    /// 是否真的跑过声明的测试命令。
    pub harness_available: bool,
    /// 声明的测试命令是否通过（没有 harness 时恒为 `false`）。
    pub unit_tests_passed: bool,
    pub total_tests: u64,
    pub passed: u64,
    pub failed: u64,
    pub duration_ms: u64,
    pub failures: Vec<String>,
}

impl RegressionTestResult {
    /// 通过率。
    ///
    /// `total_tests == 0` 时返回 1.0 —— 语义是「没有证据表明失败」，
    /// 而不是「已验证通过」。判断质量时请结合 [`Self::harness_available`]，
    /// 不要把这个值当作满分证据。
    pub fn success_rate(&self) -> f64 {
        if self.total_tests == 0 {
            return 1.0;
        }
        self.passed as f64 / self.total_tests as f64
    }

    /// 没有 harness 的空结果。
    fn without_harness(tool_id: &str, reason: String, duration_ms: u64) -> Self {
        Self {
            tool_id: tool_id.to_string(),
            harness_available: false,
            unit_tests_passed: false,
            total_tests: 0,
            passed: 0,
            failed: 0,
            duration_ms,
            failures: vec![reason],
        }
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

    /// 覆盖生成产物的落盘目录。
    ///
    /// 必须与沙箱执行器的 `tools_path` 一致，否则探针读不到刚编译的产物。
    /// bridge 用 `sandbox.wasm_tools_dir` 调用它完成接线。
    pub fn set_tools_dir(&self, dir: impl Into<std::path::PathBuf>) {
        self.compiler.set_tools_dir(dir);
    }

    /// 当前生成产物目录。
    pub fn tools_dir(&self) -> std::path::PathBuf {
        self.compiler.tools_dir()
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
    /// 真实编译、静态/语义验证、行为探针、部署后回归与自动化技能注册。
    ///
    /// # Errors
    /// 三次尝试都没拿到「可部署 + 探针通过」的证据时返回
    /// [`crate::CoreError::Internal`]，并把最后一次失败原因带上。
    /// LLM 调用本身失败则直接返回错误（重试没有意义）。
    pub async fn resolve_gap(&self, gap: ToolGap) -> Result<(), crate::CoreError> {
        let tool_id = gap.suggested_tool_spec.tool_id.clone();
        info!(gap_id = %gap.gap_id, tool_id = %tool_id, "Starting self-programming loop to resolve tool gap");

        let mut feedback: Option<String> = None;
        let max_retries = 3;
        let mut last_error = String::from("no attempt produced a result");

        for attempt in 1..=max_retries {
            info!("Self-programming attempt {}/{}", attempt, max_retries);

            // 1. 通知开始
            self.notify(CognitiveEvent::ToolGenerationStarted {
                tool_id: tool_id.clone(),
                capability: gap.required_capability.clone(),
            })
            .await;

            // 2. 生成代码
            let code = self
                .coder
                .generate_code(&gap.suggested_tool_spec, feedback.clone())
                .await
                .map_err(|e| crate::CoreError::Internal(e.to_string()))?;

            // 3. 真实编译。编译失败是最常见的生成问题，必须可重试——
            //    把 rustc 的 stderr 原样回灌给 coder，否则下一轮只会重复同样的错误。
            let wasm_path = match self.compiler.compile(&tool_id, &code).await {
                Ok(path) => path,
                Err(e) => {
                    warn!(attempt, tool_id = %tool_id, error = %e, "WASM compilation failed");
                    last_error = e.to_string();
                    feedback = Some(format!("Attempt {attempt} failed to compile:\n{e}"));
                    continue;
                },
            };

            // 4. 静态契约 + 语义审计
            let v_report = self
                .verifier
                .verify(&gap.suggested_tool_spec, &code, &wasm_path)
                .await
                .map_err(|e| crate::CoreError::Internal(e.to_string()))?;

            if !v_report.is_deployable() {
                warn!(
                    attempt,
                    tool_id = %tool_id,
                    score = v_report.confidence_score,
                    failures = ?v_report.failures,
                    "Tool verification failed"
                );
                last_error = v_report.failures.join("; ");
                feedback = Some(format!(
                    "Attempt {attempt} verification failures:\n{}",
                    v_report.failures.join("\n")
                ));
                continue;
            }

            // 5. 行为探针：让沙箱真的执行一次刚编译出来的模块。
            //    先失效缓存，保证读到的是本轮产物而不是上一轮的。
            self.execution_manager.notify_tool_deployed(&tool_id);
            let probe = self.probe_tool(&gap.suggested_tool_spec).await;
            if !probe.is_passed() {
                warn!(attempt, tool_id = %tool_id, probe = %probe.describe(), "Behavioral probe did not pass");
                last_error = probe.describe();
                feedback = Some(format!(
                    "Attempt {attempt} behavioral probe: {}",
                    probe.describe()
                ));
                continue;
            }

            self.notify(CognitiveEvent::ToolVerified {
                tool_id: tool_id.clone(),
                score: v_report.confidence_score as f64,
            })
            .await;
            info!(
                "Tool verified (score {:.2}) and probed: {}",
                v_report.confidence_score,
                probe.describe()
            );

            // 6. 部署并沉淀（Sediment）
            let asset_id = self
                .knowledge_manager
                .sediment_tool(gap.suggested_tool_spec.clone(), wasm_path)
                .await
                .map_err(|e| crate::CoreError::Internal(e.to_string()))?;

            self.notify(CognitiveEvent::ToolDeployed {
                tool_id: tool_id.clone(),
                asset_id: asset_id.clone(),
            })
            .await;
            self.execution_manager.notify_tool_deployed(&tool_id);

            info!(
                "Tool deployed (asset_id: {}). Running regression tests...",
                asset_id
            );

            // 7. 对已部署工具运行回归测试
            let regress_result =
                self.run_regression_tests(&gap.suggested_tool_spec, &asset_id).await;

            // 8. 评估质量
            let quality =
                self.evaluate_quality(&gap.suggested_tool_spec, &regress_result, &probe).await;

            self.notify(CognitiveEvent::QualityEvaluation {
                tool_id: tool_id.clone(),
                unit_tests_passed: quality.unit_tests_passed,
                integration_tests_passed: quality.integration_tests_passed,
                regression_score: regress_result.success_rate(),
                quality_score: quality.overall_score,
            })
            .await;

            if quality.meets_registration_bar() {
                // 9. 质量合格时自动注册为技能
                self.register_skill(&gap, &quality).await;
            } else {
                warn!(
                    tool_id = %tool_id,
                    quality = quality.overall_score,
                    spec_completeness = quality.spec_completeness(),
                    reason = ?quality.rejection_reason(),
                    "Tool quality below registration bar; skipping skill registration"
                );
            }

            info!(
                "Successfully resolved tool gap {}. Tool {tool_id} deployed, probed, tested, and quality-scored.",
                gap.gap_id
            );
            return Ok(());
        }

        // 所有尝试都失败：清掉半成品，避免沙箱把一个行为错误的模块当成可用工具。
        self.remove_artifact(&tool_id).await;

        Err(crate::CoreError::Internal(format!(
            "Failed to resolve gap after {max_retries} attempts; last error: {last_error}"
        )))
    }

    /// 删除本工具的落盘产物（尽力而为，失败只记日志）。
    async fn remove_artifact(&self, tool_id: &str) {
        let path = match sisyphus_intel::artifact_path(&self.tools_dir(), tool_id) {
            Ok(path) => path,
            Err(reason) => {
                warn!(tool_id = %tool_id, %reason, "Cannot compute artifact path for cleanup");
                return;
            },
        };
        match tokio::fs::remove_file(&path).await {
            Ok(()) => {
                info!(tool_id = %tool_id, path = %path.display(), "Removed failed tool artifact")
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
            Err(e) => {
                warn!(tool_id = %tool_id, path = %path.display(), error = %e, "Failed to remove artifact")
            },
        }
    }

    /// 在沙箱里真实执行一次工具，作为「它确实能用」的证据。
    ///
    /// 用 `execute_measured` 而不是 `execute`：只有能报告测量的执行器
    /// （`WasmToolExecutor` / `SandboxdClient`）才证明模块真的跑过。
    /// 默认实现的 `measurement = None` 意味着执行器可能是任意 mock，
    /// 此时如实返回 [`ProbeOutcome::Unavailable`]，而不是假装通过。
    pub async fn probe_tool(&self, spec: &ToolSpec) -> ProbeOutcome {
        let input = serde_json::to_vec(&spec.expected_input).unwrap_or_else(|_| b"{}".to_vec());

        let outcome = self
            .execution_manager
            .tool_router
            .default
            .execute_measured(&spec.tool_id, input, PROBE_FUEL_LIMIT)
            .await;

        let (data, measurement) = match outcome {
            Ok(pair) => pair,
            Err(e) => {
                return ProbeOutcome::Failed {
                    reason: format!("sandbox call failed: {e}"),
                }
            },
        };

        let Some(measurement) = measurement else {
            return ProbeOutcome::Unavailable {
                reason: "the registered default executor reports no execution measurement, \
                         so the probe cannot prove the wasm sandbox actually ran"
                    .into(),
            };
        };

        if data.is_empty() {
            return ProbeOutcome::Failed {
                reason: "tool returned an empty payload".into(),
            };
        }

        // 生成代码用 `{"error": "..."}` 表达业务失败（见编译器的 ABI 适配层）。
        let text = String::from_utf8_lossy(&data);
        if let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(&text)
        {
            if let Some(err) = map.get("error").and_then(serde_json::Value::as_str) {
                return ProbeOutcome::Failed {
                    reason: format!("tool reported an error: {err}"),
                };
            }
        }

        ProbeOutcome::Passed {
            output_len: data.len(),
            fuel_consumed: measurement.fuel_consumed,
        }
    }

    // ---------------------------------------------------------------
    // 回归测试
    // ---------------------------------------------------------------

    /// 对已部署工具运行自动化回归测试。
    ///
    /// 只执行 `spec.test_command` 声明的命令。旧实现会在没有声明时
    /// 自己拼一个 `cargo test --test {tool_id}_test`，跑不通还记成通过
    /// （"soft pass"）——于是「没有测试」和「测试全绿」在质量报告里完全
    /// 无法区分。现在没有 harness 就如实返回 `harness_available = false`。
    pub async fn run_regression_tests(
        &self,
        spec: &ToolSpec,
        _asset_id: &str,
    ) -> RegressionTestResult {
        let start = std::time::Instant::now();
        let tool_id = &spec.tool_id;

        let Some(test_cmd) = spec.test_command.as_deref().map(str::trim) else {
            info!(tool_id = %tool_id, "No test_command declared; skipping regression harness");
            return RegressionTestResult::without_harness(
                tool_id,
                "No test_command declared; nothing was executed".into(),
                start.elapsed().as_millis() as u64,
            );
        };

        if test_cmd.is_empty() {
            info!(tool_id = %tool_id, "Empty test_command; skipping regression harness");
            return RegressionTestResult::without_harness(
                tool_id,
                "test_command is empty; nothing was executed".into(),
                start.elapsed().as_millis() as u64,
            );
        }

        let mut failures = Vec::new();
        let mut total = 0u64;
        let mut passed = 0u64;

        total += 1;
        let unit_ok = match self
            .execution_manager
            .executor
            .test_executor
            .run_test(test_cmd, spec.workdir.as_deref())
            .await
        {
            Ok(result) if result.success => {
                passed += 1;
                info!(tool_id = %tool_id, command = %test_cmd, "Declared test command passed");
                true
            },
            Ok(result) => {
                let detail = result.error_summary.unwrap_or_else(|| {
                    format!("exit code {}; stderr: {}", result.exit_code, result.stderr)
                });
                failures.push(format!("Declared test command failed: {detail}"));
                warn!(tool_id = %tool_id, command = %test_cmd, %detail, "Declared test command failed");
                false
            },
            Err(e) => {
                failures.push(format!("Declared test command could not run: {e}"));
                warn!(tool_id = %tool_id, command = %test_cmd, error = %e, "Test harness unavailable");
                false
            },
        };

        RegressionTestResult {
            tool_id: tool_id.clone(),
            harness_available: true,
            // 这个标记必须来自执行过程的真实结果，不能从 failures 文本里
            // “嗅探”关键词——早期实现写成
            // `failures.iter().any(|f| f.contains("Unit test"))`，语义正好相反。
            unit_tests_passed: unit_ok,
            total_tests: total,
            passed,
            failed: total.saturating_sub(passed),
            duration_ms: start.elapsed().as_millis() as u64,
            failures,
        }
    }

    // ---------------------------------------------------------------
    // 质量评估
    // ---------------------------------------------------------------

    /// 从多个轴评估工具质量。
    /// 返回结构化报告，含总分（0.0 - 1.0）。
    ///
    /// 权重分配的依据是「证据强度」：行为探针是唯一直接证明工具可用的
    /// 信号，权重最高；测试通过率只在声明了 harness 时才有意义；
    /// 规范完整度既是加权项也是独立硬门禁。
    pub async fn evaluate_quality(
        &self,
        spec: &ToolSpec,
        regression: &RegressionTestResult,
        probe: &ProbeOutcome,
    ) -> QualityReport {
        let mut axes = Vec::new();
        let mut recommendations = Vec::new();
        let mut blockers = Vec::new();

        // 轴 1：行为探针
        let probe_score = match probe {
            ProbeOutcome::Passed { .. } => 1.0,
            ProbeOutcome::Failed { .. } => 0.0,
            // 没验证成不等于失败，但也绝不是通过；给中性分并给出建议。
            ProbeOutcome::Unavailable { .. } => 0.5,
        };
        axes.push(QualityAxis {
            name: "behavioral_probe".into(),
            score: probe_score,
            weight: 0.35,
        });

        // 轴 2：声明的测试命令通过率（无 harness 时给中性分，不是满分）
        let test_score = if regression.harness_available {
            regression.success_rate()
        } else {
            0.5
        };
        axes.push(QualityAxis {
            name: "test_coverage".into(),
            score: test_score,
            weight: 0.15,
        });

        // 轴 3：规范完整度
        let spec_score = spec_completeness_score(spec);
        axes.push(QualityAxis {
            name: "spec_completeness".into(),
            score: spec_score,
            weight: 0.25,
        });

        // 轴 4：稳定性 —— 探针通过且测试无失败才算稳
        let stability_score = match probe {
            ProbeOutcome::Passed { .. } if regression.failed == 0 => 0.95,
            ProbeOutcome::Passed { .. } => 0.7,
            ProbeOutcome::Unavailable { .. } => 0.5,
            ProbeOutcome::Failed { .. } => 0.1,
        };
        axes.push(QualityAxis {
            name: "stability".into(),
            score: stability_score,
            weight: 0.15,
        });

        // 轴 5：执行效率 —— 用探针测到的真实燃料消耗，而不是命令字符串长度
        let efficiency_score = match probe {
            ProbeOutcome::Passed {
                fuel_consumed: 0, ..
            } => 0.5,
            ProbeOutcome::Passed { fuel_consumed, .. } if *fuel_consumed < 10_000 => 1.0,
            ProbeOutcome::Passed { fuel_consumed, .. } if *fuel_consumed < 200_000 => 0.8,
            ProbeOutcome::Passed { .. } => 0.5,
            _ => 0.5,
        };
        axes.push(QualityAxis {
            name: "efficiency".into(),
            score: efficiency_score,
            weight: 0.10,
        });

        // 计算加权总分
        let overall_score: f64 = axes.iter().map(|a| a.score * a.weight).sum();

        // 生成建议
        match probe {
            ProbeOutcome::Failed { reason } => {
                blockers.push(format!("behavioral probe failed: {reason}"));
                recommendations.push(format!(
                    "Fix the behavioral failure before deployment: {reason}"
                ));
            },
            ProbeOutcome::Unavailable { .. } => recommendations.push(
                "Wire a measuring tool executor (wasm sandbox) so the probe can prove the tool runs"
                    .to_string(),
            ),
            ProbeOutcome::Passed { .. } => {},
        }
        if regression.harness_available && regression.failed > 0 {
            blockers.push(format!(
                "declared test command reported {} failing test(s)",
                regression.failed
            ));
        }
        if !regression.harness_available {
            recommendations.push("Declare a test_command to get regression coverage".to_string());
        }
        if regression.failed > 0 {
            recommendations.push("Fix failing tests before deployment to production".to_string());
        }
        if spec.success_criteria.trim().is_empty() {
            recommendations.push("Add explicit success criteria to the tool spec".to_string());
        }
        if overall_score < 0.5 {
            recommendations
                .push("Consider regenerating the tool with more detailed spec".to_string());
        }

        QualityReport {
            tool_id: spec.tool_id.clone(),
            overall_score,
            axes,
            unit_tests_passed: regression.unit_tests_passed,
            integration_tests_passed: probe.is_passed(),
            blockers,
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

    fn probe_passed() -> ProbeOutcome {
        ProbeOutcome::Passed {
            output_len: 32,
            fuel_consumed: 1_500,
        }
    }

    fn probe_failed() -> ProbeOutcome {
        ProbeOutcome::Failed {
            reason: "sandbox call failed: trap".into(),
        }
    }

    fn probe_unavailable() -> ProbeOutcome {
        ProbeOutcome::Unavailable {
            reason: "executor reports no measurement".into(),
        }
    }

    fn clean_regression(tool_id: &str) -> RegressionTestResult {
        RegressionTestResult {
            tool_id: tool_id.into(),
            harness_available: true,
            unit_tests_passed: true,
            total_tests: 1,
            passed: 1,
            failed: 0,
            duration_ms: 10,
            failures: vec![],
        }
    }

    fn no_harness_regression(tool_id: &str) -> RegressionTestResult {
        RegressionTestResult {
            tool_id: tool_id.into(),
            harness_available: false,
            unit_tests_passed: false,
            total_tests: 0,
            passed: 0,
            failed: 0,
            duration_ms: 0,
            failures: vec!["No test_command declared".into()],
        }
    }

    #[test]
    fn probe_outcome_classification() {
        assert!(probe_passed().is_passed());
        assert!(!probe_failed().is_passed());
        assert!(
            !probe_unavailable().is_passed(),
            "an unrun probe must never count as a pass"
        );
        assert!(probe_unavailable().describe().contains("unavailable"));
    }

    #[test]
    fn regression_result_success_rate() {
        let r = RegressionTestResult {
            tool_id: "t".into(),
            harness_available: true,
            unit_tests_passed: false,
            total_tests: 10,
            passed: 7,
            failed: 3,
            duration_ms: 500,
            failures: vec!["test_1 failed".into(), "test_2 failed".into()],
        };
        assert!((r.success_rate() - 0.7).abs() < 0.001);
    }

    #[test]
    fn regression_result_empty() {
        let r = no_harness_regression("empty");
        assert_eq!(
            r.success_rate(),
            1.0,
            "no evidence of failure, not proof of success"
        );
        assert!(!r.harness_available);
        assert_eq!(r.total_tests, 0);
    }

    #[test]
    fn regression_result_all_fail() {
        let r = RegressionTestResult {
            tool_id: "fail_all".into(),
            harness_available: true,
            unit_tests_passed: false,
            total_tests: 3,
            passed: 0,
            failed: 3,
            duration_ms: 50,
            failures: vec!["all failed".into()],
        };
        assert_eq!(r.success_rate(), 0.0);
    }

    #[tokio::test]
    async fn weights_sum_to_one_and_probe_dominates() {
        let spec = make_test_spec("sum_check");
        let manager = manager_with_noop_executors().await;

        let report =
            manager.evaluate_quality(&spec, &clean_regression("sum_check"), &probe_passed()).await;
        let weight_sum: f64 = report.axes.iter().map(|a| a.weight).sum();
        assert!(
            (weight_sum - 1.0).abs() < 1e-9,
            "weights must sum to 1.0, got {weight_sum}"
        );

        let probe_axis = report.axes.iter().find(|a| a.name == "behavioral_probe").expect("axis");
        assert!(
            probe_axis.weight >= 0.3,
            "the only direct evidence must carry the most weight"
        );

        // 满分：探针通过 + 测试全绿 + 规范齐备
        assert!(report.overall_score > 0.9, "{}", report.overall_score);
        assert!(report.integration_tests_passed);
    }

    #[tokio::test]
    async fn failed_probe_fails_the_registration_bar() {
        let manager = manager_with_noop_executors().await;
        let spec = make_test_spec("probe_fail");
        let report =
            manager.evaluate_quality(&spec, &clean_regression("probe_fail"), &probe_failed()).await;

        assert_eq!(report.axis_score("behavioral_probe"), 0.0);
        assert!(
            !report.meets_registration_bar(),
            "a tool whose probe failed must not be registered (score {:.3})",
            report.overall_score
        );
        assert!(!report.integration_tests_passed);
        assert!(report.recommendations.iter().any(|r| r.contains("behavioral failure")));
        assert!(
            report.rejection_reason().is_some_and(|r| r.contains("behavioral probe failed")),
            "the rejection reason must name the hard failure"
        );
    }

    #[tokio::test]
    async fn unavailable_probe_is_neutral_not_full_marks() {
        let manager = manager_with_noop_executors().await;
        let spec = make_test_spec("probe_na");
        let report = manager
            .evaluate_quality(&spec, &clean_regression("probe_na"), &probe_unavailable())
            .await;

        assert_eq!(report.axis_score("behavioral_probe"), 0.5);
        assert!(
            !report.integration_tests_passed,
            "unverified is not verified"
        );
        assert!(report.recommendations.iter().any(|r| r.contains("measuring tool executor")));
    }

    #[tokio::test]
    async fn missing_harness_does_not_score_full_test_coverage() {
        let manager = manager_with_noop_executors().await;
        let spec = make_test_spec("no_harness");
        let with_harness =
            manager.evaluate_quality(&spec, &clean_regression("no_harness"), &probe_passed()).await;
        let without_harness = manager
            .evaluate_quality(&spec, &no_harness_regression("no_harness"), &probe_passed())
            .await;

        assert_eq!(with_harness.axis_score("test_coverage"), 1.0);
        assert_eq!(without_harness.axis_score("test_coverage"), 0.5);
        assert!(
            without_harness.overall_score < with_harness.overall_score,
            "no tests must score strictly lower than green tests"
        );
        assert!(without_harness.recommendations.iter().any(|r| r.contains("test_command")));
    }

    #[tokio::test]
    async fn failing_declared_tests_block_registration_even_with_a_green_probe() {
        let manager = manager_with_noop_executors().await;
        let spec = make_test_spec("tests_fail");
        let failing = RegressionTestResult {
            tool_id: "tests_fail".into(),
            harness_available: true,
            unit_tests_passed: false,
            total_tests: 1,
            passed: 0,
            failed: 1,
            duration_ms: 5,
            failures: vec!["boom".into()],
        };
        let report = manager.evaluate_quality(&spec, &failing, &probe_passed()).await;

        // 加权分可以很高（探针通过了），但声明的测试失败是硬失败。
        assert!(
            !report.meets_registration_bar(),
            "a declared-but-failing test suite must block registration (score {:.3})",
            report.overall_score
        );
        assert!(report.rejection_reason().is_some_and(|r| r.contains("hard failure")));
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
                    name: "behavioral_probe".into(),
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
            blockers: vec![],
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

    // ── 探针语义 ────────────────────────────────────────────────

    /// 不报告测量的执行器 → `Unavailable`，绝不能算通过。
    struct PlainExecutor;

    #[async_trait::async_trait]
    impl sisyphus_types::ToolExecutor for PlainExecutor {
        async fn execute(
            &self,
            _tool_id: &str,
            _args: Vec<u8>,
            _fuel: u64,
        ) -> Result<Vec<u8>, sisyphus_types::TypesError> {
            Ok(b"looks fine".to_vec())
        }
    }

    /// 报告测量但返回错误载荷的执行器 → `Failed`。
    struct ErrorPayloadExecutor;

    #[async_trait::async_trait]
    impl sisyphus_types::ToolExecutor for ErrorPayloadExecutor {
        async fn execute(
            &self,
            _tool_id: &str,
            _args: Vec<u8>,
            _fuel: u64,
        ) -> Result<Vec<u8>, sisyphus_types::TypesError> {
            Ok(b"{\"error\":\"bad input\"}".to_vec())
        }

        async fn execute_measured(
            &self,
            tool_id: &str,
            args: Vec<u8>,
            fuel: u64,
        ) -> Result<
            (Vec<u8>, Option<sisyphus_types::ExecutionMeasurement>),
            sisyphus_types::TypesError,
        > {
            let data = self.execute(tool_id, args, fuel).await?;
            Ok((
                data,
                Some(sisyphus_types::ExecutionMeasurement {
                    fuel_consumed: 1_000,
                    duration_ms: 1,
                    peak_memory_bytes: None,
                }),
            ))
        }
    }

    async fn manager_with_executor(
        executor: Arc<dyn sisyphus_types::ToolExecutor>,
    ) -> SelfProgrammingManager {
        let state_store = Arc::new(
            sisyphus_mem::MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
                .await
                .expect("state store"),
        );
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.expect("pool");
        let sop_store = Arc::new(sisyphus_mem::SopSqlStore::new(pool).await.expect("sop store"));

        struct NoopTest;
        #[async_trait::async_trait]
        impl sisyphus_types::TestExecutor for NoopTest {
            async fn run_test(
                &self,
                _command: &str,
                _workdir: Option<&str>,
            ) -> Result<sisyphus_types::TestResult, sisyphus_types::TypesError> {
                Ok(sisyphus_types::TestResult {
                    success: true,
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: 0,
                    duration_ms: 0,
                    error_summary: None,
                })
            }
        }

        let exec_manager = Arc::new(crate::ExecutionManager::new(
            state_store,
            sop_store,
            executor,
            Arc::new(NoopTest),
            Arc::new(crate::intervention_manager::InterventionManager::new()),
            Arc::new(crate::internalization_manager::InternalizationManager::new(
                Arc::new(sisyphus_tools_llm::metis::DummyLlmClient),
                Arc::new(sisyphus_knowledge::AssetManager::new(
                    ".omo/test_self_programming_unit".into(),
                )),
            )),
            None,
        ));

        SelfProgrammingManager::new(
            Arc::new(sisyphus_tools_llm::metis::DummyLlmClient),
            exec_manager,
            Arc::new(sisyphus_knowledge::AssetManager::new(
                ".omo/test_self_programming_unit".into(),
            )),
            None,
        )
    }

    async fn manager_with_noop_executors() -> SelfProgrammingManager {
        manager_with_executor(Arc::new(PlainExecutor)).await
    }

    #[tokio::test]
    async fn probe_without_measurement_is_unavailable() {
        let manager = manager_with_noop_executors().await;
        let outcome = manager.probe_tool(&make_test_spec("plain")).await;
        assert!(
            matches!(outcome, ProbeOutcome::Unavailable { .. }),
            "{outcome:?}"
        );
    }

    #[tokio::test]
    async fn probe_detects_error_payload() {
        let manager = manager_with_executor(Arc::new(ErrorPayloadExecutor)).await;
        let outcome = manager.probe_tool(&make_test_spec("err")).await;
        match outcome {
            ProbeOutcome::Failed { reason } => assert!(reason.contains("bad input"), "{reason}"),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn tools_dir_can_be_redirected() {
        let dir = tempfile::tempdir().expect("temp dir");
        let manager = manager_with_noop_executors().await;
        manager.set_tools_dir(dir.path());
        assert_eq!(manager.tools_dir(), dir.path());
    }
}
