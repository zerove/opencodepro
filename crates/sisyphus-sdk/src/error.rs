//! SDK error types.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SdkError {
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("Server error: {status} - {message}")]
    ServerError { status: u16, message: String },

    #[error("Connection failed: {0}")]
    ConnectionError(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Invalid response: {0}")]
    InvalidResponse(String),
}
