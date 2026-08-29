//! `sisyphus-mem` 公共错误类型。
//!
//! 内部新代码优先使用 [`MemError`]；对外 API（`MfsStore::new` 等）暂时保留
//! `anyhow::Result` 以避免破坏 30+ 调用点。本模块提供 `From<MemError> for
//! anyhow::Error` 自动桥接，调用方可以渐进迁移。

use std::io;
use thiserror::Error;

#[derive(Debug, Error)]
/// Mem Error.
pub enum MemError {
    #[error("Invalid L1 capacity: {0}")]
    InvalidL1Capacity(String),

    #[error("Invalid SQLite URL '{url}': {source}")]
    InvalidSqliteUrl { url: String, #[source] source: sqlx::Error },

    #[error("Failed to create database parent dir '{path}': {source}")]
    CreateDbParent { path: String, #[source] source: io::Error },

    #[error("SQLite error in {context}")]
    Sqlx {
        context: &'static str,
        #[source]
        source: sqlx::Error,
    },

    #[error("IO error in {context}: {source}")]
    Io {
        context: &'static str,
        #[source]
        source: io::Error,
    },

    #[error("Serialization error in {context}: {source}")]
    Serde {
        context: &'static str,
        #[source]
        source: serde_json::Error,
    },

    #[error("Channel error in {context}: {message}")]
    Channel { context: &'static str, message: String },

    #[error("Inconsistent buffer state: {0}")]
    InconsistentState(String),

    #[error("Operation not allowed: {0}")]
    NotAllowed(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Skill market error: {0}")]
    SkillMarket(String),

    #[error("Experience bank error: {0}")]
    ExperienceBank(String),

    #[error("Vector memory error: {0}")]
    VectorMemory(String),

    #[error("User preferences error: {0}")]
    UserPreferences(String),

    #[error("Roadmap store error: {0}")]
    RoadmapStore(String),

    #[error("SOP store error: {0}")]
    SopStore(String),

    #[error("Manager error: {0}")]
    Manager(String),
}


impl From<sqlx::Error> for MemError {
    fn from(source: sqlx::Error) -> Self {
        MemError::Sqlx {
            context: "sqlx",
            source,
        }
    }
}

impl From<io::Error> for MemError {
    fn from(source: io::Error) -> Self {
        MemError::Io {
            context: "io",
            source,
        }
    }
}

impl From<serde_json::Error> for MemError {
    fn from(source: serde_json::Error) -> Self {
        MemError::Serde {
            context: "json",
            source,
        }
    }
}

pub type MemResult<T> = Result<T, MemError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mem_error_display_includes_context() {
        let err = MemError::InvalidL1Capacity("got 0".to_string());
        assert!(err.to_string().contains("Invalid L1 capacity"));
        assert!(err.to_string().contains("got 0"));
    }

    #[test]
    fn mem_error_from_io() {
        let io = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let err: MemError = io.into();
        assert!(matches!(err, MemError::Io { .. }));
    }

    #[test]
    fn mem_error_from_serde() {
        let json_err: serde_json::Error = serde_json::from_str::<i32>("not_a_number").unwrap_err();
        let err: MemError = json_err.into();
        assert!(matches!(err, MemError::Serde { .. }));
    }

    #[test]
    fn mem_error_to_anyhow_bridge() {
        let err = MemError::NotFound("session-42".to_string());
        let any: anyhow::Error = err.into();
        assert!(any.to_string().contains("session-42"));
    }

    #[test]
    fn mem_result_works_with_question_mark() {
        fn inner() -> MemResult<u32> {
            Ok(42)
        }
        fn outer() -> MemResult<u32> {
            let v = inner()?;
            Ok(v + 1)
        }
        assert_eq!(outer().expect("ok"), 43);
    }
}
