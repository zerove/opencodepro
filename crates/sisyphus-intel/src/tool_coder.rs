//! Sisyphus 的自主工具编码代理。

use sisyphus_tools_llm::LlmClient;
use sisyphus_types::tools::ToolSpec;
use std::sync::Arc;

pub struct ToolCoder {
    llm_client: Arc<dyn LlmClient>,
}

impl ToolCoder {
    /// New.
    pub fn new(llm_client: Arc<dyn LlmClient>) -> Self {
        Self { llm_client }
    }

    /// 基于工具规范与以往失败的可选反馈，生成工具的 Rust 源代码。
    pub async fn generate_code(
        &self,
        spec: &ToolSpec,
        feedback: Option<String>,
    ) -> Result<String, crate::IntelError> {
        let mut prompt = format!(
            "You are a Senior Rust Engineer specializing in WebAssembly (WASM) for the Sisyphus cognitive engine.\n\n\
            TASK: Implement a tool based on the following specification.\n\n\
            SPECIFICATION:\n\
            ID: {}\n\
            Capability: {}\n\
            Expected Input: {}\n\
            Expected Output: {}\n\
            Success Criteria: {}\n\n\
            REQUIREMENTS:\n\
            1. The code must be a complete, standalone Rust file.\n\
            2. It must use the `sisyphus_wasm_sdk` crate for interfacing.\n\
            3. Implement the `SisyphusTool` trait.\n\
            4. Ensure strict error handling; do not use `unwrap()` or `expect()`.\n\
            5. The output must be a JSON string following the output schema.\n\
            6. Include a `main` function that registers the tool with the SDK.\n\n\
            Only return the Rust code inside a markdown code block.",
            spec.tool_id, spec.required_capability, spec.expected_input, spec.expected_output, spec.success_criteria
        );

        if let Some(fb) = feedback {
            prompt.push_str(&format!(
                "\n\nPREVIOUS ATTEMPT FAILED. FEEDBACK FROM VERIFIER:\n{}\n\
                Please analyze the errors and provide a corrected version of the code.",
                fb
            ));
        }

        let response = self.llm_client.chat("You are a Senior Rust Engineer specializing in WebAssembly (WASM) for the Sisyphus cognitive engine.", &prompt, false).await
            .map_err(|e| crate::IntelError::Internal(e.to_string()))?;

        // 从 markdown 块提取代码
        let code = self.extract_code(&response)?;
        Ok(code)
    }

    fn extract_code(&self, text: &str) -> Result<String, crate::IntelError> {
        if let Some(start) = text.find("```rust") {
            let rest = &text[start + 7..];
            if let Some(end) = rest.find("```") {
                return Ok(rest[..end].trim().to_string());
            }
        }

        // 兜底：若无 markdown 块，尝试找到第一个 { 与最后一个 }
        if let (Some(start), Some(end)) = (text.find("fn main"), text.rfind("}")) {
            return Ok(text[start..end + 1].to_string());
        }

        Err(crate::IntelError::Internal(
            "Could not extract Rust code from LLM response".into(),
        ))
    }
}
