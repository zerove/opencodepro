//! Session management for conversation history.
//!
//! Provides persistence, CRUD, compression, checkpointing, and export/import
//! of conversation sessions.
//!
//! ## Sub-modules
//! - [`types`]: Shared types (Message, Session, SessionStore, SessionCheckpoint, CompressStrategy).

pub mod types;

pub use types::{CompressStrategy, Message, Session, SessionCheckpoint};

use metrics::counter;
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

    let current_tokens = session.messages.iter().map(|m| m.estimated_tokens).sum::<u64>();

    match strategy {
        CompressStrategy::Threshold(limit) if current_tokens <= *limit => {
            return Ok(session.clone());
        },
        _ => {},
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
    let db_dir =
        path.parent().ok_or_else(|| ToolError::TodoError("invalid session path".into()))?;
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
    let db_dir =
        path.parent().ok_or_else(|| ToolError::TodoError("invalid session path".into()))?;
    let checkpoint_path = db_dir.join("checkpoints").join(format!("{session_id}.json"));

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
    let db_dir =
        path.parent().ok_or_else(|| ToolError::TodoError("invalid session path".into()))?;
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
///
/// 页面是**离线可打开**的：不引用外部 CSS/JS/字体。
pub fn session_export_html(path: &Path, id: &str, output: &Path) -> ToolResult<()> {
    let session = session_get(path, id)?;
    let html = render_session_html(&session);
    fs::write(output, html).map_err(ToolError::Io)
}

/// 把会话渲染为离线 HTML。
///
/// 结构：时间轴（每条消息带序号/时间点）+ 折叠块（工具调用、长消息）+
/// diff 着色 + 元数据中的 trace/sop 关联徽标。
pub fn render_session_html(session: &Session) -> String {
    let total = session.messages.len();
    let mut messages = String::new();
    for (idx, msg) in session.messages.iter().enumerate() {
        messages.push_str(&render_message_block(idx + 1, total, msg));
    }

    let tokens: u64 = session.messages.iter().map(|m| m.estimated_tokens).sum();
    let stats = format!("{total} messages, ~{tokens} tokens");
    let trace = render_trace_badges(session);
    let name = html_escape(&session.name);

    let mut html = String::with_capacity(messages.len() + CSS.len() + 1024);
    html.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"UTF-8\">\n");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">\n");
    html.push_str(&format!("<title>Session: {name}</title>\n<style>\n"));
    html.push_str(CSS);
    html.push_str("</style>\n</head>\n<body>\n<div class=\"container\">\n");
    html.push_str(&format!(
        "<div class=\"header\"><h1>{name}</h1><div class=\"meta\">ID: {} &middot; {stats} &middot; {}</div>{trace}</div>\n",
        html_escape(&session.id),
        html_escape(&session.updated_at),
    ));
    html.push_str("<div class=\"timeline\">\n");
    html.push_str(&messages);
    html.push_str("</div>\n");
    html.push_str(&format!(
        "<div class=\"footer\">Generated by Sisyphus &middot; {stats}</div>\n</div>\n</body>\n</html>"
    ));
    html
}

/// 单条消息的时间轴节点。
fn render_message_block(index: usize, total: usize, msg: &Message) -> String {
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
        "<span class=\"badge\">compressed</span>"
    } else {
        ""
    };
    let body = render_message_content(&msg.content);
    let body = if msg.content.chars().count() > LONG_MESSAGE_CHARS {
        format!(
            "<details class=\"long\"><summary>展开全文（{} 字符）</summary>{body}</details>",
            msg.content.chars().count()
        )
    } else {
        body
    };

    format!(
        "<div class=\"message {role_class}\" id=\"m{index}\">\n<div class=\"dot\"></div>\n<div class=\"header\"><span class=\"idx\">#{index}/{total}</span> {role_label} <span class=\"time\">{created_at}</span>{compressed_tag}</div>\n<div class=\"body\">{body}</div>\n</div>\n",
        created_at = html_escape(&msg.created_at),
    )
}

/// 渲染消息正文：转义 HTML，并把 fenced code / `<tool_call>` 转为结构化块。
fn render_message_content(content: &str) -> String {
    let mut out = String::new();
    let mut rest = content;
    loop {
        let fence = rest.find("```");
        let tool_tag = rest.find("<tool_call>");
        let next = match (fence, tool_tag) {
            (Some(f), Some(t)) if f <= t => Some((f, false)),
            (Some(f), None) => Some((f, false)),
            (Some(_), Some(t)) => Some((t, true)),
            (None, Some(t)) => Some((t, true)),
            (None, None) => None,
        };
        let Some((pos, is_tool)) = next else {
            out.push_str(&html_escape(rest));
            break;
        };
        out.push_str(&html_escape(&rest[..pos]));

        if is_tool {
            let after = &rest[pos + TOOL_TAG_OPEN.len()..];
            let Some(end) = after.find(TOOL_TAG_CLOSE) else {
                out.push_str(&html_escape(&rest[pos..]));
                break;
            };
            out.push_str(&render_tool_details(&html_escape(&after[..end])));
            rest = &after[end + TOOL_TAG_CLOSE.len()..];
            continue;
        }

        let after = &rest[pos + 3..];
        let lang_end = after.find('\n').unwrap_or(after.len());
        let lang = after[..lang_end].trim();
        let body_start = if lang_end < after.len() {
            lang_end + 1
        } else {
            lang_end
        };
        let body_and_rest = &after[body_start..];
        match body_and_rest.find("```") {
            Some(end) => {
                out.push_str(&render_code_block(lang, &body_and_rest[..end]));
                rest = &body_and_rest[end + 3..];
            },
            None => {
                // 未闭合的 fence：按代码块渲染剩余内容，避免把 ``` 当普通文本
                out.push_str(&render_code_block(lang, body_and_rest));
                break;
            },
        }
    }
    out
}

fn render_code_block(lang: &str, code: &str) -> String {
    let normalized = lang.to_ascii_lowercase();
    if normalized == "diff" {
        format!("<pre class=\"code diff\">{}</pre>", render_diff(code))
    } else if normalized == "tool" || normalized == "tool_call" {
        render_tool_details(&html_escape(code))
    } else {
        let attr = if normalized.is_empty() {
            String::new()
        } else {
            format!(" lang-{normalized}")
        };
        format!("<pre class=\"code{attr}\">{}</pre>", html_escape(code))
    }
}

fn render_tool_details(escaped_body: &str) -> String {
    format!(
        "<details class=\"tool\"><summary>🔧 tool call</summary><pre class=\"code\">{escaped_body}</pre></details>"
    )
}

/// diff 着色：`+` 行绿色、`-` 行红色、`@@` 行高亮，其余保持中性。
fn render_diff(code: &str) -> String {
    let mut out = String::new();
    for (i, line) in code.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let class = if line.starts_with("@@") {
            "hunk"
        } else if line.starts_with('+') && !line.starts_with("+++") {
            "add"
        } else if line.starts_with('-') && !line.starts_with("---") {
            "del"
        } else {
            "ctx"
        };
        out.push_str(&format!(
            "<span class=\"{class}\">{}</span>",
            html_escape(line)
        ));
    }
    out
}

/// 元数据中的 trace/sop 关联徽标（按 key 排序，保证输出稳定）。
fn render_trace_badges(session: &Session) -> String {
    let mut keys: Vec<(&String, &String)> = session
        .metadata
        .iter()
        .filter(|(k, _)| {
            let k = k.to_ascii_lowercase();
            k.contains("trace") || k.contains("sop")
        })
        .collect();
    keys.sort_by(|a, b| a.0.cmp(b.0));
    if keys.is_empty() {
        return String::new();
    }
    let mut out = String::from("<div class=\"trace\">");
    for (key, value) in keys {
        out.push_str(&format!(
            "<span class=\"chip\">{}: {}</span>",
            html_escape(key),
            html_escape(value)
        ));
    }
    out.push_str("</div>");
    out
}

/// 超过该字符数的消息折叠展示，避免单条超长消息淹没时间轴。
const LONG_MESSAGE_CHARS: usize = 1500;
const TOOL_TAG_OPEN: &str = "<tool_call>";
const TOOL_TAG_CLOSE: &str = "</tool_call>";

const CSS: &str = r#"
  * { margin: 0; padding: 0; box-sizing: border-box; }
  body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background: #0d1117; color: #c9d1d9; line-height: 1.6; }
  .container { max-width: 860px; margin: 0 auto; padding: 24px; }
  .header { border-bottom: 1px solid #30363d; padding-bottom: 16px; margin-bottom: 24px; }
  .header h1 { font-size: 1.5rem; color: #f0f6fc; }
  .header .meta { color: #8b949e; font-size: 0.85rem; margin-top: 4px; }
  .trace { margin-top: 8px; display: flex; flex-wrap: wrap; gap: 6px; }
  .chip { background: #1f2937; border: 1px solid #30363d; color: #79c0ff; border-radius: 999px; padding: 2px 10px; font-size: 0.75rem; }
  .timeline { border-left: 2px solid #30363d; margin-left: 10px; padding-left: 22px; }
  .message { position: relative; margin-bottom: 18px; padding: 12px 16px; border-radius: 8px; border: 1px solid #30363d; }
  .message .dot { position: absolute; left: -29px; top: 16px; width: 10px; height: 10px; border-radius: 50%; background: #58a6ff; border: 2px solid #0d1117; }
  .message.user { background: #161b22; border-left: 3px solid #58a6ff; }
  .message.user .dot { background: #58a6ff; }
  .message.assistant { background: #0d1117; border-left: 3px solid #3fb950; }
  .message.assistant .dot { background: #3fb950; }
  .message.system { background: #0d1117; border-left: 3px solid #d29922; }
  .message.system .dot { background: #d29922; }
  .message.other { background: #161b22; border-left: 3px solid #8b949e; }
  .message .header { font-size: 0.8rem; color: #8b949e; margin-bottom: 8px; border: none; padding: 0; }
  .message .header .idx { color: #6e7681; margin-right: 6px; font-variant-numeric: tabular-nums; }
  .message .header .badge { background: #30363d; padding: 1px 6px; border-radius: 4px; font-size: 0.75rem; margin-left: 6px; }
  .message .body { white-space: pre-wrap; word-break: break-word; font-size: 0.9rem; }
  .code { background: #010409; border: 1px solid #21262d; border-radius: 6px; padding: 10px 12px; margin: 8px 0; overflow-x: auto; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 0.82rem; white-space: pre; }
  .code.diff .add { color: #3fb950; display: block; }
  .code.diff .del { color: #f85149; display: block; }
  .code.diff .hunk { color: #79c0ff; display: block; }
  .code.diff .ctx { color: #8b949e; display: block; }
  details.tool { margin: 8px 0; border: 1px solid #21262d; border-radius: 6px; padding: 6px 10px; background: #0b1220; }
  details.tool summary { cursor: pointer; color: #79c0ff; font-size: 0.85rem; }
  details.long summary { cursor: pointer; color: #8b949e; font-size: 0.8rem; margin: 4px 0; }
  .footer { text-align: center; color: #484f58; font-size: 0.75rem; padding: 24px 0; border-top: 1px solid #30363d; margin-top: 24px; }
"#;

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
    counter!("sisyphus_session_created_total").increment(1);
    Ok(session)
}

/// List all sessions, newest first.
pub fn session_list(path: &Path) -> ToolResult<Vec<Session>> {
    let store = SessionStore::load_or_empty(path);
    let mut sessions: Vec<_> = store.sessions.into_values().collect();
    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then_with(|| b.id.cmp(&a.id)));
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
