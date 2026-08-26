use sisyphus_intel::{ReflectionReport, SimulationResult, SisyphusIntel};
use sisyphus_tools_llm_tools::{momus, oracle};
use sisyphus_types::{ExecutionContext, SOPGraph};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// IntelligenceManager 封装系统的认知能力，
/// 为预测、反思与演进提供高层 API。
pub struct IntelligenceManager {
    pub intel: Arc<SisyphusIntel>,
    pub synthesizer: Arc<dyn sisyphus_intel::FlowSynthesizer>,
    pub llm_client: Arc<dyn sisyphus_tools_llm::LlmClient>,
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
    pub fn new(
        intel: Arc<SisyphusIntel>,
        synthesizer: Arc<dyn sisyphus_intel::FlowSynthesizer>,
        llm_client: Arc<dyn sisyphus_tools_llm::LlmClient>,
    ) -> Self {
        Self {
            intel,
            synthesizer,
            llm_client,
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
        self.synthesizer
            .synthesize(goal, ctx, project_id, mode, insights)
            .await
    }

    /// 使用仿真器预测图谱执行的结果。
    pub async fn predict_outcome(
        &self,
        graph: Arc<SOPGraph>,
        ctx: &ExecutionContext,
        project_id: &str,
        sop_id: &str,
    ) -> Result<SimulationResult, sisyphus_types::IntelError> {
        self.intel
            .simulator
            .simulate(graph, ctx, project_id, sop_id, None)
            .await
    }

    /// 反思执行轨迹，计算偏差并提出改进建议。
    pub async fn reflect_on_trace(
        &self,
        trace: &str,
        prediction: &SimulationResult,
        ctx: &ExecutionContext,
    ) -> Result<ReflectionReport, sisyphus_types::IntelError> {
        // 1. 核心反思器的基础反思
        let mut report = self
            .intel
            .reflector
            .reflect(trace, prediction, None)
            .await?;

        // 2. 使用专业工具的深度语义反思
        let mut expert_suggestions = Vec::new();

        // a. Oracle 分析：检查所有被修改文件的复杂度与安全问题
        if let Ok(files) = ctx.modified_files.read() {
            for path in files.iter() {
                if let Ok(analysis) = oracle::analyze_file(path) {
                    for issue in analysis.issues {
                        expert_suggestions.push(format!(
                            "[{}] {} at {}:{}: {}",
                            issue.severity,
                            issue.message,
                            path.display(),
                            issue.line,
                            issue.code.as_deref().unwrap_or("")
                        ));
                    }
                }
            }
        }

        // b. Momus 评审：审查结果中类计划或类代码的输出
        for entry in ctx.results.iter() {
            let node_name = entry.key();
            let output = entry.value();
            let content = output.to_string();

            // 启发式规则：若节点名包含 'plan' 或输出较长且结构化，使用 Momus 计划评审
            if node_name.contains("plan") || content.contains("## ") {
                let review =
                    momus::review_plan(self.llm_client.as_ref(), &content, node_name.as_str())
                        .await;
                for comment in review.comments {
                    expert_suggestions.push(format!(
                        "[{}] (Plan Review) {}: {} - Suggestion: {}",
                        comment.severity,
                        comment.category,
                        comment.message,
                        comment.suggestion.as_deref().unwrap_or("N/A")
                    ));
                }
            }

            // 若输出看起来像代码（含常见 Rust 关键字），使用 Momus 代码评审
            if content.contains("fn ") || content.contains("pub struct") {
                // review_code 需要路径。因数据在内存中，这里使用虚拟路径。
                let path_str = format!("/virtual/{}", node_name);
                let virtual_path = Path::new(&path_str);
                if let Ok(review) =
                    momus::review_code(self.llm_client.as_ref(), virtual_path, &content).await
                {
                    for comment in review.comments {
                        expert_suggestions.push(format!(
                            "[{}] (Code Review) {}: {} - Suggestion: {}",
                            comment.severity,
                            comment.category,
                            comment.message,
                            comment.suggestion.as_deref().unwrap_or("N/A")
                        ));
                    }
                }
            }
        }

        // 将专家发现合并到报告中
        report.suggested_improvements.extend(expert_suggestions);

        Ok(report)
    }

    /// 批判性地分析反思报告，识别风险或盲点。
    pub async fn critique_reflection(
        &self,
        report: &ReflectionReport,
        prediction: &SimulationResult,
        // B3：超时秒数，来自 cognitive_loop 配置；0 表示禁用批判阶段。
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
            }
            Err(_) => {
                tracing::warn!(
                    "LLM critique timed out after {}s, using permissive default",
                    timeout.as_secs()
                );
                return Ok(permissive_critique("LLM timed out"));
            }
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
        self.intel
            .evolver
            .evolve(graph, report, prediction, insights)
            .await
    }
}
