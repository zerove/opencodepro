use sisyphus_intel::{ReflectionReport, SimulationResult, SisyphusIntel};
use sisyphus_types::{ExecutionContext, SOPGraph};
use std::sync::Arc;
use std::time::Duration;

/// IntelligenceManager 封装系统的认知能力，
/// 为预测、反思与演进提供高层 API。
pub struct IntelligenceManager {
    pub intel: Arc<SisyphusIntel>,
    pub synthesizer: Arc<dyn sisyphus_intel::FlowSynthesizer>,
    pub llm_client: Arc<dyn sisyphus_types::LlmClient>,
    /// Deep semantic review. Injected rather than called directly: core used to
    /// import `momus`/`oracle` here, which kept `sisyphus-tools-llm-tools` in
    /// core's production dependencies (ADR-014).
    pub expert_reviewer: Arc<dyn sisyphus_types::ExpertReviewer>,
}

/// 批判阶段的宽松默认结果：无发现、零风险、允许继续。
/// 用于 LLM 不可达、超时或被显式禁用时的安全降级。
fn permissive_critique(reason: &str) -> sisyphus_types::CritiqueResult {
    sisyphus_types::CritiqueResult {
        critical_findings: Vec::new(),
        risk_score: 0.0,
        suggested_refinement: format!("No critique available ({})", reason),
        should_proceed: true,
    }
}

impl IntelligenceManager {
    /// New.
    pub fn new(
        intel: Arc<SisyphusIntel>,
        synthesizer: Arc<dyn sisyphus_intel::FlowSynthesizer>,
        llm_client: Arc<dyn sisyphus_types::LlmClient>,
        expert_reviewer: Arc<dyn sisyphus_types::ExpertReviewer>,
    ) -> Self {
        Self {
            intel,
            synthesizer,
            llm_client,
            expert_reviewer,
        }
    }

    /// 从自然语言目标综合生成新的 SOP 图谱。
    pub async fn synthesize_goal(
        &self,
        goal: &str,
        ctx: &ExecutionContext,
        project_id: &str,
        mode: sisyphus_types::WorkMode,
        insights: Option<&str>,
    ) -> Result<sisyphus_intel::synthesis::SynthesisResult, sisyphus_types::IntelError> {
        self.synthesizer.synthesize(goal, ctx, project_id, mode, insights).await
    }

    /// 使用仿真器预测图谱执行的结果。
    pub async fn predict_outcome(
        &self,
        graph: Arc<SOPGraph>,
        ctx: &ExecutionContext,
        project_id: &str,
        sop_id: &str,
    ) -> Result<SimulationResult, sisyphus_types::IntelError> {
        self.intel.simulator.simulate(graph, ctx, project_id, sop_id, None).await
    }

    /// 反思执行轨迹，计算偏差并提出改进建议。
    /// Run the two-stage reflection pipeline on an execution trace.
    ///
    /// Stage 1: the base reflector produces a [`ReflectionReport`] from the
    /// raw trace, the original prediction, and the execution context. Stage
    /// 2 (when available) uses the expert reflection tool to enrich the
    /// report with deeper semantic suggestions for graph evolution.
    ///
    /// # Arguments
    /// * `trace` - Serialised execution trace (typically from [`ExecutionContext`])
    /// * `prediction` - The prediction that preceded execution
    /// * `ctx` - Current execution context for the SOP run
    ///
    /// # Returns
    /// A [`ReflectionReport`] suitable for consumption by [`IntelligenceManager::evolve_graph`].
    pub async fn reflect_on_trace(
        &self,
        trace: &str,
        prediction: &SimulationResult,
        ctx: &ExecutionContext,
    ) -> Result<ReflectionReport, sisyphus_types::IntelError> {
        // 1. 核心反思器的基础反思
        let mut report = self.intel.reflector.reflect(trace, prediction, None).await?;

        // 2. 深度语义反思交给注入的专家评审器。
        //
        // 具体实现（Oracle 文件分析 + Momus 计划/代码评审）连同它的启发式判断
        // 都在 tools 层；core 只负责把输入凑齐并把字符串收下。锁在 await 之前
        // 就已释放，避免跨 await 持锁。
        let modified_files: Vec<std::path::PathBuf> = ctx
            .modified_files
            .read()
            .map(|files| files.iter().cloned().collect())
            .unwrap_or_default();
        let results: Vec<(String, String)> = ctx
            .results
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().to_string()))
            .collect();

        let expert_suggestions = self
            .expert_reviewer
            .suggestions(modified_files, results, self.llm_client.as_ref())
            .await;

        // 将专家发现合并到报告中
        report.suggested_improvements.extend(expert_suggestions);

        Ok(report)
    }

    /// 批判性地分析反思报告，识别风险或盲点。
    pub async fn critique_reflection(
        &self,
        report: &ReflectionReport,
        prediction: &SimulationResult,
        // B3：统一的循环内 LLM 阶段超时（秒），来自 cognitive_loop 配置；
        // 0 表示禁用该阶段的 LLM 调用。
        timeout_secs: u64,
    ) -> Result<sisyphus_types::CritiqueResult, sisyphus_types::IntelError> {
        let prompt = format!(
            "You are a rigorous AI Architecture Critic. Analyze the following reflection report and simulation prediction.\n\n\
            Simulation Prediction:\n{:?}\n\n\
            Reflection Report:\n{:?}\n\n\
            Task: Identify any logical fallacies, over-optimistic assumptions, or missing edge cases in the suggested improvements. \
            Determine if the proposed evolution is safe or if it might introduce new regressions.\n\n\
            Return your response in JSON format with the following fields:\n\
            - critical_findings: List of specific concerns\n\
            - risk_score: 0.0 to 1.0\n\
            - suggested_refinement: How to improve the evolution plan\n\
            - should_proceed: boolean",
            prediction, report
        );

        // 零超时 = 显式禁用批判阶段（跳过 LLM 调用）。
        if timeout_secs == 0 {
            tracing::debug!("LLM critique disabled via zero timeout");
            return Ok(permissive_critique("disabled"));
        }

        // 为批判调用设置可配置边界（评审 B3：原硬编码 2s 远低于生产
        // LLM 延迟），使不可达或缓慢的 LLM 无法拖慢认知循环；
        // 失败/超时时回退到宽松默认值。
        let timeout = Duration::from_secs(timeout_secs);
        let response = match tokio::time::timeout(timeout, self.llm_client.generate(&prompt)).await
        {
            Ok(Ok(resp)) => resp,
            Ok(Err(e)) => {
                tracing::warn!(
                    "LLM critique failed after {}s, using permissive default: {e}",
                    timeout.as_secs()
                );
                return Ok(permissive_critique("LLM unreachable"));
            },
            Err(_) => {
                tracing::warn!(
                    "LLM critique timed out after {}s, using permissive default",
                    timeout.as_secs()
                );
                return Ok(permissive_critique("LLM timed out"));
            },
        };

        serde_json::from_str::<sisyphus_types::CritiqueResult>(&response).map_err(|e| {
            sisyphus_types::IntelError::Internal(format!(
                "Failed to parse critique JSON: {}. Response was: {}",
                e, response
            ))
        })
    }

    /// 基于反思报告与预测演进图谱。
    pub async fn evolve_graph(
        &self,
        graph: Arc<SOPGraph>,
        report: &ReflectionReport,
        prediction: &SimulationResult,
        insights: Option<&str>,
    ) -> Result<Arc<SOPGraph>, sisyphus_types::IntelError> {
        self.intel.evolver.evolve(graph, report, prediction, insights).await
    }
}
