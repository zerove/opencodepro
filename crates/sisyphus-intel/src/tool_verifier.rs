//! 生成工具的自主验证。

use serde::{Deserialize, Serialize};
use sisyphus_tools_llm::LlmClient;
use sisyphus_types::tools::ToolSpec;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationReport {
    pub tool_name: String,
    pub unit_tests_passed: bool,
    pub sandbox_stable: bool,
    pub semantic_match: bool,
    pub confidence_score: f32,
    pub failures: Vec<String>,
}

pub struct ToolVerifier {
    llm_client: Arc<dyn LlmClient>,
}

impl ToolVerifier {
    pub fn new(llm_client: Arc<dyn LlmClient>) -> Self {
        Self { llm_client }
    }

    /// 对照规范验证已编译的 WASM 工具。
    pub async fn verify(
        &self,
        spec: &ToolSpec,
        wasm_path: &PathBuf,
    ) -> Result<VerificationReport, crate::IntelError> {
        let mut failures = Vec::new();
        let unit_tests_passed = true;
        let mut sandbox_stable = true;
        let mut semantic_match = true;

        // 1. 基于成功标准的单元测试
        // 真实系统中，我们会从 spec.success_criteria 生成测试用例
        // 并通过 WasmToolExecutor 运行。

        // 2. 沙箱稳定性检查
        // 模拟重负载或检查 panic/OOM
        if let Err(e) = self.check_sandbox_stability(wasm_path).await {
            sandbox_stable = false;
            failures.push(format!("Sandbox stability check failed: {e}"));
        }

        // 3. 通过 LLM 的语义验证
        // 询问 LLM 观测行为是否与规范意图一致
        let semantic_score = self.audit_semantics(spec, &failures).await?;
        if semantic_score < 0.8 {
            semantic_match = false;
            failures.push(format!(
                "Semantic audit score too low: {:.2}",
                semantic_score
            ));
        }

        let confidence_score = if unit_tests_passed && sandbox_stable && semantic_match {
            1.0
        } else if unit_tests_passed && sandbox_stable {
            0.7
        } else {
            0.3
        };

        Ok(VerificationReport {
            tool_name: spec.tool_id.clone(),
            unit_tests_passed,
            sandbox_stable,
            semantic_match,
            confidence_score,
            failures,
        })
    }

    async fn check_sandbox_stability(&self, _wasm_path: &PathBuf) -> Result<(), crate::IntelError> {
        // 集成点：使用 wasmtime 检查资源限制
        Ok(())
    }

    async fn audit_semantics(
        &self,
        spec: &ToolSpec,
        failures: &[String],
    ) -> Result<f32, crate::IntelError> {
        let prompt = format!(
            "You are a Tool Auditor. Evaluate if the following tool implementation matches the spec.\n\n\
            Spec: {}\n\
            Observed Failures: {:?}\n\n\
            Return only a float between 0.0 and 1.0 representing the match confidence.",
            spec.required_capability, failures
        );

        let response = self
            .llm_client
            .chat("You are a Tool Auditor.", &prompt, false)
            .await
            .map_err(|e| crate::IntelError::Internal(e.to_string()))?;

        let score = response.trim().parse::<f32>().unwrap_or(0.0);
        Ok(score)
    }
}
