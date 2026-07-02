//! Shared types for the Oracle analysis module.

use std::path::PathBuf;

/// Result of analyzing a file: extracted structure and metadata.
#[derive(Debug, Clone)]
pub struct FileAnalysis {
    pub path: PathBuf,
    pub line_count: usize,
    pub char_count: usize,
    pub functions: Vec<FunctionInfo>,
    pub imports: Vec<String>,
    pub structs: Vec<String>,
    pub issues: Vec<CodeIssue>,
}

#[derive(Debug, Clone)]
pub struct FunctionInfo {
    pub name: String,
    pub line: usize,
    pub is_public: bool,
    pub is_async: bool,
    pub signature: String,
}

#[derive(Debug, Clone)]
pub struct CodeIssue {
    pub severity: IssueSeverity,
    pub file: PathBuf,
    pub line: usize,
    pub message: String,
    pub code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssueSeverity {
    Warning,
    Style,
    Info,
}

impl std::fmt::Display for IssueSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IssueSeverity::Warning => write!(f, "warning"),
            IssueSeverity::Style => write!(f, "style"),
            IssueSeverity::Info => write!(f, "info"),
        }
    }
}

/// Summary of a directory's module structure.
#[derive(Debug, Clone)]
pub struct ModuleSummary {
    pub path: PathBuf,
    pub file_count: usize,
    pub total_lines: usize,
    pub files: Vec<FileAnalysis>,
}
