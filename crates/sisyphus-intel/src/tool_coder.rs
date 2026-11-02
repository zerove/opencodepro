//! Sisyphus 的自主工具编码代理。

use sisyphus_tools_llm::LlmClient;
use sisyphus_types::tools::ToolSpec;
use std::sync::Arc;

/// 生成代码的入口签名（编译器的 ABI 适配层依赖它）。
pub const REQUIRED_ENTRY_POINT: &str = "pub fn run(input: &str) -> Result<String, String>";

/// Tool Coder.
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
        let prompt = format!(
            "You are a Senior Rust Engineer writing a WebAssembly tool for the Sisyphus \
             cognitive engine.\n\n\
             SPECIFICATION\n\
             ID: {}\n\
             Capability: {}\n\
             Expected Input (JSON schema): {}\n\
             Expected Output (JSON schema): {}\n\
             Success Criteria: {}\n\n\
             HARD REQUIREMENTS (violating any of these makes the tool unusable)\n\
             1. Output ONE complete Rust source file, inside a ```rust code block.\n\
             2. The file must define exactly this entry point:\n\
             \x20  `{REQUIRED_ENTRY_POINT}`\n\
             3. `run` receives the raw input string and returns the JSON string described by \
             the expected output schema. Return `Err(message)` for invalid input.\n\
             4. Use ONLY the Rust standard library. No `sisyphus_wasm_sdk`, no serde, no \
             external crates — the file is compiled with a bare `rustc` invocation.\n\
             5. Do NOT define `main`, `execute`, `#[no_mangle]`, or any `extern \"C\"` \
             function: the compiler appends the WASM ABI adapter itself.\n\
             6. Build JSON with `format!`/`String` manually. Escape quotes and control \
             characters in string values.\n\
             7. Never use `unwrap()` or `expect()`; propagate errors with `?` or `map_err`.\n\
             8. Keep it deterministic and side-effect free: no file, network, clock or \
             randomness access (the sandbox provides none of them).\n\n\
             The tool will be executed in a fuel-metered WASM sandbox with an empty JSON \
             object as the first probe input, so it must not panic on missing fields.",
            spec.tool_id,
            spec.required_capability,
            spec.expected_input,
            spec.expected_output,
            spec.success_criteria,
        );

        let mut prompt = prompt;
        if let Some(fb) = feedback {
            prompt.push_str(&format!(
                "\n\nPREVIOUS ATTEMPT FAILED. The verifier reported:\n{fb}\n\
                 Fix these problems and return the corrected file."
            ));
        }

        let response = self
            .llm_client
            .chat(
                "You are a Senior Rust Engineer specializing in WebAssembly (WASM) for the \
                 Sisyphus cognitive engine.",
                &prompt,
                false,
            )
            .await
            .map_err(|e| crate::IntelError::LlmError(e.to_string()))?;

        self.extract_code(&response)
    }

    /// 从 LLM 回复里取出 Rust 源码。
    ///
    /// 只接受 fenced code block 或纯源码；**不再**用「找第一个 `fn main`」兜底——
    /// 那会把解释性文字里的片段当成完整程序交给编译器。
    fn extract_code(&self, text: &str) -> Result<String, crate::IntelError> {
        if let Some(start) = text.find("```") {
            let rest = &text[start + 3..];
            // 围栏后的第一行是语言标记（```rust / ```Rust / ```），整行跳过。
            let body_start = rest.find('\n').ok_or_else(|| {
                crate::IntelError::Internal(
                    "Could not extract Rust code: code fence has no body".into(),
                )
            })?;
            let body = &rest[body_start + 1..];
            let end = body.find("```").ok_or_else(|| {
                crate::IntelError::Internal(
                    "Could not extract Rust code: unterminated code block".into(),
                )
            })?;
            let code = body[..end].trim();
            if code.is_empty() {
                return Err(crate::IntelError::Internal(
                    "Could not extract Rust code: the code block is empty".into(),
                ));
            }
            return Ok(code.to_string());
        }

        let trimmed = text.trim();
        if trimmed.contains("fn run(") {
            return Ok(trimmed.to_string());
        }

        Err(crate::IntelError::Internal(
            "Could not extract Rust code from LLM response: no code block and no `fn run`".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coder() -> ToolCoder {
        ToolCoder::new(Arc::new(sisyphus_tools_llm::metis::DummyLlmClient))
    }

    #[test]
    fn extracts_fenced_rust_block() {
        let text = "Here you go:\n```rust\npub fn run(i: &str) -> Result<String, String> { Ok(i.into()) }\n```\nDone.";
        let code = coder().extract_code(text).expect("code");
        assert!(code.starts_with("pub fn run"));
        assert!(!code.contains("```"));
    }

    #[test]
    fn extracts_bare_fence() {
        let text = "```\npub fn run(i: &str) -> Result<String, String> { Ok(i.into()) }\n```";
        assert!(coder().extract_code(text).is_ok());
    }

    #[test]
    fn accepts_unfenced_source_with_entry_point() {
        let text = "pub fn run(i: &str) -> Result<String, String> { Ok(i.into()) }";
        assert!(coder().extract_code(text).is_ok());
    }

    #[test]
    fn rejects_prose_without_code() {
        let err = coder().extract_code("I cannot help with that.").expect_err("must reject");
        assert!(err.to_string().contains("Could not extract"), "{err}");
    }

    #[test]
    fn rejects_empty_fence() {
        assert!(coder().extract_code("```rust\n\n```").is_err());
    }
}
