use thiserror::Error;

/// Errors that can occur during team goal execution.
#[derive(Error, Debug)]
pub enum TeamError {
    /// No planner LLM is configured.
    #[error("No planner LLM configured – cannot decompose goal")]
    NoPlanner,

    /// LLM call failed.
    #[error("LLM call failed: {0}")]
    LlmFailed(String),

    /// The LLM returned an invalid or unparseable plan.
    #[error("Invalid plan from LLM: {0}")]
    InvalidPlan(String),

    /// The referenced agent is not registered.
    #[error("Agent '{0}' is not registered")]
    AgentNotFound(String),

    /// The tool is not supported by the assigned agent.
    #[error("Agent '{agent}' does not support tool '{tool}'")]
    ToolNotSupported { agent: String, tool: String },

    /// Task execution failed.
    #[error("Task execution error: {0}")]
    ExecutionError(String),

    /// Concurrency limit exceeded.
    #[error("Concurrency limit: {0}")]
    ConcurrencyLimit(String),
}

impl From<String> for TeamError {
    fn from(s: String) -> Self {
        TeamError::ExecutionError(s)
    }
}
