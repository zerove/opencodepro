//! 生成工具的自主验证。
//!
//! 验证分两层，都不依赖「假装跑过测试」：
//!
//! 1. **静态层**（[`crate::wasm_validate`]）：产物是不是合法 WASM、
//!    是否导出沙箱要求的 `memory` + `execute`。这一层完全确定，失败即拒绝。
//! 2. **语义层**（LLM 审计）：源码是否实现了规范要求的能力。
//!
//! 行为正确性不在这里判定 —— 那需要真的在沙箱里跑一遍，由
//! `sisyphus-core` 的行为探针负责。早期实现把 `unit_tests_passed` 硬编码成
//! `true`、把 `check_sandbox_stability` 写成恒返回 `Ok(())`，等于自己给自己
//! 发合格证；这两个字段已被删除。

use serde::{Deserialize, Serialize};
use sisyphus_tools_llm::LlmClient;
use sisyphus_types::tools::ToolSpec;
use std::path::Path;
use std::sync::Arc;

use crate::wasm_validate;

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Verification Report.
pub struct VerificationReport {
    pub tool_name: String,
    /// 产物是否为合法 WASM 且满足沙箱导出契约（静态判定）。
    pub module_valid: bool,
    /// LLM 语义审计是否认为实现符合规范。
    pub semantic_match: bool,
    /// 0.0 = 模块不可用；0.5 = 模块可用但语义不符；1.0 = 两层都通过。
    pub confidence_score: f32,
    pub failures: Vec<String>,
}

impl VerificationReport {
    /// 是否具备部署资格。
    ///
    /// 只认可**两层都通过**：静态契约成立且语义审计达标。
    /// 单看 `confidence_score >= 0.7` 会让「语义不符」蒙混过关。
    pub fn is_deployable(&self) -> bool {
        self.module_valid && self.semantic_match
    }
}

/// Tool Verifier.
pub struct ToolVerifier {
    llm_client: Arc<dyn LlmClient>,
}

/// 语义审计的通过线。
const SEMANTIC_PASS_THRESHOLD: f32 = 0.8;

impl ToolVerifier {
    /// New.
    pub fn new(llm_client: Arc<dyn LlmClient>) -> Self {
        Self { llm_client }
    }

    /// 对照规范验证已编译的 WASM 工具。
    ///
    /// # Errors
    /// 仅在 LLM 调用失败时返回错误；静态校验失败会体现在
    /// `module_valid = false` + `failures` 里（这是可重试的生成问题，
    /// 不是基础设施故障）。
    pub async fn verify(
        &self,
        spec: &ToolSpec,
        code: &str,
        wasm_path: &Path,
    ) -> Result<VerificationReport, crate::IntelError> {
        let mut failures = Vec::new();

        // 1. 静态层：读产物做导出表校验。
        let module_valid = match tokio::fs::read(wasm_path).await {
            Ok(bytes) => match wasm_validate::inspect_wasm(&bytes) {
                Ok(info) => {
                    tracing::debug!(
                        tool_id = %spec.tool_id,
                        bytes = info.size_bytes,
                        exports = info.exports.len(),
                        "Generated module satisfies the sandbox ABI"
                    );
                    true
                },
                Err(reason) => {
                    failures.push(format!("Module contract violation: {reason}"));
                    false
                },
            },
            Err(e) => {
                failures.push(format!(
                    "Compiled artifact `{}` is unreadable: {e}",
                    wasm_path.display()
                ));
                false
            },
        };

        // 2. 语义层：让 LLM 对照规范审计源码。
        // 模块本身不可用时跳过 —— 语义再像也没有意义，而且省一次调用。
        let semantic_match = if module_valid {
            let score = self.audit_semantics(spec, code, &failures).await?;
            if score < SEMANTIC_PASS_THRESHOLD {
                failures.push(format!(
                    "Semantic audit score {score:.2} below {SEMANTIC_PASS_THRESHOLD:.1}: \
                     the implementation does not appear to fulfil the spec"
                ));
                false
            } else {
                true
            }
        } else {
            false
        };

        let confidence_score = match (module_valid, semantic_match) {
            (true, true) => 1.0,
            (true, false) => 0.5,
            _ => 0.0,
        };

        Ok(VerificationReport {
            tool_name: spec.tool_id.clone(),
            module_valid,
            semantic_match,
            confidence_score,
            failures,
        })
    }

    async fn audit_semantics(
        &self,
        spec: &ToolSpec,
        code: &str,
        failures: &[String],
    ) -> Result<f32, crate::IntelError> {
        let prompt = format!(
            "You are a Tool Auditor for the Sisyphus cognitive engine. Decide whether the \
             implementation fulfils the specification.\n\n\
             SPECIFICATION\n\
             ID: {}\n\
             Capability: {}\n\
             Expected Input: {}\n\
             Expected Output: {}\n\
             Success Criteria: {}\n\n\
             IMPLEMENTATION\n\
             ```rust\n{code}\n```\n\n\
             Known contract failures: {:?}\n\n\
             The entry point must be `pub fn run(input: &str) -> Result<String, String>` \
             returning a JSON string matching the expected output.\n\n\
             Reply with ONLY a decimal number between 0.0 and 1.0 expressing how well the \
             implementation matches the specification.",
            spec.tool_id,
            spec.required_capability,
            spec.expected_input,
            spec.expected_output,
            spec.success_criteria,
            failures
        );

        let response = self
            .llm_client
            .chat(
                "You are a Tool Auditor. You reply with a single number and nothing else.",
                &prompt,
                false,
            )
            .await
            .map_err(|e| crate::IntelError::LlmError(e.to_string()))?;

        Ok(parse_score(&response))
    }
}

/// 从 LLM 回复里取出第一个 0..=1 的浮点数。
///
/// 模型经常附带解释（"0.85 - looks correct"），早期实现直接
/// `trim().parse()` 会在这种回复上退化成 0.0，把好工具判死。
fn parse_score(response: &str) -> f32 {
    let mut current = String::new();
    let mut best: Option<f32> = None;

    for ch in response.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            current.push(ch);
        } else if !current.is_empty() {
            if let Ok(value) = current.parse::<f32>() {
                if (0.0..=1.0).contains(&value) {
                    best = Some(value);
                    break;
                }
            }
            current.clear();
        }
    }
    if best.is_none() && !current.is_empty() {
        if let Ok(value) = current.parse::<f32>() {
            if (0.0..=1.0).contains(&value) {
                best = Some(value);
            }
        }
    }

    best.unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_score_accepts_bare_number() {
        assert_eq!(parse_score("0.85"), 0.85);
    }

    #[test]
    fn parse_score_accepts_number_with_prose() {
        assert_eq!(parse_score("I would say 0.9 - it looks correct."), 0.9);
    }

    #[test]
    fn parse_score_rejects_out_of_range() {
        assert_eq!(parse_score("42"), 0.0);
    }

    #[test]
    fn parse_score_handles_garbage() {
        assert_eq!(parse_score("no idea"), 0.0);
    }

    #[test]
    fn deployable_requires_both_layers() {
        let report = |module_valid, semantic_match| VerificationReport {
            tool_name: "t".into(),
            module_valid,
            semantic_match,
            confidence_score: 0.5,
            failures: vec![],
        };
        assert!(report(true, true).is_deployable());
        assert!(!report(true, false).is_deployable());
        assert!(!report(false, true).is_deployable());
        assert!(!report(false, false).is_deployable());
    }
}
