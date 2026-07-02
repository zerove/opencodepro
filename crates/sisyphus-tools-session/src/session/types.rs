//! Shared types for the session system.

use sisyphus_tools_core::{ToolError, ToolResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// A single message in a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
    pub created_at: String,
    pub estimated_tokens: u64,
    pub is_compressed: bool,
}

/// A conversation session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub message_count: u64,
    pub messages: Vec<Message>,
    pub metadata: HashMap<String, String>,
}

/// On-disk persistence for sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct SessionStore {
    pub(super) sessions: HashMap<String, Session>,
    pub(super) next_seq: u64,
}

impl SessionStore {
    pub(super) fn empty() -> Self {
        Self {
            sessions: HashMap::new(),
            next_seq: 1,
        }
    }

    pub(super) fn load_or_empty(path: &Path) -> Self {
        if path.exists() {
            fs::read_to_string(path)
                .ok()
                .and_then(|content| serde_json::from_str(&content).ok())
                .unwrap_or_else(Self::empty)
        } else {
            Self::empty()
        }
    }

    pub(super) fn save(&self, path: &Path) -> ToolResult<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(ToolError::Io)?;
        }
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| ToolError::TodoError(format!("serialize failed: {e}")))?;
        fs::write(path, content).map_err(ToolError::Io)
    }
}

/// Checkpoint for session interrupt recovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCheckpoint {
    pub session_id: String,
    pub last_message_idx: usize,
    pub timestamp: String,
    pub metadata: HashMap<String, String>,
}

/// Strategy for session compression.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CompressStrategy {
    /// Compress if total token count exceeds the given threshold.
    Threshold(u64),
}

impl Default for CompressStrategy {
    fn default() -> Self {
        Self::Threshold(32000)
    }
}

// ── Private helpers ───────────────────────────────────────────────────────

pub(super) fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let millis = dur.as_millis() % 1000;
    let secs_in_day = secs % 86400;
    let hours = secs_in_day / 3600;
    let minutes = (secs_in_day % 3600) / 60;
    let seconds = secs_in_day % 60;

    let days_since_epoch = secs / 86400;
    let year = 1970 + (days_since_epoch as f64 / 365.25) as u64;
    let month = 1 + ((days_since_epoch % 365) as f64 / 30.44) as u64;
    let day = 1 + ((days_since_epoch % 365) as f64 % 30.44) as u64;

    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}.{millis:03}Z")
}

/// Simple heuristic for token estimation: 1 token ≈ 4 characters.
pub(super) fn estimate_tokens(text: &str) -> u64 {
    (text.len() as u64).div_ceil(4)
}

pub(super) fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
