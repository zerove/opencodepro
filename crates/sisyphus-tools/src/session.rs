use std::collections::HashMap;
use std::fs;
use std::path::Path;
use serde::{Deserialize, Serialize};
use crate::{ToolError, ToolResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
    pub created_at: String,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionStore {
    sessions: HashMap<String, Session>,
    next_seq: u64,
}

impl SessionStore {
    fn empty() -> Self {
        Self { sessions: HashMap::new(), next_seq: 1 }
    }

    fn load_or_empty(path: &Path) -> Self {
        if path.exists() {
            fs::read_to_string(path)
                .ok()
                .and_then(|content| serde_json::from_str(&content).ok())
                .unwrap_or_else(Self::empty)
        } else {
            Self::empty()
        }
    }

    fn save(&self, path: &Path) -> ToolResult<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(ToolError::Io)?;
        }
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| ToolError::TodoError(format!("serialize failed: {e}")))?;
        fs::write(path, content).map_err(ToolError::Io)
    }
}

fn now_iso() -> String {
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
        b.updated_at.cmp(&a.updated_at)
            .then_with(|| b.id.cmp(&a.id))
    });
    Ok(sessions)
}

/// Get a session by ID.
pub fn session_get(path: &Path, id: &str) -> ToolResult<Session> {
    let store = SessionStore::load_or_empty(path);
    store.sessions.get(id).cloned()
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
pub fn session_add_message(path: &Path, id: &str, role: &str, content: &str) -> ToolResult<Session> {
    let mut store = SessionStore::load_or_empty(path);
    let session = store.sessions.get_mut(id)
        .ok_or_else(|| ToolError::TodoError(format!("session not found: {id}")))?;

    let msg = Message {
        role: role.to_string(),
        content: content.to_string(),
        created_at: now_iso(),
    };
    session.messages.push(msg);
    session.message_count = session.messages.len() as u64;
    session.updated_at = now_iso();
    let result = session.clone();
    store.save(path)?;
    Ok(result)
}

/// Update a session's name.
pub fn session_rename(path: &Path, id: &str, name: &str) -> ToolResult<Session> {
    let mut store = SessionStore::load_or_empty(path);
    let session = store.sessions.get_mut(id)
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
}
