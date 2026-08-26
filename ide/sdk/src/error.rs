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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display_http() {
        let err = SdkError::ConnectionError("connection refused".into());
        assert!(err.to_string().contains("connection refused"));
    }

    #[test]
    fn test_error_display_server() {
        let err = SdkError::ServerError {
            status: 500,
            message: "boom".into(),
        };
        assert!(err.to_string().contains("500"));
        assert!(err.to_string().contains("boom"));
    }

    #[test]
    fn test_error_display_not_found() {
        let err = SdkError::NotFound("state_key".into());
        assert!(err.to_string().contains("state_key"));
    }

    #[test]
    fn test_error_display_invalid_response() {
        let err = SdkError::InvalidResponse("bad json".into());
        assert!(err.to_string().contains("bad json"));
    }

    #[test]
    fn test_json_error_conversion() {
        let json_err = serde_json::from_str::<serde_json::Value>("{invalid").unwrap_err();
        let sdk_err: SdkError = json_err.into();
        assert!(matches!(sdk_err, SdkError::JsonError(_)));
    }
}
