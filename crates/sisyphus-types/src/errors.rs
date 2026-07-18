use thiserror::Error;

#[derive(Debug, Error)]
pub enum TypesError {
    #[error("Failed to read config file '{path}': {source}")]
    ConfigReadError {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("Failed to parse config file '{path}': {source}")]
    ConfigParseError {
        path: String,
        #[source]
        source: toml::de::Error,
    },
    #[error("State store error: {0}")]
    StateStoreError(String),
    #[error("Node execution error: {0}")]
    NodeExecutionError(String),
    #[error("Tool execution error for '{tool_id}': {details}")]
    ToolExecutionError { tool_id: String, details: String },
    #[error("Graph error: {0}")]
    GraphError(String),
    #[error("Config validation error: {0}")]
    ConfigValidationError(String),
    #[error("Internal system error: {0}")]
    InternalError(String),
}
