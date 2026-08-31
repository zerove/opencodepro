//! Code analysis: file analysis, directory inspections, code reviews.
//!
//! ## Sub-modules
//! - [`types`]: FileAnalysis, FunctionInfo, CodeIssue, IssueSeverity, ModuleSummary.

pub mod types;

pub use types::{CodeIssue, FileAnalysis, FunctionInfo, IssueSeverity, ModuleSummary};

use regex::Regex;
use sisyphus_tools_core::ToolResult;
use std::path::Path;

/// Analyze a single file: extract functions, imports, structs, detect issues.
pub fn analyze_file(path: &Path) -> ToolResult<FileAnalysis> {
    let content = sisyphus_tools_system::file::read(path)?;
    let lines: Vec<&str> = content.lines().collect();
    let line_count = lines.len();
    let char_count = content.len();

    let mut functions = vec![];
    let mut imports = vec![];
    let mut structs = vec![];
    let mut issues = vec![];

    let fn_re = Regex::new(r"(pub\s+)?(async\s+)?fn\s+(\w+)\s*\(([^)]*)\)\s*(->\s*([^{;]+))?")
        .map_err(|e| sisyphus_tools_core::ToolError::Generic(format!("fn regex failed: {e}")))?;
    let import_re = Regex::new(r"^use\s+(.+);$").map_err(|e| {
        sisyphus_tools_core::ToolError::Generic(format!("import regex failed: {e}"))
    })?;
    let struct_re = Regex::new(r"(pub\s+)?struct\s+(\w+)").map_err(|e| {
        sisyphus_tools_core::ToolError::Generic(format!("struct regex failed: {e}"))
    })?;
    let long_line_re = Regex::new(r"^.{120,}$").map_err(|e| {
        sisyphus_tools_core::ToolError::Generic(format!("long_line regex failed: {e}"))
    })?;
    let todo_re = Regex::new(r"(TODO|FIXME|HACK|XXX)")
        .map_err(|e| sisyphus_tools_core::ToolError::Generic(format!("todo regex failed: {e}")))?;
    let complexity_re = Regex::new(r"\b(if|for|while|match|loop)\b").map_err(|e| {
        sisyphus_tools_core::ToolError::Generic(format!("complexity regex failed: {e}"))
    })?;

    for (i, line) in lines.iter().enumerate() {
        let line_num = i + 1;

        // Match functions
        if let Some(caps) = fn_re.captures(line) {
            let is_pub = caps.get(1).is_some_and(|m| m.as_str().contains("pub"));
            let is_async = caps.get(2).is_some_and(|m| m.as_str().contains("async"));
            let name = caps.get(3).map_or("", |m| m.as_str()).to_string();
            let sig = caps.get(0).map_or("", |m| m.as_str()).to_string();

            // Basic complexity analysis: count control flow keywords until the end of the function
            let mut complexity = 0;
            let mut brace_depth = 0;
            let mut found_start = false;
            for (j, l) in lines[i..].iter().enumerate() {
                brace_depth += l.chars().filter(|&c| c == '{').count();
                brace_depth -= l.chars().filter(|&c| c == '}').count();
                if brace_depth > 0 || (j == 0 && l.contains('{')) {
                    found_start = true;
                    complexity += complexity_re.find_iter(l).count();
                }
                if found_start && brace_depth == 0 {
                    break;
                }
            }

            if complexity > 10 {
                issues.push(CodeIssue {
                    severity: IssueSeverity::Warning,
                    file: path.to_path_buf(),
                    line: line_num,
                    message: format!(
                        "Function '{}' has high complexity ({} branches).",
                        name, complexity
                    ),
                    code: Some(sig.clone()),
                });
            }

            functions.push(FunctionInfo {
                name,
                line: line_num,
                is_public: is_pub,
                is_async,
                signature: sig,
            });
        }

        // Match imports
        if let Some(caps) = import_re.captures(line) {
            imports.push(caps.get(1).map_or("", |m| m.as_str()).to_string());
        }

        // Match structs
        if let Some(caps) = struct_re.captures(line) {
            structs.push(caps.get(2).map_or("", |m| m.as_str()).to_string());
        }

        // Long lines
        if long_line_re.is_match(line) {
            issues.push(CodeIssue {
                severity: IssueSeverity::Style,
                file: path.to_path_buf(),
                line: line_num,
                message: format!("line too long ({} chars, max 120)", line.len()),
                code: Some(line.to_string()),
            });
        }

        // TODO/FIXME markers
        if todo_re.is_match(line) {
            issues.push(CodeIssue {
                severity: IssueSeverity::Info,
                file: path.to_path_buf(),
                line: line_num,
                message: "contains TODO/FIXME marker".to_string(),
                code: Some(line.to_string()),
            });
        }
    }

    Ok(FileAnalysis {
        path: path.to_path_buf(),
        line_count,
        char_count,
        functions,
        imports,
        structs,
        issues,
    })
}

/// Analyze all source files in a directory.
pub fn analyze_directory(root: &Path, include: Option<&str>) -> ToolResult<ModuleSummary> {
    let file_filter = include.unwrap_or(r"\.(rs|py|js|ts|go|md|toml|json|yaml)$");
    let result = sisyphus_tools_system::file::search_patterns(&[".+"], root, Some(file_filter), 0)?;

    let mut files = vec![];
    let mut total_lines = 0;

    let mut seen = std::collections::HashSet::new();
    for m in &result.matches {
        if seen.insert(m.file.clone()) {
            if let Ok(analysis) = analyze_file(&m.file) {
                total_lines += analysis.line_count;
                files.push(analysis);
            }
        }
    }

    Ok(ModuleSummary {
        path: root.to_path_buf(),
        file_count: files.len(),
        total_lines,
        files,
    })
}

/// Find potential issues across multiple files.
pub fn code_review(paths: &[&Path]) -> ToolResult<Vec<CodeIssue>> {
    let mut all_issues = vec![];
    for path in paths {
        if path.is_dir() {
            let summary = analyze_directory(path, Some(r"\.rs$"))?;
            for file in summary.files {
                all_issues.extend(file.issues);
            }
        } else if path.is_file() {
            let analysis = analyze_file(path)?;
            all_issues.extend(analysis.issues);
        }
    }
    Ok(all_issues)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn setup_file(dir: &TempDir, name: &str, content: &str) -> PathBuf {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn test_analyze_file_functions() {
        let dir = TempDir::new().unwrap();
        let file = setup_file(
            &dir,
            "test.rs",
            "\
fn private_fn(x: i32) -> i32 {
    x + 1
}

/// Public async fn.
pub async fn public_async_fn() -> String {
    \"hello\".to_string()
}

pub struct MyStruct {
    field: u32,
}
",
        );
        let analysis = analyze_file(&file).unwrap();
        assert_eq!(analysis.functions.len(), 2);
        assert_eq!(analysis.functions[0].name, "private_fn");
        assert!(!analysis.functions[0].is_public);
        assert!(!analysis.functions[0].is_async);

        assert_eq!(analysis.functions[1].name, "public_async_fn");
        assert!(analysis.functions[1].is_public);
        assert!(analysis.functions[1].is_async);

        assert_eq!(analysis.structs.len(), 1);
        assert_eq!(analysis.structs[0], "MyStruct");
    }

    #[test]
    fn test_analyze_file_imports() {
        let dir = TempDir::new().unwrap();
        let file = setup_file(
            &dir,
            "lib.rs",
            "\
use std::collections::HashMap;
use regex::Regex;

/// Run.
pub fn run() {}
",
        );
        let analysis = analyze_file(&file).unwrap();
        assert_eq!(analysis.imports.len(), 2);
        assert_eq!(analysis.imports[0], "std::collections::HashMap");
        assert_eq!(analysis.imports[1], "regex::Regex");
    }

    #[test]
    fn test_analyze_file_long_lines() {
        let dir = TempDir::new().unwrap();
        let long = "x".repeat(150);
        let file = setup_file(
            &dir,
            "long.rs",
            &format!("fn main() {{\n    println!(\"{}\");\n}}", long),
        );
        let analysis = analyze_file(&file).unwrap();
        let long_line_issues: Vec<_> =
            analysis.issues.iter().filter(|i| i.message.contains("line too long")).collect();
        assert_eq!(long_line_issues.len(), 1);
        assert_eq!(long_line_issues[0].severity, IssueSeverity::Style);
    }

    #[test]
    fn test_analyze_file_todo_marker() {
        let dir = TempDir::new().unwrap();
        let file = setup_file(
            &dir,
            "todo.rs",
            "\
// TODO: implement this later
fn foo() {}
// FIXME: this is broken
fn bar() {}
",
        );
        let analysis = analyze_file(&file).unwrap();
        let todo_issues: Vec<_> =
            analysis.issues.iter().filter(|i| i.message.contains("TODO/FIXME")).collect();
        assert_eq!(todo_issues.len(), 2);
    }

    #[test]
    fn test_analyze_directory() {
        let dir = TempDir::new().unwrap();
        setup_file(&dir, "src/main.rs", "fn main() { println!(\"hello\"); }");
        setup_file(
            &dir,
            "src/lib.rs",
            "pub fn add(a: i32, b: i32) -> i32 { a + b }",
        );
        setup_file(&dir, "README.md", "# Project");
        let summary = analyze_directory(dir.path(), Some(r"\.rs$")).unwrap();
        assert_eq!(summary.file_count, 2);
        assert!(summary.total_lines > 0);
    }

    #[test]
    fn test_code_review_single_file() {
        let dir = TempDir::new().unwrap();
        let file = setup_file(
            &dir,
            "src/lib.rs",
            "\
// TODO: need to implement
/// Add.
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
",
        );
        let paths = vec![file.as_path()];
        let issues = code_review(&paths).unwrap();
        let todo_count = issues.iter().filter(|i| i.message.contains("TODO/FIXME")).count();
        assert_eq!(todo_count, 1);
    }

    #[test]
    fn test_code_review_directory() {
        let dir = TempDir::new().unwrap();
        setup_file(
            &dir,
            "src/lib.rs",
            "\
// FIXME: fix this later
/// Run.
pub fn run() { loop {} }
",
        );
        let src_path = dir.path().join("src");
        let paths = vec![src_path.as_path()];
        if let Ok(issues) = code_review(&paths) {
            assert!(!issues.is_empty());
        }
    }

    #[test]
    fn test_analyze_file_line_count() {
        let dir = TempDir::new().unwrap();
        let content = "line1\nline2\nline3\n";
        let file = setup_file(&dir, "lines.rs", content);
        let analysis = analyze_file(&file).unwrap();
        assert_eq!(analysis.line_count, 3);
        assert_eq!(analysis.char_count, content.len());
    }

    #[test]
    fn test_analyze_file_empty() {
        let dir = TempDir::new().unwrap();
        let file = setup_file(&dir, "empty.rs", "");
        let analysis = analyze_file(&file).unwrap();
        assert_eq!(analysis.line_count, 0);
        assert_eq!(analysis.functions.len(), 0);
        assert_eq!(analysis.imports.len(), 0);
        assert_eq!(analysis.structs.len(), 0);
        assert_eq!(analysis.issues.len(), 0);
    }

    #[test]
    fn test_analyze_file_not_found() {
        let result = analyze_file(Path::new("/tmp/__sisyphus_nonexistent_oracle__"));
        assert!(result.is_err());
    }

    #[test]
    fn test_analyze_file_no_functions_or_imports() {
        let dir = TempDir::new().unwrap();
        let file = setup_file(
            &dir,
            "plain.txt",
            "just some text\nwith no code\nstructures",
        );
        let analysis = analyze_file(&file).unwrap();
        assert_eq!(analysis.functions.len(), 0);
        assert_eq!(analysis.imports.len(), 0);
        assert_eq!(analysis.structs.len(), 0);
    }

    #[test]
    fn test_analyze_directory_empty() {
        let dir = TempDir::new().unwrap();
        let summary = analyze_directory(dir.path(), Some(r"\.rs$")).unwrap();
        assert_eq!(summary.file_count, 0);
        assert_eq!(summary.total_lines, 0);
    }

    #[test]
    fn test_analyze_directory_no_matching_files() {
        let dir = TempDir::new().unwrap();
        setup_file(&dir, "data.json", "{\"key\": \"value\"}");
        setup_file(&dir, "readme.md", "# Readme");
        let summary = analyze_directory(dir.path(), Some(r"\.rs$")).unwrap();
        assert_eq!(summary.file_count, 0);
    }

    #[test]
    fn test_code_review_empty_paths() {
        let issues = code_review(&[]).unwrap();
        assert!(issues.is_empty());
    }

    #[test]
    fn test_code_review_nonexistent_path() {
        let path = Path::new("/tmp/__sisyphus_nonexistent_review__");
        let issues = code_review(&[path]).unwrap();
        assert!(issues.is_empty());
    }

    #[test]
    fn test_analyze_file_function_no_params() {
        let dir = TempDir::new().unwrap();
        let file = setup_file(
            &dir,
            "simple.rs",
            "fn hello() -> &'static str {\n    \"world\"\n}",
        );
        let analysis = analyze_file(&file).unwrap();
        assert_eq!(analysis.functions.len(), 1);
        assert_eq!(analysis.functions[0].name, "hello");
        assert!(!analysis.functions[0].is_public);
        assert!(!analysis.functions[0].is_async);
    }

    #[test]
    fn test_analyze_file_multi_line_import_not_matched() {
        let dir = TempDir::new().unwrap();
        let file = setup_file(
            &dir,
            "multi.rs",
            "\
use std::collections::{
    HashMap,
    VecDeque,
};
",
        );
        let analysis = analyze_file(&file).unwrap();
        assert_eq!(
            analysis.imports.len(),
            0,
            "multi-line imports not matched by simple regex"
        );
    }

    #[test]
    fn test_issue_severity_display() {
        assert_eq!(IssueSeverity::Warning.to_string(), "warning");
        assert_eq!(IssueSeverity::Style.to_string(), "style");
        assert_eq!(IssueSeverity::Info.to_string(), "info");
    }
}
