//! Integration tests for sisyphus-tools-llm-tools `oracle` types module.
//!
//! Exercises the pure data types `FileAnalysis`, `FunctionInfo`,
//! `CodeIssue`, `IssueSeverity`, and `ModuleSummary`.

use sisyphus_tools_llm_tools::oracle::{
    CodeIssue, FileAnalysis, FunctionInfo, IssueSeverity, ModuleSummary,
};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// FileAnalysis
// ---------------------------------------------------------------------------

#[test]
fn file_analysis_field_assignment() {
    let fa = FileAnalysis {
        path: PathBuf::from("/tmp/test.rs"),
        line_count: 100,
        char_count: 5000,
        functions: vec![],
        imports: vec!["use std::fs".into()],
        structs: vec!["MyStruct".into()],
        issues: vec![],
    };
    assert_eq!(fa.line_count, 100);
    assert_eq!(fa.char_count, 5000);
    assert_eq!(fa.imports.len(), 1);
    assert_eq!(fa.structs[0], "MyStruct");
}

#[test]
fn file_analysis_clone() {
    let fa = FileAnalysis {
        path: PathBuf::from("/p"),
        line_count: 1,
        char_count: 1,
        functions: vec![],
        imports: vec![],
        structs: vec![],
        issues: vec![],
    };
    let cloned = fa.clone();
    assert_eq!(cloned.line_count, 1);
    assert_eq!(cloned.path, fa.path);
}

#[test]
fn file_analysis_debug() {
    let fa = FileAnalysis {
        path: PathBuf::from("/p"),
        line_count: 0,
        char_count: 0,
        functions: vec![],
        imports: vec![],
        structs: vec![],
        issues: vec![],
    };
    let dbg = format!("{:?}", fa);
    assert!(dbg.contains("FileAnalysis"));
}

#[test]
fn file_analysis_empty() {
    let fa = FileAnalysis {
        path: PathBuf::from(""),
        line_count: 0,
        char_count: 0,
        functions: vec![],
        imports: vec![],
        structs: vec![],
        issues: vec![],
    };
    assert!(fa.functions.is_empty());
    assert!(fa.imports.is_empty());
    assert!(fa.structs.is_empty());
    assert!(fa.issues.is_empty());
}

#[test]
fn file_analysis_unicode_imports() {
    let fa = FileAnalysis {
        path: PathBuf::from("/p"),
        line_count: 1,
        char_count: 1,
        functions: vec![],
        imports: vec!["use crate::模块_α".into()],
        structs: vec![],
        issues: vec![],
    };
    assert_eq!(fa.imports[0], "use crate::模块_α");
}

// ---------------------------------------------------------------------------
// FunctionInfo
// ---------------------------------------------------------------------------

#[test]
fn function_info_field_assignment() {
    let f = FunctionInfo {
        name: "my_function".into(),
        line: 42,
        is_public: true,
        is_async: true,
        signature: "pub async fn my_function() -> u8".into(),
    };
    assert_eq!(f.name, "my_function");
    assert_eq!(f.line, 42);
    assert!(f.is_public);
    assert!(f.is_async);
}

#[test]
fn function_info_clone() {
    let f = FunctionInfo {
        name: "f".into(),
        line: 1,
        is_public: false,
        is_async: false,
        signature: "fn f()".into(),
    };
    let cloned = f.clone();
    assert_eq!(cloned.name, "f");
    assert!(!cloned.is_public);
}

#[test]
fn function_info_unicode_name() {
    let f = FunctionInfo {
        name: "函数_α".into(),
        line: 1,
        is_public: true,
        is_async: false,
        signature: "pub fn 函数_α()".into(),
    };
    assert_eq!(f.name, "函数_α");
}

// ---------------------------------------------------------------------------
// CodeIssue
// ---------------------------------------------------------------------------

#[test]
fn code_issue_field_assignment() {
    let issue = CodeIssue {
        severity: IssueSeverity::Warning,
        file: PathBuf::from("/file.rs"),
        line: 100,
        message: "line too long".into(),
        code: Some("E0001".into()),
    };
    assert_eq!(issue.line, 100);
    assert_eq!(issue.severity, IssueSeverity::Warning);
    assert_eq!(issue.code.as_deref(), Some("E0001"));
}

#[test]
fn code_issue_without_code() {
    let issue = CodeIssue {
        severity: IssueSeverity::Info,
        file: PathBuf::from("/f"),
        line: 1,
        message: "info".into(),
        code: None,
    };
    assert!(issue.code.is_none());
}

#[test]
fn code_issue_clone() {
    let issue = CodeIssue {
        severity: IssueSeverity::Style,
        file: PathBuf::from("/f"),
        line: 0,
        message: "m".into(),
        code: None,
    };
    let cloned = issue.clone();
    assert_eq!(cloned.severity, IssueSeverity::Style);
}

#[test]
fn code_issue_unicode_message() {
    let issue = CodeIssue {
        severity: IssueSeverity::Warning,
        file: PathBuf::from("/中文.rs"),
        line: 1,
        message: "警告:代码过长".into(),
        code: None,
    };
    assert_eq!(issue.message, "警告:代码过长");
}

// ---------------------------------------------------------------------------
// IssueSeverity
// ---------------------------------------------------------------------------

#[test]
fn issue_severity_equality() {
    assert_eq!(IssueSeverity::Warning, IssueSeverity::Warning);
    assert_ne!(IssueSeverity::Warning, IssueSeverity::Info);
    assert_ne!(IssueSeverity::Style, IssueSeverity::Info);
}

#[test]
fn issue_severity_clone() {
    let a = IssueSeverity::Style;
    let b = a.clone();
    assert_eq!(a, b);
}

#[test]
fn issue_severity_display() {
    assert_eq!(format!("{}", IssueSeverity::Warning), "warning");
    assert_eq!(format!("{}", IssueSeverity::Style), "style");
    assert_eq!(format!("{}", IssueSeverity::Info), "info");
}

#[test]
fn issue_severity_debug() {
    let _ = format!("{:?}", IssueSeverity::Warning);
    let _ = format!("{:?}", IssueSeverity::Style);
}

#[test]
fn issue_severity_distinct() {
    let variants = [
        IssueSeverity::Warning,
        IssueSeverity::Style,
        IssueSeverity::Info,
    ];
    for (i, a) in variants.iter().enumerate() {
        for (j, b) in variants.iter().enumerate() {
            if i == j {
                assert_eq!(a, b);
            } else {
                assert_ne!(a, b);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ModuleSummary
// ---------------------------------------------------------------------------

#[test]
fn module_summary_field_assignment() {
    let ms = ModuleSummary {
        path: PathBuf::from("/module"),
        file_count: 5,
        total_lines: 1000,
        files: vec![],
    };
    assert_eq!(ms.file_count, 5);
    assert_eq!(ms.total_lines, 1000);
}

#[test]
fn module_summary_clone() {
    let ms = ModuleSummary {
        path: PathBuf::from("/m"),
        file_count: 1,
        total_lines: 1,
        files: vec![],
    };
    let cloned = ms.clone();
    assert_eq!(cloned.file_count, 1);
}

#[test]
fn module_summary_unicode() {
    let ms = ModuleSummary {
        path: PathBuf::from("/模块-α"),
        file_count: 0,
        total_lines: 0,
        files: vec![],
    };
    assert_eq!(ms.path.to_str().unwrap(), "/模块-α");
}

#[test]
fn module_summary_with_files() {
    let fa = FileAnalysis {
        path: PathBuf::from("/file.rs"),
        line_count: 10,
        char_count: 100,
        functions: vec![],
        imports: vec![],
        structs: vec![],
        issues: vec![],
    };
    let ms = ModuleSummary {
        path: PathBuf::from("/m"),
        file_count: 1,
        total_lines: 10,
        files: vec![fa],
    };
    assert_eq!(ms.files.len(), 1);
    assert_eq!(ms.files[0].line_count, 10);
}
