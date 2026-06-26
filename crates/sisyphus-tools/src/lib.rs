use thiserror::Error;

pub mod agent_registry;
pub mod bash;
pub mod build;
pub mod config_store;
pub mod delegate;
pub mod diagnose;
pub use diagnose::*;
pub mod executor;
pub mod executor_mcp;
pub mod explore;
pub mod feedback;
pub mod file;
pub mod git;
pub mod hook;
pub mod lsp;
pub mod llm;
pub mod librarian;
pub mod metis;
pub mod momus;
pub mod oracle;
pub mod patch;
pub mod permissions;
pub mod plan;
pub mod precommit;
pub use precommit::*;
pub mod project_rules;
pub mod scaffold;
pub use scaffold::*;
pub mod scribe;
pub mod security;
pub mod session;
pub mod skill;
pub mod todo;
pub mod web;

pub mod agent_channel;
pub mod architecture;
pub mod auto_doc;
pub mod auto_fix;
pub mod benchmark;
pub use benchmark::*;
pub mod bug_fixer;
pub mod circuit_breaker;
pub use circuit_breaker::*;
pub mod dep_manager;
pub mod mcp;
pub mod meta_learner;
pub mod model_router;
pub use model_router::*;
pub mod team;
pub mod test_gen;
pub mod tool_registry;

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

pub use agent_registry::*;
pub use bash::*;
pub use build::*;
pub use team::*;
pub use tool_registry::*;
pub use config_store::*;
pub use delegate::*;
pub use executor::*;
pub use executor_mcp::*;
pub use mcp::*;
pub use explore::*;
pub use feedback::*;
pub use file::*;
pub use git::*;
pub use hook::*;
pub use librarian::*;
pub use lsp::*;
pub use llm::*;
pub use metis::*;
pub use momus::*;
pub use oracle::*;
pub use patch::*;
pub use security::*;
pub use permissions::*;
pub use plan::*;
pub use project_rules::*;
pub use scribe::*;
pub use session::*;
pub use skill::*;
pub use todo::*;
pub use web::*;
