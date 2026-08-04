//! 自主工具生成的缺口分析。

use crate::reflect::ReflectionReport;
use serde::{Deserialize, Serialize};
use sisyphus_types::{tools::ToolSpec, SOPGraph};
use std::sync::Arc;
use std::time::Duration;

/// 等待缺口分析 LLM 调用的最长时间。保持较短，使不可达的 LLM
/// 将缺口检测降级为"无缺口"结果，而不是
/// 让认知循环卡满整个 HTTP 客户端超时（最多 60 秒）。
const GAP_ANALYSIS_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolGap {
    pub gap_id: String,
    pub required_capability: String,
    pub failing_node: String,
    pub suggested_tool_spec: ToolSpec,
}

pub struct GapAnalyzer {
    llm_client: Arc<dyn sisyphus_tools_llm::LlmClient>,
}

impl GapAnalyzer {
    pub fn new(llm_client: Arc<dyn sisyphus_tools_llm::LlmClient>) -> Self {
        Self { llm_client }
    }

    /// 分析反思报告，判断是否需要新工具。
    pub async fn detect_gap(
        &self,
        report: &ReflectionReport,
        _sop_graph: &SOPGraph,
        goal_text: &str,
    ) -> Result<Option<ToolGap>, crate::IntelError> {
        let prompt = format!(
            "Analyze the following reflection report and decide if the failure is due to a missing tool.\n\n\
            Goal: {goal_text}\n\
            Reflection: {report:?}\n\n\
            If a new tool is needed, respond with 'GAP_FOUND' followed by a JSON ToolSpec with fields: \
            tool_id, required_capability, expected_input, expected_output, success_criteria.\n\
            If the issue can be solved by adjusting the SOP or using existing tools, respond with 'NO_GAP'.",
            goal_text = goal_text,
            report = report
        );

        let response = match tokio::time::timeout(
            GAP_ANALYSIS_TIMEOUT,
            self.llm_client
                .chat("You are a Sisyphus Gap Analyzer.", &prompt, true),
        )
        .await
        {
            Ok(Ok(resp)) => resp,
            Ok(Err(e)) => {
                tracing::warn!("LLM gap analysis failed, assuming no gap: {e}");
                return Ok(None);
            }
            Err(_) => {
                tracing::warn!(
                    timeout_ms = GAP_ANALYSIS_TIMEOUT.as_millis(),
                    "LLM gap analysis timed out, assuming no gap"
                );
                return Ok(None);
            }
        };

        if response.contains("GAP_FOUND") {
            // 从响应中提取 JSON
            let json_start = response.find('{').unwrap_or(0);
            let json_end = response.rfind('}').map(|i| i + 1).unwrap_or(response.len());
            let spec_json = &response[json_start..json_end];

            let spec: ToolSpec = serde_json::from_str(spec_json).map_err(|e| {
                crate::IntelError::Internal(format!("Failed to parse ToolSpec: {e}"))
            })?;

            // 找出哪个节点在失败
            let failing_node = report
                .suggested_improvements
                .first()
                .cloned()
                .unwrap_or_else(|| "unknown".to_string());

            Ok(Some(ToolGap {
                gap_id: format!("gap_{}", uuid::Uuid::new_v4()),
                required_capability: spec.required_capability.clone(),
                failing_node,
                suggested_tool_spec: spec,
            }))
        } else {
            Ok(None)
        }
    }
}
