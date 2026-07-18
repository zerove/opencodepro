use async_trait::async_trait;
use serde_json::Value;
use sisyphus_tools_llm::LlmClient;
use sisyphus_types::{ExecutionContext, MicroReflectionDecision, MicroReflector, TypesError};
use std::collections::HashMap;
use std::sync::Arc;

pub struct LlmMicroReflector {
    llm: Arc<dyn LlmClient>,
}

impl LlmMicroReflector {
    pub fn new(llm: Arc<dyn LlmClient>) -> Self {
        Self { llm }
    }
}

#[async_trait]
impl MicroReflector for LlmMicroReflector {
    async fn predict(&self, node_name: &str, ctx: &ExecutionContext) -> Result<String, TypesError> {
        let prompt = format!(
            "You are a Runtime Cognitive Monitor. Before executing a node in the SOP, predict its expected output based on the current context.\n\n\
            Node: {}\n\
            Current Context Summary: {:?}\n\n\
            If this is a verification/test node, predict the exact success criteria (e.g., 'success: true, exit_code: 0'). Otherwise, provide a concise, one-sentence description of the expected successful result.",
            node_name, ctx.results
        );

        self.llm
            .generate(&prompt)
            .await
            .map_err(|e| TypesError::InternalError(e.to_string()))
    }

    async fn reflect(
        &self,
        node_name: &str,
        prediction: &str,
        actual: &str,
        ctx: &ExecutionContext,
    ) -> Result<MicroReflectionDecision, TypesError> {
        let prompt = format!(
            "You are a Runtime Cognitive Monitor. A node in the SOP has just executed.\n\n\
            Node: {}\n\
            Expected Output: {}\n\
            Actual Output: {}\n\
            Current Context Summary: {:?}\n\n\
            Analyze the deviation. \n\n\
            TDD MODE: If this is a verification/test node and the actual output indicates a failure (e.g., 'success: false' or contains 'ERROR'/'FAIL'), you MUST act as a TDD Engineer. Analyze the failure, identify the root cause from the logs, and return 'PIVOT: <diagnosis and fix suggestion>'.\n\n\
            Otherwise:\n\
            - If the actual output is logically consistent with the expectation, return 'CONTINUE'.\n\
            - If the output is slightly off but correctable by updating context variables, return 'ADJUST: {{ \"var\": \"val\", ... }}' as a JSON object.\n\
            - If the output indicates a fundamental failure or a need to change the plan, return 'PIVOT: <reason>'.",
            node_name, prediction, actual, ctx.results
        );

        let response = self
            .llm
            .generate(&prompt)
            .await
            .map_err(|e| TypesError::InternalError(e.to_string()))?;

        let trimmed = response.trim();
        let upper = trimmed.to_uppercase();

        if upper.starts_with("CONTINUE") {
            Ok(MicroReflectionDecision::Continue)
        } else if upper.starts_with("ADJUST") {
            let json_part = trimmed.strip_prefix("ADJUST:").unwrap_or(trimmed).trim();
            let updates: HashMap<String, Value> =
                serde_json::from_str(json_part).unwrap_or_else(|_| HashMap::new());
            Ok(MicroReflectionDecision::Adjust { updates })
        } else if upper.starts_with("PIVOT") {
            let reason = trimmed
                .strip_prefix("PIVOT:")
                .unwrap_or(trimmed)
                .trim()
                .to_string();
            Ok(MicroReflectionDecision::Pivot { reason })
        } else {
            Ok(MicroReflectionDecision::Continue)
        }
    }
}
