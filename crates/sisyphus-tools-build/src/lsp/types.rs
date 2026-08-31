//! Shared types for LSP integration.
//!
//! Minimal LSP protocol types (no lsp-types dependency) and the LspError enum.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── LSP Data Types ─────────────────────────────────────────

/// Position in a text document (0-based).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspPosition {
    pub line: u32,
    pub character: u32,
}

/// Range between two positions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspRange {
    pub start: LspPosition,
    pub end: LspPosition,
}

/// A diagnostic message from the language server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspDiagnostic {
    pub range: LspRange,
    #[serde(default)]
    pub severity: Option<i64>,
    #[serde(default)]
    pub message: String,
}

/// A location within a document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspLocation {
    pub uri: String,
    pub range: LspRange,
}

/// Hover information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspHover {
    pub contents: String,
}

/// A document symbol (function, class, variable, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspDocumentSymbol {
    pub name: String,
    pub kind: i64,
    pub range: LspRange,
    pub selection_range: LspRange,
    #[serde(default)]
    pub children: Option<Vec<LspDocumentSymbol>>,
}

/// A completion item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspCompletionItem {
    pub label: String,
    #[serde(default)]
    pub detail: Option<String>,
}

/// Convert severity integer to human-readable name.
pub fn severity_name(s: i64) -> &'static str {
    match s {
        1 => "error",
        2 => "warning",
        3 => "info",
        4 => "hint",
        _ => "?",
    }
}

// ── LSP Error ──────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
/// Lsp Error.
pub enum LspError {
    #[error("LSP transport error: {0}")]
    Transport(String),
    #[error("LSP protocol error: {0}")]
    Protocol(String),
    #[error("LSP server error (code {code}): {message}")]
    ServerError { code: i64, message: String },
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serde error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("Language server exited unexpectedly")]
    ServerDied,
}

pub type LspResult<T> = Result<T, LspError>;

/// Extract hover text from an LSP hover response contents (MarkupContent | MarkedString | MarkedString[]).
pub fn extract_hover_text(contents: &Value) -> String {
    match contents {
        Value::String(s) => s.clone(),
        Value::Object(map) => {
            if let Some(val) = map.get("value").and_then(|v| v.as_str()) {
                return val.to_string();
            }
            contents.to_string()
        },
        Value::Array(arr) => arr.iter().map(extract_hover_text).collect::<Vec<_>>().join("\n"),
        _ => contents.to_string(),
    }
}

/// Convert a file path to a file:// URI.
pub fn to_file_uri(path: &str) -> String {
    let p = std::path::Path::new(path);
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        let Ok(cwd) = std::env::current_dir() else {
            return format!("file://{path}");
        };
        cwd.join(path)
    };
    let canonical = abs.to_string_lossy().replace('\\', "/");
    if canonical.starts_with('/') {
        format!("file://{canonical}")
    } else {
        format!("file:///{canonical}")
    }
}
