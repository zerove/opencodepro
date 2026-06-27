use crate::ToolResult;

/// Generates documentation stubs for Rust source files.
///
/// Uses line-based parsing to extract function/trait/struct/enum signatures
/// and produces standardized doc comments.
pub struct AutoDocGenerator;

impl AutoDocGenerator {
    /// Generate a doc comment for a specific function in a file.
    pub fn generate_for_function(file_path: &str, fn_name: &str) -> ToolResult<String> {
        let content = std::fs::read_to_string(file_path).map_err(crate::ToolError::Io)?;

        let lines: Vec<&str> = content.lines().collect();
        let mut doc = String::new();
        let mut found = false;
        let mut i = 0;

        while i < lines.len() {
            let trimmed = lines[i].trim();

            // Look for "pub fn fn_name" or "fn fn_name"
            if trimmed.contains(&format!("fn {}", fn_name))
                && (trimmed.starts_with("pub")
                    || trimmed.starts_with("async")
                    || trimmed.starts_with("fn"))
            {
                found = true;

                // Collect the full signature (may span multiple lines)
                let mut signature = String::new();
                let mut brace_depth = 0;
                let mut j = i;

                while j < lines.len() {
                    let line = lines[j];
                    signature.push_str(line);
                    signature.push('\n');

                    for ch in line.chars() {
                        match ch {
                            '{' => brace_depth += 1,
                            '}' => brace_depth -= 1,
                            ';' if brace_depth == 0 => {
                                // End of signature (trait declaration)
                                break;
                            }
                            _ => {}
                        }
                    }

                    if brace_depth > 0 {
                        break; // We've entered the body, signature complete
                    }
                    j += 1;
                }

                // Parse the signature to extract parameters and return type
                let params = Self::extract_params(&signature);
                let return_type = Self::extract_return_type(&signature);

                doc.push_str(&format!("/// Executes the [`{}`] operation.\n", fn_name));
                if !params.is_empty() {
                    doc.push_str("///\n");
                    doc.push_str("/// # Parameters\n");
                    for (name, ty) in &params {
                        doc.push_str(&format!("/// - `{}`: {}\n", name, ty));
                    }
                }
                if let Some(rt) = &return_type {
                    doc.push_str("///\n");
                    doc.push_str(&format!("/// # Returns\n/// {}\n", rt));
                }
                doc.push_str("///\n");
                doc.push_str("/// # Auto-generated documentation stub.\n");

                break;
            }
            i += 1;
        }

        if !found {
            return Err(crate::ToolError::Generic(format!(
                "Function '{}' not found in {}",
                fn_name, file_path
            )));
        }

        Ok(doc)
    }

    /// Generate doc comments for all public items in a file.
    /// Returns (item_name, doc_string) pairs.
    pub fn generate_for_module(file_path: &str) -> ToolResult<Vec<(String, String)>> {
        let content = std::fs::read_to_string(file_path).map_err(crate::ToolError::Io)?;

        let mut results = Vec::new();

        // Find all public items (pub fn, pub struct, pub enum, pub trait)
        for line in content.lines() {
            let trimmed = line.trim();

            // Detect public items
            let item_name = if let Some(rest) = trimmed.strip_prefix("pub fn ") {
                Some(rest.split('(').next().unwrap_or("").trim().to_string())
            } else if let Some(rest) = trimmed.strip_prefix("pub struct ") {
                Some(
                    rest.split('<')
                        .next()
                        .unwrap_or("")
                        .split('{')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string(),
                )
            } else if let Some(rest) = trimmed.strip_prefix("pub enum ") {
                Some(
                    rest.split('<')
                        .next()
                        .unwrap_or("")
                        .split('{')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string(),
                )
            } else {
                trimmed.strip_prefix("pub trait ").map(|rest| {
                    rest.split('<')
                        .next()
                        .unwrap_or("")
                        .split('{')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string()
                })
            };

            if let Some(name) = item_name {
                if !name.is_empty() {
                    let doc = if trimmed.starts_with("pub fn ") {
                        let params = Self::extract_params(line);
                        let return_type = Self::extract_return_type(line);
                        let mut d = format!("/// The [`{}`] function.\n", name);
                        if !params.is_empty() {
                            d.push_str("///\n/// # Parameters\n");
                            for (p, t) in &params {
                                d.push_str(&format!("/// - `{}`: {}\n", p, t));
                            }
                        }
                        if let Some(rt) = &return_type {
                            d.push_str(&format!("///\n/// # Returns\n/// {}\n", rt));
                        }
                        d
                    } else {
                        format!(
                            "/// The [`{}`] type.\n///\n/// Auto-generated documentation stub.\n",
                            name
                        )
                    };
                    results.push((name, doc));
                }
            }
        }

        Ok(results)
    }

    /// Extract parameter names and types from a function signature.
    fn extract_params(signature: &str) -> Vec<(String, String)> {
        // Find the parameter list between the first ( and the matching { or ->
        let after_name = if let Some(pos) = signature.find('(') {
            &signature[pos + 1..]
        } else {
            return vec![];
        };

        let mut params_str = String::new();
        let mut depth = 1;
        for ch in after_name.chars() {
            if ch == ')' {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            if ch == '(' {
                depth += 1;
            }
            params_str.push(ch);
        }

        if params_str.trim().is_empty() || params_str.trim() == "self" {
            return vec![];
        }

        params_str
            .split(',')
            .filter_map(|p| {
                let p = p.trim();
                if p.is_empty() || p == "self" || p == "&self" || p == "&mut self" {
                    return None;
                }
                // Split on ": " to get name: type
                let colon_pos = p.rfind(':')?;
                let name = p[..colon_pos].trim().to_string();
                let ty = p[colon_pos + 1..].trim().to_string();
                Some((name, ty))
            })
            .collect()
    }

    /// Extract return type from a function signature.
    fn extract_return_type(signature: &str) -> Option<String> {
        // Find "->" before any {
        let sig = if let Some(pos) = signature.find('{') {
            &signature[..pos]
        } else {
            signature
        };

        if let Some(pos) = sig.rfind("->") {
            let ret = sig[pos + 2..].trim().to_string();
            if !ret.is_empty() && ret != "{" {
                Some(ret.trim_end_matches('{').trim().to_string())
            } else {
                None
            }
        } else {
            None
        }
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
        let code = r#"
pub fn hello(name: &str, count: u32) -> String {
    format!("Hello {}", name)
}
"#;
        let (_f, path) = create_temp_rs(code);
        let doc = AutoDocGenerator::generate_for_function(&path, "hello").unwrap();
        assert!(doc.contains("hello"));
        assert!(doc.contains("name"));
        assert!(doc.contains("count"));
        assert!(doc.contains("String"));
    }

    #[test]
    fn test_generate_for_function_not_found() {
        let code = "fn existing() {}";
        let (_f, path) = create_temp_rs(code);
        let result = AutoDocGenerator::generate_for_function(&path, "nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_generate_for_function_async() {
        let code = r#"
pub async fn fetch_data(url: &str) -> Result<String, Error> {
    Ok("data".into())
}
"#;
        let (_f, path) = create_temp_rs(code);
        let doc = AutoDocGenerator::generate_for_function(&path, "fetch_data").unwrap();
        assert!(doc.contains("fetch_data"));
        assert!(doc.contains("url"));
    }

    #[test]
    fn test_generate_for_module() {
        let code = r#"
pub fn foo(x: i32) -> i32 { x }
pub struct Bar { pub field: u32 }
pub enum Baz { A, B }
fn hidden() {}
"#;
        let (_f, path) = create_temp_rs(code);
        let results = AutoDocGenerator::generate_for_module(&path).unwrap();
        let names: Vec<String> = results.iter().map(|(n, _)| n.clone()).collect();
        assert!(names.contains(&"foo".to_string()));
        assert!(names.contains(&"Bar".to_string()));
        assert!(names.contains(&"Baz".to_string()));
        assert!(!names.contains(&"hidden".to_string()));
    }

    #[test]
    fn test_extract_params() {
        let sig = "fn test(a: i32, b: String, c: &[u8])";
        let params = AutoDocGenerator::extract_params(sig);
        assert_eq!(params.len(), 3);
        assert_eq!(params[0], ("a".to_string(), "i32".to_string()));
        assert_eq!(params[1], ("b".to_string(), "String".to_string()));
    }

    #[test]
    fn test_extract_params_empty() {
        assert!(AutoDocGenerator::extract_params("fn empty()").is_empty());
        assert!(AutoDocGenerator::extract_params("fn no_args() {}").is_empty());
    }

    #[test]
    fn test_extract_return_type() {
        let sig = "fn test() -> Result<String, Error>";
        let ret = AutoDocGenerator::extract_return_type(sig);
        assert_eq!(ret, Some("Result<String, Error>".to_string()));
    }

    #[test]
    fn test_extract_return_type_none() {
        let sig = "fn test()";
        assert!(AutoDocGenerator::extract_return_type(sig).is_none());
    }
}
