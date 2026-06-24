use thiserror::Error;

pub mod bash;
pub mod config_store;
pub mod delegate;
pub mod executor;
pub mod executor_mcp;
pub mod explore;
pub mod feedback;
pub mod file;
pub mod hook;
pub mod librarian;
pub mod lsp;
pub mod llm;
pub mod mcp;
pub mod metis;
pub mod momus;
pub mod oracle;
pub mod security;
pub mod permissions;
pub mod plan;
pub mod scribe;
pub mod session;
pub mod skill;
pub mod todo;
pub mod web;

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
    CommandFailed { code: i32, stdout: String, stderr: String },
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

pub use bash::*;
pub use config_store::*;
pub use delegate::*;
pub use executor::*;
pub use executor_mcp::*;
pub use explore::*;
pub use feedback::*;
pub use file::*;
pub use hook::*;
pub use librarian::*;
pub use lsp::*;
pub use llm::*;
pub use metis::*;
pub use momus::*;
pub use oracle::*;
pub use security::*;
pub use permissions::*;
pub use plan::*;
pub use scribe::*;
pub use session::*;
pub use skill::*;
pub use todo::*;
pub use web::*;
