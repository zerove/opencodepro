use crate::ToolResult;
use serde::{Deserialize, Serialize};

/// A generated test with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedTest {
    pub fn_name: String,
    pub test_code: String,
    pub coverage_estimate: f64,
}

/// Generates Rust test code stubs from function signatures.
///
/// Uses line-based parsing to extract function information
/// and produces `#[test]` function stubs with basic assertions.
pub struct TestGenerator;

impl TestGenerator {
    /// Generate a test for a specific function in a source file.
    pub fn generate_for_function(file_path: &str, fn_name: &str) -> ToolResult<GeneratedTest> {
        let content = std::fs::read_to_string(file_path)
            .map_err(crate::ToolError::Io)?;

        // Find the function signature
        let mut found = false;
        let mut is_pub = false;
        let mut has_return = false;
        let mut param_count = 0;
        let mut is_async = false;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.contains(&format!("fn {}", fn_name)) {
                found = true;
                is_pub = trimmed.starts_with("pub ") || trimmed.starts_with("pub(crate) ");
                is_async = trimmed.contains("async ");
                has_return = trimmed.contains("-> ");
                // Count parameters (everything between parentheses)
                if let Some(start) = trimmed.find('(') {
                    if let Some(end) = trimmed[start..].find(')') {
                        let params = trimmed[start + 1..start + end].trim();
                        if params.is_empty() || params == "self" || params == "&self" || params == "&mut self" {
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
            return Err(crate::ToolError::Generic(format!(
                "Function '{}' not found in {}",
                fn_name, file_path
            )));
        }

        let test_code = Self::build_test_code(fn_name, is_pub, is_async, has_return, param_count);
        let coverage_estimate = if param_count > 2 { 0.3 } else if param_count > 0 { 0.5 } else { 0.8 };

        Ok(GeneratedTest {
            fn_name: fn_name.to_string(),
            test_code,
            coverage_estimate,
        })
    }

    /// Generate tests for all public functions in a source file.
    pub fn generate_for_module(file_path: &str) -> ToolResult<Vec<GeneratedTest>> {
        let content = std::fs::read_to_string(file_path)
            .map_err(crate::ToolError::Io)?;

        let mut tests = Vec::new();
        let mut current_fn: Option<String> = None;
        let mut current_is_pub = false;
        let mut current_is_async = false;
        let mut current_has_return = false;
        let mut current_param_count = 0;

        for line in content.lines() {
            let trimmed = line.trim();

            // Detect function definitions
            if trimmed.contains("fn ") {
                // Extract function name
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
                current_is_async = trimmed.contains("async ");
                current_has_return = trimmed.contains("-> ");

                // Count params
                if let Some(start) = trimmed.find('(') {
                    if let Some(end) = trimmed[start..].find(')') {
                        let params = trimmed[start + 1..start + end].trim();
                        if params.is_empty() || params == "self" || params == "&self" || params == "&mut self" {
                            current_param_count = 0;
                        } else {
                            current_param_count = params.split(',').count();
                        }
                    }
                }

                // Handle inline function bodies (e.g., `pub fn foo() -> i32 { 42 }`)
                if trimmed.contains('{') && trimmed.contains('}') {
                    if current_is_pub {
                        let test_code = Self::build_test_code(&name, true, current_is_async, current_has_return, current_param_count);
                        let coverage_estimate = if current_param_count > 2 { 0.3 } else if current_param_count > 0 { 0.5 } else { 0.8 };
                        tests.push(GeneratedTest {
                            fn_name: name,
                            test_code,
                            coverage_estimate,
                        });
                    }
                } else {
                    current_fn = Some(name);
                }
            } else if trimmed.starts_with('#') || trimmed.starts_with("///") || trimmed.starts_with("//") {
                // Skip attributes and comments
                continue;
            } else if trimmed.starts_with('}') {
                // End of function body
                if let Some(name) = current_fn.take() {
                    if current_is_pub {
                        let test_code = Self::build_test_code(&name, true, current_is_async, current_has_return, current_param_count);
                        let coverage_estimate = if current_param_count > 2 { 0.3 } else if current_param_count > 0 { 0.5 } else { 0.8 };
                        tests.push(GeneratedTest {
                            fn_name: name,
                            test_code,
                            coverage_estimate,
                        });
                    }
                }
                current_fn = None;
            }
        }

        Ok(tests)
    }

    /// Write generated tests to a file.
    pub fn write_tests(tests: &[GeneratedTest], output_file: &str) -> ToolResult<()> {
        let mut content = String::from("// Auto-generated tests\n// Generated by TestGenerator\n\n");

        // Add imports if needed
        content.push_str("#[cfg(test)]\n");
        content.push_str("mod generated_tests {\n");
        content.push_str("    use super::*;\n\n");

        for test in tests {
            content.push_str(&test.test_code);
            content.push('\n');
        }

        content.push_str("}\n");

        std::fs::write(output_file, content)
            .map_err(crate::ToolError::Io)?;

        Ok(())
    }

    fn build_test_code(
        fn_name: &str,
        is_pub: bool,
        is_async: bool,
        has_return: bool,
        param_count: usize,
    ) -> String {
        let test_name = format!("test_{}", fn_name);
        let mut code = String::new();

        if is_async {
            code.push_str("    #[tokio::test]\n");
            code.push_str(&format!("    async fn {}() {{\n", test_name));
        } else {
            code.push_str("    #[test]\n");
            code.push_str(&format!("    fn {}() {{\n", test_name));
        }

        if !is_pub {
            code.push_str(&format!(
                "        // Note: {} is not public, testing via module access\n",
                fn_name
            ));
        }

        if param_count > 0 {
            code.push_str("        // TODO: Fill in actual parameters\n");
            code.push_str("        // let result = ");
            if is_async {
                code.push_str(&format!("{}(/* params */).await;\n", fn_name));
            } else {
                code.push_str(&format!("{}(/* params */);\n", fn_name));
            }

            if has_return {
                code.push_str("        // assert!(result.is_ok());\n");
            }
        } else {
            if is_async {
                code.push_str(&format!("        let result = {}().await;\n", fn_name));
            } else {
                code.push_str(&format!("        let result = {}();\n", fn_name));
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

    #[test]
    fn test_generate_for_function_found() {
        let code = "pub fn hello() -> String { \"hello\".into() }";
        let (_f, path) = create_temp_rs(code);
        let test = TestGenerator::generate_for_function(&path, "hello").unwrap();
        assert_eq!(test.fn_name, "hello");
        assert!(test.test_code.contains("fn test_hello"));
        assert!(test.test_code.contains("result = hello()"));
        assert!(test.coverage_estimate > 0.0);
    }

    #[test]
    fn test_generate_for_function_not_found() {
        let code = "fn existing() {}";
        let (_f, path) = create_temp_rs(code);
        let result = TestGenerator::generate_for_function(&path, "nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_generate_for_function_with_params() {
        let code = "pub fn add(a: i32, b: i32) -> i32 { a + b }";
        let (_f, path) = create_temp_rs(code);
        let test = TestGenerator::generate_for_function(&path, "add").unwrap();
        assert!(test.test_code.contains("TODO: Fill in actual parameters"));
        assert!(test.test_code.contains("/* params */"));
    }

    #[test]
    fn test_generate_for_function_async() {
        let code = "pub async fn fetch(url: &str) -> Result<String, String> { Ok(\"data\".into()) }";
        let (_f, path) = create_temp_rs(code);
        let test = TestGenerator::generate_for_function(&path, "fetch").unwrap();
        assert!(test.test_code.contains("#[tokio::test]"));
        assert!(test.test_code.contains("async fn test_fetch"));
        assert!(test.test_code.contains(".await"));
    }

    #[test]
    fn test_generate_for_module() {
        let code = r#"
pub fn foo() -> i32 { 42 }
pub fn bar(x: i32) -> i32 { x }
fn hidden() {}
"#;
        let (_f, path) = create_temp_rs(code);
        let tests = TestGenerator::generate_for_module(&path).unwrap();
        let names: Vec<String> = tests.iter().map(|t| t.fn_name.clone()).collect();
        assert!(names.contains(&"foo".to_string()));
        assert!(names.contains(&"bar".to_string()));
        assert!(!names.contains(&"hidden".to_string()));
    }

    #[test]
    fn test_generate_for_module_empty() {
        let code = "fn hidden() {}";
        let (_f, path) = create_temp_rs(code);
        let tests = TestGenerator::generate_for_module(&path).unwrap();
        assert!(tests.is_empty());
    }

    #[test]
    fn test_write_tests() {
        let dir = tempfile::tempdir().unwrap();
        let out_path = dir.path().join("generated_test.rs");

        let tests = vec![GeneratedTest {
            fn_name: "example".into(),
            test_code: "    #[test]\n    fn test_example() {\n        assert!(true);\n    }\n".into(),
            coverage_estimate: 0.5,
        }];

        TestGenerator::write_tests(&tests, out_path.to_str().unwrap()).unwrap();
        let content = std::fs::read_to_string(&out_path).unwrap();
        assert!(content.contains("test_example"));
        assert!(content.contains("generated_tests"));
    }

    #[test]
    fn test_private_function_note() {
        let code = "fn private_helper() -> i32 { 42 }";
        let (_f, path) = create_temp_rs(code);
        let test = TestGenerator::generate_for_function(&path, "private_helper").unwrap();
        assert!(test.test_code.contains("not public"));
    }
}
