use serde::{Deserialize, Serialize};
use sisyphus_tools_core::ToolResult;
use sisyphus_tools_llm::metis::LlmClient;
use std::sync::Arc;

/// A generated test with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedTest {
    pub fn_name: String,
    pub test_code: String,
    pub coverage_estimate: f64,
}

/// Generates Rust test code from function signatures.
///
/// If an LLM client is provided, it generates meaningful tests based on function bodies.
/// Otherwise, it falls back to producing basic test stubs.
pub struct TestGenerator {
    pub(crate) llm_client: Option<Arc<dyn LlmClient>>,
}

impl Default for TestGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl TestGenerator {
    /// New.
    pub fn new() -> Self {
        Self { llm_client: None }
    }

    /// With llm.
    pub fn with_llm(self, client: Arc<dyn LlmClient>) -> Self {
        Self {
            llm_client: Some(client),
        }
    }

    /// Extract the full body of a function from the source code.
    fn extract_function_body(&self, content: &str, fn_name: &str) -> Option<String> {
        let lines: Vec<&str> = content.lines().collect();
        let mut start_line = None;

        for (i, line) in lines.iter().enumerate() {
            if line.trim().contains(&format!("fn {fn_name}")) {
                start_line = Some(i);
                break;
            }
        }

        let start = start_line?;

        let mut brace_depth = 0;
        let mut found_start = false;
        let mut end_line = 0;

        for (i, line) in lines[start..].iter().enumerate() {
            for c in line.chars() {
                if c == '{' {
                    brace_depth += 1;
                    found_start = true;
                } else if c == '}' {
                    brace_depth -= 1;
                }
            }
            if found_start && brace_depth == 0 {
                end_line = start + i + 1;
                break;
            }
        }

        if end_line > 0 {
            Some(lines[start..end_line].join("\n"))
        } else {
            None
        }
    }

    async fn generate_meaningful_test(&self, code: &str) -> Result<String, String> {
        let prompt = format!(
            r#"Act as a Rust expert. Generate a high-quality unit test for the following function.
The test should be a complete Rust function with the `#[test]` or `#[tokio::test]` attribute.
Include realistic input values and assertions that verify the function's expected behavior.
Return ONLY the Rust code for the test function, without markdown backticks or explanations.

Function to test:
{code}"#
        );

        let Some(llm_client) = self.llm_client.as_ref() else {
            return Err("LLM client not available".to_string());
        };

        let response = llm_client
            .generate(&prompt)
            .await
            .map_err(|e| e.to_string())?;

        let cleaned = response
            .trim()
            .trim_start_matches("```rust")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();

        Ok(cleaned.to_string())
    }

    /// Generate a test for a specific function in a source file.
    pub async fn generate_for_function(
        &self,
        file_path: &str,
        fn_name: &str,
    ) -> ToolResult<GeneratedTest> {
        let content =
            std::fs::read_to_string(file_path).map_err(sisyphus_tools_core::ToolError::Io)?;

        // Try LLM generation if available
        if self.llm_client.is_some() {
            if let Some(body) = self.extract_function_body(&content, fn_name) {
                if let Ok(test_code) = self.generate_meaningful_test(&body).await {
                    return Ok(GeneratedTest {
                        fn_name: fn_name.to_string(),
                        test_code,
                        coverage_estimate: 0.8,
                    });
                }
            }
        }

        // Fallback to stub generation
        let mut found = false;
        let mut is_pub = false;
        let mut has_return = false;
        let mut param_count = 0;
        let mut is_async = false;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.contains(&format!("fn {fn_name}")) {
                found = true;
                is_pub = trimmed.starts_with("pub ") || trimmed.starts_with("pub(crate) ");
                is_async = trimmed.contains("async ");
                has_return = trimmed.contains("-> ");
                if let Some(start) = trimmed.find('(') {
                    if let Some(end) = trimmed[start..].find(')') {
                        let params = trimmed[start + 1..start + end].trim();
                        if params.is_empty()
                            || params == "self"
                            || params == "&self"
                            || params == "&mut self"
                        {
                            param_count = 0;
                        } else {
                            param_count = params.split(',').count();
                        }
                    }
                }
                break;
            }
        }

        if !found {
            return Err(sisyphus_tools_core::ToolError::Generic(format!(
                "Function '{fn_name}' not found in {file_path}"
            )));
        }

        let test_code = Self::build_test_code(fn_name, is_pub, is_async, has_return, param_count);
        let coverage_estimate = if param_count > 2 {
            0.3
        } else if param_count > 0 {
            0.5
        } else {
            0.8
        };

        Ok(GeneratedTest {
            fn_name: fn_name.to_string(),
            test_code,
            coverage_estimate,
        })
    }

    /// Generate tests for all public functions in a source file.
    pub async fn generate_for_module(&self, file_path: &str) -> ToolResult<Vec<GeneratedTest>> {
        let content =
            std::fs::read_to_string(file_path).map_err(sisyphus_tools_core::ToolError::Io)?;

        let mut tests = Vec::new();
        let mut current_fn: Option<String> = None;
        let mut current_is_pub = false;

        for line in content.lines() {
            let trimmed = line.trim();

            if trimmed.contains("fn ") {
                let fn_start = if let Some(pos) = trimmed.find("fn ") {
                    pos + 3
                } else {
                    continue;
                };
                let name_end = if let Some(pos) = trimmed[fn_start..].find('(') {
                    fn_start + pos
                } else {
                    continue;
                };
                let name = trimmed[fn_start..name_end].trim().to_string();
                if name.is_empty() || name == "fn" {
                    continue;
                }

                current_is_pub = trimmed.starts_with("pub ");

                if trimmed.contains('{') && trimmed.contains('}') {
                    if current_is_pub {
                        if let Ok(test) = self.generate_for_function(file_path, &name).await {
                            tests.push(test);
                        }
                    }
                } else {
                    current_fn = Some(name);
                }
            } else if trimmed.starts_with('#')
                || trimmed.starts_with("///")
                || trimmed.starts_with("//")
            {
                continue;
            } else if trimmed.starts_with('}') {
                if let Some(name) = current_fn.take() {
                    if current_is_pub {
                        if let Ok(test) = self.generate_for_function(file_path, &name).await {
                            tests.push(test);
                        }
                    }
                }
                current_fn = None;
            }
        }

        Ok(tests)
    }

    /// Write generated tests to a file.
    pub fn write_tests(tests: &[GeneratedTest], output_file: &str) -> ToolResult<()> {
        let mut content =
            String::from("// Auto-generated tests\n// Generated by TestGenerator\n\n");

        content.push_str("#[cfg(test)]\n");
        content.push_str("mod generated_tests {\n");
        content.push_str("    use super::*;\n\n");

        for test in tests {
            content.push_str(&test.test_code);
            content.push('\n');
        }

        content.push_str("}\n");

        std::fs::write(output_file, content).map_err(sisyphus_tools_core::ToolError::Io)?;

        Ok(())
    }

    fn build_test_code(
        fn_name: &str,
        is_pub: bool,
        is_async: bool,
        has_return: bool,
        param_count: usize,
    ) -> String {
        let test_name = format!("test_{fn_name}");
        let mut code = String::new();

        if is_async {
            code.push_str("    #[tokio::test]\n");
            code.push_str(&format!("    async fn {test_name}() {{\n"));
        } else {
            code.push_str("    #[test]\n");
            code.push_str(&format!("    fn {test_name}() {{\n"));
        }

        if !is_pub {
            code.push_str(&format!(
                "        // Note: {fn_name} is not public, testing via module access\n"
            ));
        }

        if param_count > 0 {
            code.push_str("        // TODO: Fill in actual parameters\n");
            code.push_str("        // let result = ");
            if is_async {
                code.push_str(&format!("{fn_name}(/* params */).await;\n"));
            } else {
                code.push_str(&format!("{fn_name}(/* params */);\n"));
            }

            if has_return {
                code.push_str("        // assert!(result.is_ok());\n");
            }
        } else {
            if is_async {
                code.push_str(&format!("        let result = {fn_name}().await;\n"));
            } else {
                code.push_str(&format!("        let result = {fn_name}();\n"));
            }

            if has_return {
                code.push_str("        // assert!(result.is_ok());\n");
            }
        }

        code.push_str("        // TODO: Add meaningful assertions\n");
        code.push_str("    }\n");

        code
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn create_temp_rs(content: &str) -> (tempfile::NamedTempFile, String) {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, "{}", content).unwrap();
        let path = f.path().to_string_lossy().to_string();
        (f, path)
    }

    #[tokio::test]
    async fn test_generate_for_function_found() {
        let code = "pub fn hello() -> String { \"hello\".into() }";
        let (_f, path) = create_temp_rs(code);
        let gen = TestGenerator::new();
        let test = gen.generate_for_function(&path, "hello").await.unwrap();
        assert_eq!(test.fn_name, "hello");
        assert!(test.test_code.contains("fn test_hello"));
        assert!(test.test_code.contains("result = hello()"));
        assert!(test.coverage_estimate > 0.0);
    }

    #[tokio::test]
    async fn test_generate_for_function_not_found() {
        let code = "fn existing() {}";
        let (_f, path) = create_temp_rs(code);
        let gen = TestGenerator::new();
        let result = gen.generate_for_function(&path, "nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_generate_for_function_with_params() {
        let code = "pub fn add(a: i32, b: i32) -> i32 { a + b }";
        let (_f, path) = create_temp_rs(code);
        let gen = TestGenerator::new();
        let test = gen.generate_for_function(&path, "add").await.unwrap();
        assert!(test.test_code.contains("TODO: Fill in actual parameters"));
        assert!(test.test_code.contains("/* params */"));
    }

    #[tokio::test]
    async fn test_generate_for_function_async() {
        let code =
            "pub async fn fetch(url: &str) -> Result<String, String> { Ok(\"data\".into()) }";
        let (_f, path) = create_temp_rs(code);
        let gen = TestGenerator::new();
        let test = gen.generate_for_function(&path, "fetch").await.unwrap();
        assert!(test.test_code.contains("#[tokio::test]"));
        assert!(test.test_code.contains("async fn test_fetch"));
        assert!(test.test_code.contains(".await"));
    }

    #[tokio::test]
    async fn test_generate_for_module() {
        let code = r#"
/// Foo.
pub fn foo() -> i32 { 42 }
/// Bar.
pub fn bar(x: i32) -> i32 { x }
fn hidden() {}
"#;
        let (_f, path) = create_temp_rs(code);
        let gen = TestGenerator::new();
        let tests = gen.generate_for_module(&path).await.unwrap();
        let names: Vec<String> = tests.iter().map(|t| t.fn_name.clone()).collect();
        assert!(names.contains(&"foo".to_string()));
        assert!(names.contains(&"bar".to_string()));
        assert!(!names.contains(&"hidden".to_string()));
    }

    #[tokio::test]
    async fn test_generate_for_module_empty() {
        let code = "fn hidden() {}";
        let (_f, path) = create_temp_rs(code);
        let gen = TestGenerator::new();
        let tests = gen.generate_for_module(&path).await.unwrap();
        assert!(tests.is_empty());
    }

    #[test]
    fn test_write_tests() {
        let dir = tempfile::tempdir().unwrap();
        let out_path = dir.path().join("generated_test.rs");

        let tests = vec![GeneratedTest {
            fn_name: "example".into(),
            test_code: "    #[test]\n    fn test_example() {\n        assert!(true);\n    }\n"
                .into(),
            coverage_estimate: 0.5,
        }];

        TestGenerator::write_tests(&tests, out_path.to_str().unwrap()).unwrap();
        let content = std::fs::read_to_string(&out_path).unwrap();
        assert!(content.contains("test_example"));
        assert!(content.contains("generated_tests"));
    }

    #[tokio::test]
    async fn test_private_function_note() {
        let code = "fn private_helper() -> i32 { 42 }";
        let (_f, path) = create_temp_rs(code);
        let gen = TestGenerator::new();
        let test = gen
            .generate_for_function(&path, "private_helper")
            .await
            .unwrap();
        assert!(test.test_code.contains("not public"));
    }
}
