#![allow(clippy::cast_precision_loss)]
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ToolError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Generic error: {0}")]
    Generic(String),
    #[error("Path not found: {0}")]
    NotFound(String),
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    #[error("Edit failed: no match for old string")]
    EditNoMatch,
    #[error("Edit failed: multiple matches")]
    EditAmbiguous,
    #[error("Glob pattern error: {0}")]
    GlobPattern(String),
    #[error("Grep error: {0}")]
    GrepError(String),
    #[error("Operation timed out: {0}")]
    Timeout(String),
    #[error("Web error: {0}")]
    WebError(String),
    #[error("Todo error: {0}")]
    TodoError(String),
    #[error("Bash command failed (exit {code}): {stderr}")]
    CommandFailed {
        code: i32,
        stdout: String,
        stderr: String,
    },
}

pub type ToolResult<T> = Result<T, ToolError>;

impl From<String> for ToolError {
    fn from(s: String) -> Self {
        ToolError::Generic(s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Permission {
    Allow,
    Deny,
    Ask,
}

pub type PermissionRules = Vec<(String, Permission)>;

pub mod hook;
pub mod feedback;
pub mod bridge;
