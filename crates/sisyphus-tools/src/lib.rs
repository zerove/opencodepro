use thiserror::Error;

pub mod bash;
pub mod delegate;
pub mod explore;
pub mod file;
pub mod oracle;
pub mod plan;
pub mod mcp;
pub mod permissions;
pub mod session;
pub mod todo;
pub mod web;

#[derive(Error, Debug)]
pub enum ToolError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
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
    CommandFailed { code: i32, stdout: String, stderr: String },
}

pub type ToolResult<T> = Result<T, ToolError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Permission {
    Allow,
    Deny,
    Ask,
}

pub type PermissionRules = Vec<(String, Permission)>;

pub use bash::*;
pub use delegate::*;
pub use explore::*;
pub use file::*;
pub use oracle::*;
pub use plan::*;
pub use mcp::*;
pub use permissions::*;
pub use session::*;
pub use todo::*;
pub use web::*;
