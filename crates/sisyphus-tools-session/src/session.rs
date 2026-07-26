//! Session management for conversation history.
//!
//! Provides persistence, CRUD, compression, checkpointing, and export/import
//! of conversation sessions.
//!
//! ## Sub-modules
//! - [`types`]: Shared types (Message, Session, SessionStore, SessionCheckpoint, CompressStrategy).

pub mod types;

pub use types::{CompressStrategy, Message, Session, SessionCheckpoint};

use sisyphus_tools_core::{ToolError, ToolResult};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use types::{estimate_tokens, html_escape, now_iso, SessionStore};

/// Sums the estimated tokens of all messages in a session.
pub fn session_estimate_token_count(path: &Path, id: &str) -> ToolResult<u64> {
    let session = session_get(path, id)?;
    Ok(session.messages.iter().map(|m| m.estimated_tokens).sum())
}

/// Compresses a session by merging consecutive assistant+user message pairs into summaries.
///
/// If the strategy is `Threshold(limit)` and the current token count is below the limit,
/// the session is returned unchanged.
///
/// Compressed messages are marked with `is_compressed = true` and original content
/// is stored in `metadata` as `compressed_<idx>`.
pub fn session_compress(path: &Path, id: &str, strategy: &CompressStrategy) -> ToolResult<Session> {
    let mut store = SessionStore::load_or_empty(path);
    let session = store
        .sessions
        .get_mut(id)
        .ok_or_else(|| ToolError::TodoError(format!("session not found: {id}")))?;

    let current_tokens = session
        .messages
        .iter()
        .map(|m| m.estimated_tokens)
        .sum::<u64>();

    match strategy {
        CompressStrategy::Threshold(limit) if current_tokens <= *limit => {
            return Ok(session.clone());
        }
        _ => {}
    }

    let mut new_messages = Vec::new();
    let mut i = 0;
    let msgs = &session.messages;

    while i < msgs.len() {
        if i + 1 < msgs.len() && msgs[i].role == "assistant" && msgs[i + 1].role == "user" {
            let combined_content = format!(
                "Summary of assistant message: {}\nUser response: {}",
                msgs[i].content,
                msgs[i + 1].content
            );

            let compressed_msg = Message {
                role: "system".to_string(),
                content: format!("Compressed summary: {combined_content}"),
                created_at: now_iso(),
                estimated_tokens: estimate_tokens(&combined_content),
                is_compressed: true,
            };

            new_messages.push(compressed_msg);
            i += 2;
        } else {
            new_messages.push(msgs[i].clone());
            i += 1;
        }
    }

    // Store original content in metadata for recovery
    for (idx, msg) in msgs.iter().enumerate() {
        session
            .metadata
            .insert(format!("compressed_{idx}"), msg.content.clone());
    }

    session.messages = new_messages;
    session.message_count = session.messages.len() as u64;
    session.updated_at = now_iso();

    let result = session.clone();
    store.save(path)?;
    Ok(result)
}

/// Saves a session checkpoint to the checkpoints directory.
pub fn session_save_checkpoint(
    path: &Path,
    session_id: &str,
    metadata: HashMap<String, String>,
) -> ToolResult<()> {
    let db_dir = path
        .parent()
        .ok_or_else(|| ToolError::TodoError("invalid session path".into()))?;
    let checkpoint_dir = db_dir.join("checkpoints");
    fs::create_dir_all(&checkpoint_dir).map_err(ToolError::Io)?;

    let checkpoint_path = checkpoint_dir.join(format!("{session_id}.json"));
    let checkpoint = SessionCheckpoint {
        session_id: session_id.to_string(),
        last_message_idx: 0, // Default to 0, can be updated by caller if needed
        timestamp: now_iso(),
        metadata,
    };

    let content = serde_json::to_string_pretty(&checkpoint)
        .map_err(|e| ToolError::TodoError(format!("serialize checkpoint failed: {e}")))?;
    fs::write(checkpoint_path, content).map_err(ToolError::Io)
}

/// Loads a session checkpoint by ID.
pub fn session_load_checkpoint(path: &Path, session_id: &str) -> ToolResult<SessionCheckpoint> {
    let db_dir = path
        .parent()
        .ok_or_else(|| ToolError::TodoError("invalid session path".into()))?;
    let checkpoint_path = db_dir
        .join("checkpoints")
        .join(format!("{session_id}.json"));

    if !checkpoint_path.exists() {
        return Err(ToolError::TodoError(format!(
            "checkpoint not found: {session_id}"
        )));
    }

    let content = fs::read_to_string(checkpoint_path).map_err(ToolError::Io)?;
    serde_json::from_str(&content)
        .map_err(|e| ToolError::TodoError(format!("deserialize checkpoint failed: {e}")))
}

/// Clears a session checkpoint by ID.
pub fn session_clear_checkpoint(path: &Path, session_id: &str) -> ToolResult<()> {
    let db_dir = path
        .parent()
        .ok_or_else(|| ToolError::TodoError("invalid session path".into()))?;
    let checkpoint_path = db_dir
        .join("checkpoints")
        .join(format!("{session_id}.json"));

    if checkpoint_path.exists() {
        fs::remove_file(checkpoint_path).map_err(ToolError::Io)?;
    }
    Ok(())
}

/// Exports a session as a standalone JSON file.
pub fn session_export(path: &Path, id: &str, output: &Path) -> ToolResult<()> {
    let session = session_get(path, id)?;
    let content = serde_json::to_string_pretty(&session)
        .map_err(|e| ToolError::TodoError(format!("serialize failed: {e}")))?;
    fs::write(output, content).map_err(ToolError::Io)
}

/// Export a session as a self-contained HTML file for sharing in a browser.
pub fn session_export_html(path: &Path, id: &str, output: &Path) -> ToolResult<()> {
    let session = session_get(path, id)?;
    let mut message_html = String::new();

    for msg in &session.messages {
        let role_class = match msg.role.as_str() {
            "user" => "user",
            "assistant" | "model" => "assistant",
            "system" => "system",
            _ => "other",
        };
        let role_label = match msg.role.as_str() {
            "user" => "👤 User",
            "assistant" => "🤖 Assistant",
            "model" => "🧠 Model",
            "system" => "⚙️ System",
            other => other,
        };
        let compressed_tag = if msg.is_compressed {
            r#" <span class="badge">compressed</span>"#
        } else {
            ""
        };
        let content_escaped = msg
            .content
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");

        message_html.push_str(&format!(
            r#"<div class="message {role_class}">
            <div class="header">{role_label} <span class="time">{created_at}</span>{compressed_tag}</div>
            <div class="body">{content_escaped}</div>
            </div>
            "#,
            role_class = role_class,
            role_label = role_label,
            created_at = msg.created_at,
            compressed_tag = compressed_tag,
            content_escaped = content_escaped,
        ));
    }

    let stats = format!(
        "{} messages, ~{} tokens",
        session.message_count,
        session
            .messages
            .iter()
            .map(|m| m.estimated_tokens)
            .sum::<u64>(),
    );

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Session: {name}</title>
<style>
  * {{ margin: 0; padding: 0; box-sizing: border-box; }}
  body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background: #0d1117; color: #c9d1d9; line-height: 1.6; }}
  .container {{ max-width: 800px; margin: 0 auto; padding: 20px; }}
  .header {{ border-bottom: 1px solid #30363d; padding-bottom: 16px; margin-bottom: 24px; }}
  .header h1 {{ font-size: 1.5rem; color: #f0f6fc; }}
  .header .meta {{ color: #8b949e; font-size: 0.85rem; margin-top: 4px; }}
  .message {{ margin-bottom: 16px; padding: 12px 16px; border-radius: 8px; border: 1px solid #30363d; }}
  .message.user {{ background: #161b22; border-left: 3px solid #58a6ff; }}
  .message.assistant {{ background: #0d1117; border-left: 3px solid #3fb950; }}
  .message.system {{ background: #0d1117; border-left: 3px solid #d29922; }}
  .message.other {{ background: #161b22; border-left: 3px solid #8b949e; }}
  .message .header {{ font-size: 0.8rem; color: #8b949e; margin-bottom: 8px; border: none; padding: 0; }}
  .message .header .badge {{ background: #30363d; padding: 1px 6px; border-radius: 4px; font-size: 0.75rem; margin-left: 6px; }}
  .message .body {{ white-space: pre-wrap; word-break: break-word; font-size: 0.9rem; }}
  .footer {{ text-align: center; color: #484f58; font-size: 0.75rem; padding: 24px 0; border-top: 1px solid #30363d; margin-top: 24px; }}
</style>
</head>
<body>
<div class="container">
  <div class="header">
    <h1>{name}</h1>
    <div class="meta">ID: {id} &middot; {stats} &middot; {updated}</div>
  </div>
  {messages}
  <div class="footer">Generated by Sisyphus &middot; {stats}</div>
</div>
</body>
</html>"#,
        name = html_escape(&session.name),
        id = html_escape(&session.id),
        stats = stats,
        updated = session.updated_at,
        messages = message_html,
    );

    fs::write(output, html).map_err(ToolError::Io)
}

/// Imports a session from a standalone JSON file.
pub fn session_import(path: &Path, input: &Path) -> ToolResult<Session> {
    let content = fs::read_to_string(input).map_err(ToolError::Io)?;
    let session: Session = serde_json::from_str(&content)
        .map_err(|e| ToolError::TodoError(format!("deserialize failed: {e}")))?;

    let mut store = SessionStore::load_or_empty(path);
    store.sessions.insert(session.id.clone(), session.clone());
    store.save(path)?;
    Ok(session)
}

/// Create a new session with the given name.
pub fn session_create(path: &Path, name: &str) -> ToolResult<Session> {
    let mut store = SessionStore::load_or_empty(path);
    let id = format!("ses_{:x}", store.next_seq);
    store.next_seq += 1;
    let now = now_iso();
    let session = Session {
        id,
        name: name.to_string(),
        created_at: now.clone(),
        updated_at: now.clone(),
        message_count: 0,
        messages: Vec::new(),
        metadata: HashMap::new(),
        parent_session_id: None,
        child_session_ids: Vec::new(),
    };
    let id = session.id.clone();
    store.sessions.insert(id.clone(), session.clone());
    store.save(path)?;
    Ok(session)
}

/// List all sessions, newest first.
pub fn session_list(path: &Path) -> ToolResult<Vec<Session>> {
    let store = SessionStore::load_or_empty(path);
    let mut sessions: Vec<_> = store.sessions.into_values().collect();
    sessions.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| b.id.cmp(&a.id))
    });
    Ok(sessions)
}

/// Get a session by ID.
pub fn session_get(path: &Path, id: &str) -> ToolResult<Session> {
    let store = SessionStore::load_or_empty(path);
    store
        .sessions
        .get(id)
        .cloned()
        .ok_or_else(|| ToolError::TodoError(format!("session not found: {id}")))
}

/// Delete a session by ID.
pub fn session_delete(path: &Path, id: &str) -> ToolResult<()> {
    let mut store = SessionStore::load_or_empty(path);
    if store.sessions.remove(id).is_none() {
        return Err(ToolError::TodoError(format!("session not found: {id}")));
    }
    store.save(path)
}

/// Append a message to a session.
pub fn session_add_message(
    path: &Path,
    id: &str,
    role: &str,
    content: &str,
) -> ToolResult<Session> {
    let _ = session_clear_checkpoint(path, id);
    let mut store = SessionStore::load_or_empty(path);
    let session = store
        .sessions
        .get_mut(id)
        .ok_or_else(|| ToolError::TodoError(format!("session not found: {id}")))?;

    let msg = Message {
        role: role.to_string(),
        content: content.to_string(),
        created_at: now_iso(),
        estimated_tokens: estimate_tokens(content),
        is_compressed: false,
    };
    session.messages.push(msg);
    session.message_count = session.messages.len() as u64;
    session.updated_at = now_iso();
    let result = session.clone();
    store.save(path)?;
    Ok(result)
}

/// Fork a session — create a new session as a copy of an existing one.
/// All messages from the original are preserved in the new session.
pub fn session_fork(path: &Path, id: &str, new_name: &str) -> ToolResult<Session> {
    let mut store = SessionStore::load_or_empty(path);
    let original = store
        .sessions
        .get(id)
        .ok_or_else(|| ToolError::TodoError(format!("session not found: {id}")))?
        .clone();

    let new_id = format!("ses_{:x}", store.next_seq);
    store.next_seq += 1;
    let now = now_iso();
    let forked = Session {
        id: new_id,
        name: new_name.to_string(),
        created_at: now.clone(),
        updated_at: now.clone(),
        message_count: original.messages.len() as u64,
        messages: original.messages,
        metadata: original.metadata,
        parent_session_id: Some(id.to_string()),
        child_session_ids: Vec::new(),
    };
    let result = forked.clone();
    store.sessions.insert(forked.id.clone(), forked);
    store.save(path)?;
    Ok(result)
}

/// Continue a session — load the full message history as context for continuation.
/// Returns the session unchanged; this is a semantic operation that signals
/// "resuming from this session" to the caller.
pub fn session_continue(path: &Path, id: &str) -> ToolResult<Session> {
    if session_load_checkpoint(path, id).is_ok() {
        tracing::warn!("Session {id} has an existing checkpoint. Recovery may be needed.");
    }
    let mut store = SessionStore::load_or_empty(path);
    let session = store
        .sessions
        .get_mut(id)
        .ok_or_else(|| ToolError::TodoError(format!("session not found: {id}")))?;
    session.updated_at = now_iso();
    let result = session.clone();
    store.save(path)?;
    Ok(result)
}

/// Undo the last message in a session — removes it and decrements message count.
pub fn session_undo(path: &Path, id: &str) -> ToolResult<Session> {
    let mut store = SessionStore::load_or_empty(path);
    let session = store
        .sessions
        .get_mut(id)
        .ok_or_else(|| ToolError::TodoError(format!("session not found: {id}")))?;

    if session.messages.is_empty() {
        return Err(ToolError::TodoError(
            "nothing to undo — session is empty".into(),
        ));
    }

    session.messages.pop();
    session.message_count = session.messages.len() as u64;
    session.updated_at = now_iso();
    let result = session.clone();
    store.save(path)?;
    Ok(result)
}

/// Update a session's name.
pub fn session_rename(path: &Path, id: &str, name: &str) -> ToolResult<Session> {
    let mut store = SessionStore::load_or_empty(path);
    let session = store
        .sessions
        .get_mut(id)
        .ok_or_else(|| ToolError::TodoError(format!("session not found: {id}")))?;
    session.name = name.to_string();
    session.updated_at = now_iso();
    let result = session.clone();
    store.save(path)?;
    Ok(result)
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
