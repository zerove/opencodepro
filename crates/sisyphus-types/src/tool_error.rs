//! Error type shared by the tool layer and its callers.
//!
//! This lives in `sisyphus-types` rather than `sisyphus-tools-core` so that
//! foundational crates can reference tool errors without depending on a tool
//! implementation crate. `sisyphus-tools-core` re-exports both names, so
//! existing paths keep resolving (ADR-014 phase 1).

use std::sync::Arc;

use thiserror::Error;

#[derive(Error, Debug)]
/// Tool Error.
pub enum ToolError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Generic error: {0}")]
    Generic(String),
    #[error("Path not found: {0}")]
    NotFound(String),
    /// The caller sent something the tool could not accept: a missing or
    /// malformed parameter, an unparseable value. Distinct from
    /// [`ToolError::Generic`] so a bad request reports as the caller's fault
    /// rather than an internal error.
    #[error("Invalid input: {0}")]
    InvalidInput(String),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_converts_to_generic() {
        let e: ToolError = "boom".to_string().into();
        assert!(matches!(e, ToolError::Generic(ref s) if s == "boom"));
    }

    #[test]
    fn io_error_converts_via_from() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        let e: ToolError = io.into();
        assert!(matches!(e, ToolError::Io(_)));
    }

    #[test]
    fn display_is_stable_for_diagnostics() {
        assert_eq!(
            ToolError::NotFound("a/b".into()).to_string(),
            "Path not found: a/b"
        );
    }
}

// ── MCP executor construction ────────────────────────────────────────────

/// Builds [`ToolExecutor`] instances for configured MCP servers.
///
/// This exists so `sisyphus-core` can wire up MCP servers without naming a
/// concrete executor type. The previous state had `core` calling
/// `sisyphus_tools_executor::executor_mcp::MCPToolExecutor::from_stdio` /
/// `::from_url` directly, which kept `sisyphus-tools-executor` in core's
/// production dependencies for what is really a wiring concern (ADR-014).
///
/// Implementations are provided by the tools layer; `sisyphus-types` only
/// knows the shape.
pub trait McpExecutorFactory: Send + Sync {
    /// Connect to an MCP server over stdio.
    /// `args` is owned rather than borrowed so the returned future does not have
    /// to carry the caller's borrow of a config string.
    fn connect_stdio(
        &self,
        command: String,
        args: Vec<String>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<Arc<dyn crate::ToolExecutor>, String>>
                + Send
                + '_,
        >,
    >;

    /// Connect to an MCP server over HTTP.
    fn connect_url(
        &self,
        url: String,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<Arc<dyn crate::ToolExecutor>, String>>
                + Send
                + '_,
        >,
    >;
}
