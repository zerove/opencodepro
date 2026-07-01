//! Session management for conversation history.
//!
//! Provides persistence, CRUD, compression, checkpointing, and export/import
//! of conversation sessions.
//!
//! ## Sub-modules
//! - [`types`]: Shared types (Message, Session, SessionStore, SessionCheckpoint, CompressStrategy).

pub mod types;

pub use types::{
    CompressStrategy, Message, Session, SessionCheckpoint,
};

use crate::{ToolError, ToolResult};
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

    let current_tokens = session.messages.iter().map(|m| m.estimated_tokens).sum::<u64>();

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
        if i + 1 < msgs.len() && msgs[i].role == "assistant" && msgs[i+1].role == "user" {
            let combined_content = format!(
                "Summary of assistant message: {}\nUser response: {}",
                msgs[i].content, msgs[i+1].content
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
        session.metadata.insert(format!("compressed_{idx}"), msg.content.clone());
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
    let db_dir = path.parent().ok_or_else(|| ToolError::TodoError("invalid session path".into()))?;
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
    let db_dir = path.parent().ok_or_else(|| ToolError::TodoError("invalid session path".into()))?;
    let checkpoint_path = db_dir.join("checkpoints").join(format!("{session_id}.json"));

    if !checkpoint_path.exists() {
        return Err(ToolError::TodoError(format!("checkpoint not found: {session_id}")));
    }

    let content = fs::read_to_string(checkpoint_path).map_err(ToolError::Io)?;
    serde_json::from_str(&content).map_err(|e| ToolError::TodoError(format!("deserialize checkpoint failed: {e}")))
}

/// Clears a session checkpoint by ID.
pub fn session_clear_checkpoint(path: &Path, session_id: &str) -> ToolResult<()> {
    let db_dir = path.parent().ok_or_else(|| ToolError::TodoError("invalid session path".into()))?;
    let checkpoint_path = db_dir.join("checkpoints").join(format!("{session_id}.json"));

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
        session.messages.iter().map(|m| m.estimated_tokens).sum::<u64>(),
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
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fresh_db() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().expect("create temp dir");
        let path = dir.path().join("sessions.json");
        (dir, path)
    }

    #[test]
    fn test_session_create_and_get() {
        let (_d, path) = fresh_db();
        let s = session_create(&path, "test session").unwrap();
        assert_eq!(s.name, "test session");
        assert_eq!(s.message_count, 0);
        assert!(s.messages.is_empty());

        let fetched = session_get(&path, &s.id).unwrap();
        assert_eq!(fetched.id, s.id);
    }

    #[test]
    fn test_session_list() {
        let (_d, path) = fresh_db();
        session_create(&path, "first").unwrap();
        session_create(&path, "second").unwrap();

        let list = session_list(&path).unwrap();
        assert_eq!(list.len(), 2);
        // newest first
        assert_eq!(list[0].name, "second");
        assert_eq!(list[1].name, "first");
    }

    #[test]
    fn test_session_list_empty() {
        let (_d, path) = fresh_db();
        let list = session_list(&path).unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn test_session_get_not_found() {
        let (_d, path) = fresh_db();
        let result = session_get(&path, "nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_session_delete() {
        let (_d, path) = fresh_db();
        let s = session_create(&path, "delete me").unwrap();
        session_delete(&path, &s.id).unwrap();
        let list = session_list(&path).unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn test_session_delete_not_found() {
        let (_d, path) = fresh_db();
        let result = session_delete(&path, "ghost");
        assert!(result.is_err());
    }

    #[test]
    fn test_session_add_message() {
        let (_d, path) = fresh_db();
        let s = session_create(&path, "chat").unwrap();
        let s = session_add_message(&path, &s.id, "user", "hello").unwrap();
        assert_eq!(s.message_count, 1);
        assert_eq!(s.messages.len(), 1);
        assert_eq!(s.messages[0].role, "user");
        assert_eq!(s.messages[0].content, "hello");

        let s = session_add_message(&path, &s.id, "assistant", "hi there").unwrap();
        assert_eq!(s.message_count, 2);
        assert_eq!(s.messages[1].role, "assistant");
    }

    #[test]
    fn test_session_add_message_not_found() {
        let (_d, path) = fresh_db();
        let result = session_add_message(&path, "nonexistent", "user", "hello");
        assert!(result.is_err());
    }

    #[test]
    fn test_session_rename() {
        let (_d, path) = fresh_db();
        let s = session_create(&path, "old name").unwrap();
        let s = session_rename(&path, &s.id, "new name").unwrap();
        assert_eq!(s.name, "new name");

        let fetched = session_get(&path, &s.id).unwrap();
        assert_eq!(fetched.name, "new name");
    }

    #[test]
    fn test_session_persistence() {
        let dir = TempDir::new().expect("create temp dir");
        let path = dir.path().join("sessions.json");

        // Create session in one instance
        let s = session_create(&path, "persist test").unwrap();
        session_add_message(&path, &s.id, "user", "stored!").unwrap();

        // Load from a fresh store (simulating restart)
        let fetched = session_get(&path, &s.id).unwrap();
        assert_eq!(fetched.name, "persist test");
        assert_eq!(fetched.message_count, 1);
        assert_eq!(fetched.messages[0].content, "stored!");
    }

    #[test]
    fn test_session_rename_not_found() {
        let (_d, path) = fresh_db();
        let result = session_rename(&path, "nonexistent", "new name");
        assert!(result.is_err());
    }

    #[test]
    fn test_session_add_message_empty_content() {
        let (_d, path) = fresh_db();
        let s = session_create(&path, "test").unwrap();
        let s = session_add_message(&path, &s.id, "user", "").unwrap();
        assert_eq!(s.message_count, 1);
        assert_eq!(s.messages[0].content, "");
    }

    #[test]
    fn test_session_serde_message_roundtrip() {
        let msg = Message {
            role: "user".into(),
            content: "hello".into(),
            created_at: "2026-01-01T00:00:00.000Z".into(),
            estimated_tokens: 0,
            is_compressed: false,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(back.role, "user");
        assert_eq!(back.content, "hello");
    }

    #[test]
    fn test_session_serde_session_roundtrip() {
        let mut metadata = HashMap::new();
        metadata.insert("key".into(), "val".into());
        let session = Session {
            id: "ses_1".into(),
            name: "test".into(),
            created_at: "2026-01-01T00:00:00.000Z".into(),
            updated_at: "2026-06-01T00:00:00.000Z".into(),
            message_count: 2,
            messages: vec![
                Message {
                    role: "user".into(),
                    content: "hi".into(),
                    created_at: "2026-01-01T00:00:00.000Z".into(),
                    estimated_tokens: 0,
                    is_compressed: false,
                },
                Message {
                    role: "assistant".into(),
                    content: "hello".into(),
                    created_at: "2026-01-01T00:00:00.000Z".into(),
                    estimated_tokens: 0,
                    is_compressed: false,
                },
            ],
            metadata,
        };
        let json = serde_json::to_string_pretty(&session).unwrap();
        let back: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "ses_1");
        assert_eq!(back.message_count, 2);
        assert_eq!(back.messages.len(), 2);
        assert_eq!(back.metadata.get("key").unwrap(), "val");
    }

    #[test]
    fn test_session_store_empty_not_exists() {
        let (_d, path) = fresh_db();
        let store = SessionStore::load_or_empty(&path);
        assert!(store.sessions.is_empty());
        assert_eq!(store.next_seq, 1);
    }

    #[test]
    fn test_session_fork() {
        let (_d, path) = fresh_db();
        let s1 = session_create(&path, "original").unwrap();
        let s1 = session_add_message(&path, &s1.id, "user", "hello").unwrap();
        let s1 = session_add_message(&path, &s1.id, "assistant", "hi").unwrap();

        let fork = session_fork(&path, &s1.id, "forked").unwrap();
        assert_eq!(fork.name, "forked");
        assert_eq!(fork.message_count, 2);
        assert_eq!(fork.messages[0].content, "hello");
        assert_eq!(fork.messages[1].content, "hi");
        // Fork has different ID and timestamps
        assert_ne!(fork.id, s1.id);
        assert!(fork.created_at >= s1.created_at);

        // Original unchanged
        let orig = session_get(&path, &s1.id).unwrap();
        assert_eq!(orig.message_count, 2);
    }

    #[test]
    fn test_session_fork_not_found() {
        let (_d, path) = fresh_db();
        let result = session_fork(&path, "nonexistent", "forked");
        assert!(result.is_err());
    }

    #[test]
    fn test_session_undo() {
        let (_d, path) = fresh_db();
        let s = session_create(&path, "chat").unwrap();
        let s = session_add_message(&path, &s.id, "user", "first").unwrap();
        let s = session_add_message(&path, &s.id, "assistant", "second").unwrap();
        assert_eq!(s.message_count, 2);

        let s = session_undo(&path, &s.id).unwrap();
        assert_eq!(s.message_count, 1);
        assert_eq!(s.messages[0].content, "first");

        // Undo again — empty
        let s = session_undo(&path, &s.id).unwrap();
        assert_eq!(s.message_count, 0);
        assert!(s.messages.is_empty());
    }

    #[test]
    fn test_session_undo_empty() {
        let (_d, path) = fresh_db();
        let s = session_create(&path, "empty").unwrap();
        let result = session_undo(&path, &s.id);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("nothing to undo"));
    }

    #[test]
    fn test_session_undo_not_found() {
        let (_d, path) = fresh_db();
        let result = session_undo(&path, "ghost");
        assert!(result.is_err());
    }

    #[test]
    fn test_session_token_estimation() {
        let (_d, path) = fresh_db();
        let s = session_create(&path, "tokens").unwrap();
        let s = session_add_message(&path, &s.id, "user", "hello world").unwrap();

        let count = session_estimate_token_count(&path, &s.id).unwrap();
        assert_eq!(count, estimate_tokens("hello world"));
        assert_eq!(s.messages[0].estimated_tokens, count);
    }

    #[test]
    fn test_session_compress() {
        let (_d, path) = fresh_db();
        let s = session_create(&path, "compress").unwrap();
        let s = session_add_message(&path, &s.id, "assistant", "msg 1").unwrap();
        let s = session_add_message(&path, &s.id, "user", "msg 2").unwrap();

        // Compress with low threshold to force compression
        let s = session_compress(&path, &s.id, &CompressStrategy::Threshold(0)).unwrap();

        assert_eq!(s.messages.len(), 1);
        assert!(s.messages[0].is_compressed);
        assert_eq!(s.messages[0].role, "system");
        assert!(s.metadata.contains_key("compressed_0"));
        assert!(s.metadata.contains_key("compressed_1"));
    }

    #[test]
    fn test_session_checkpoints() {
        let (_d, path) = fresh_db();
        let s = session_create(&path, "checkpoint").unwrap();

        let mut meta = HashMap::new();
        meta.insert("state".into(), "running".into());
        session_save_checkpoint(&path, &s.id, meta).unwrap();

        let cp = session_load_checkpoint(&path, &s.id).unwrap();
        assert_eq!(cp.session_id, s.id);
        assert_eq!(cp.metadata.get("state").unwrap(), "running");

        session_clear_checkpoint(&path, &s.id).unwrap();
        assert!(session_load_checkpoint(&path, &s.id).is_err());
    }

    #[test]
    fn test_session_checkpoint_auto_clear() {
        let (_d, path) = fresh_db();
        let s = session_create(&path, "auto-clear").unwrap();

        session_save_checkpoint(&path, &s.id, HashMap::new()).unwrap();
        assert!(session_load_checkpoint(&path, &s.id).is_ok());

        session_add_message(&path, &s.id, "user", "hi").unwrap();
        assert!(session_load_checkpoint(&path, &s.id).is_err());
    }

    #[test]
    fn test_session_export_import() {
        let (d1, path1) = fresh_db();
        let (_d2, path2) = fresh_db();

        let s = session_create(&path1, "export").unwrap();
        let s = session_add_message(&path1, &s.id, "user", "hello").unwrap();

        let export_path = d1.path().join("session_export.json");
        session_export(&path1, &s.id, &export_path).unwrap();

        let imported = session_import(&path2, &export_path).unwrap();
        assert_eq!(imported.id, s.id);
        assert_eq!(imported.name, "export");
        assert_eq!(imported.message_count, 1);

        let fetched = session_get(&path2, &s.id).unwrap();
        assert_eq!(fetched.id, s.id);
    }

    #[test]
    fn test_session_export_html_creates_valid_output() {
        let (d, path) = fresh_db();
        let s = session_create(&path, "html test").unwrap();
        let s = session_add_message(&path, &s.id, "user", "hello world").unwrap();
        let s = session_add_message(&path, &s.id, "assistant", "hi there").unwrap();
        let s = session_add_message(&path, &s.id, "system", "<script>alert('xss')</script>").unwrap();

        let html_path = d.path().join("export.html");
        session_export_html(&path, &s.id, &html_path).unwrap();

        assert!(html_path.exists(), "HTML file should exist");

        let content = std::fs::read_to_string(&html_path).unwrap();
        assert!(content.starts_with("<!DOCTYPE html>"), "should be valid HTML");
        assert!(content.contains("Session: html test"), "should contain session name");
        assert!(content.contains("hello world"), "should contain user message");
        assert!(content.contains("hi there"), "should contain assistant message");
        assert!(content.contains("&lt;script&gt;"), "should escape HTML in content (XSS)");
        assert!(content.contains("3 messages"), "should show message count in stats");
    }

    #[test]
    fn test_session_continue_not_found() {
        let (_d, path) = fresh_db();
        let result = session_continue(&path, "ghost");
        assert!(result.is_err());
    }
}
