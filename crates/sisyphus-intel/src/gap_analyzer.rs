//! Gap analysis for autonomous tool generation.

use crate::reflect::ReflectionReport;
use serde::{Deserialize, Serialize};
use sisyphus_types::{tools::ToolSpec, SOPGraph};
use std::sync::Arc;
use std::time::Duration;

/// Maximum time to wait for the gap-analysis LLM call. Kept short so that an
/// unreachable LLM degrades gap detection to a no-gap result instead of
/// stalling the cognitive loop for the full HTTP client timeout (up to 60s).
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

    /// Analyzes the reflection report to see if a new tool is required.
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
            // Extract JSON from response
            let json_start = response.find('{').unwrap_or(0);
            let json_end = response.rfind('}').map(|i| i + 1).unwrap_or(response.len());
            let spec_json = &response[json_start..json_end];

            let spec: ToolSpec = serde_json::from_str(spec_json).map_err(|e| {
                crate::IntelError::Internal(format!("Failed to parse ToolSpec: {e}"))
            })?;

            // Find which node is failing
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
