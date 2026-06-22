use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use serde::{Deserialize, Serialize};
use crate::{ToolError, ToolResult};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: String,
    pub content: String,
    pub status: TodoStatus,
    pub priority: TodoPriority,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TodoPriority {
    Low,
    Medium,
    High,
}

impl std::fmt::Display for TodoStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TodoStatus::Pending => write!(f, "pending"),
            TodoStatus::InProgress => write!(f, "in_progress"),
            TodoStatus::Completed => write!(f, "completed"),
            TodoStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl std::str::FromStr for TodoStatus {
    type Err = ToolError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(TodoStatus::Pending),
            "in_progress" => Ok(TodoStatus::InProgress),
            "completed" => Ok(TodoStatus::Completed),
            "cancelled" => Ok(TodoStatus::Cancelled),
            _ => Err(ToolError::TodoError(format!("invalid status: {s}"))),
        }
    }
}

impl std::fmt::Display for TodoPriority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TodoPriority::Low => write!(f, "low"),
            TodoPriority::Medium => write!(f, "medium"),
            TodoPriority::High => write!(f, "high"),
        }
    }
}

impl std::str::FromStr for TodoPriority {
    type Err = ToolError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "low" => Ok(TodoPriority::Low),
            "medium" => Ok(TodoPriority::Medium),
            "high" => Ok(TodoPriority::High),
            _ => Err(ToolError::TodoError(format!("invalid priority: {s}"))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TodoStore {
    items: HashMap<String, TodoItem>,
    next_seq: u64,
}

impl TodoStore {
    fn empty() -> Self {
        Self { items: HashMap::new(), next_seq: 1 }
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
            fs::create_dir_all(parent)
                .map_err(ToolError::Io)?;
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

pub fn todo_add(path: &Path, content: &str, priority: TodoPriority) -> ToolResult<TodoItem> {
    let mut store = TodoStore::load_or_empty(path);
    let id = format!("todo-{:x}", NEXT_ID.fetch_add(1, Ordering::SeqCst));
    let now = now_iso();
    let item = TodoItem {
        id: id.clone(),
        content: content.to_string(),
        status: TodoStatus::Pending,
        priority,
        created_at: now.clone(),
        updated_at: now,
    };
    store.items.insert(id.clone(), item.clone());
    store.save(path)?;
    Ok(item)
}

pub fn todo_list(path: &Path, status_filter: Option<TodoStatus>) -> ToolResult<Vec<TodoItem>> {
    let store = TodoStore::load_or_empty(path);
    let mut items: Vec<_> = store.items.into_values().collect();
    if let Some(ref status) = status_filter {
        items.retain(|i| i.status.to_string() == status.to_string());
    }
    items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(items)
}

pub fn todo_update(
    path: &Path,
    id: &str,
    content: Option<&str>,
    status: Option<TodoStatus>,
    priority: Option<TodoPriority>,
) -> ToolResult<TodoItem> {
    let mut store = TodoStore::load_or_empty(path);
    let item = store.items.get_mut(id)
        .ok_or_else(|| ToolError::TodoError(format!("todo not found: {id}")))?;

    if let Some(c) = content {
        item.content = c.to_string();
    }
    if let Some(s) = status {
        item.status = s;
    }
    if let Some(p) = priority {
        item.priority = p;
    }
    item.updated_at = now_iso();
    let result = item.clone();
    store.save(path)?;
    Ok(result)
}

pub fn todo_delete(path: &Path, id: &str) -> ToolResult<()> {
    let mut store = TodoStore::load_or_empty(path);
    if store.items.remove(id).is_none() {
        return Err(ToolError::TodoError(format!("todo not found: {id}")));
    }
    store.save(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fresh_db() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().expect("create temp dir");
        let path = dir.path().join("todos.json");
        (dir, path)
    }

    #[test]
    fn test_todo_add_and_list() {
        let (_d, path) = fresh_db();
        let item = todo_add(&path, "test task", TodoPriority::High).unwrap();
        assert_eq!(item.content, "test task");
        assert!(matches!(item.status, TodoStatus::Pending));
        assert_eq!(item.priority.to_string(), "high");

        let items = todo_list(&path, None).unwrap();
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn test_todo_add_low_priority() {
        let (_d, path) = fresh_db();
        let item = todo_add(&path, "low task", TodoPriority::Low).unwrap();
        assert_eq!(item.priority.to_string(), "low");
    }

    #[test]
    fn test_todo_list_filter_by_status() {
        let (_d, path) = fresh_db();
        todo_add(&path, "task 1", TodoPriority::Medium).unwrap();
        let item2 = todo_add(&path, "task 2", TodoPriority::Medium).unwrap();
        todo_update(&path, &item2.id, None, Some(TodoStatus::Completed), None).unwrap();

        let pending = todo_list(&path, Some(TodoStatus::Pending)).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].content, "task 1");

        let completed = todo_list(&path, Some(TodoStatus::Completed)).unwrap();
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].content, "task 2");
    }

    #[test]
    fn test_todo_list_empty_db() {
        let (_d, path) = fresh_db();
        let items = todo_list(&path, None).unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn test_todo_update_content_and_status() {
        let (_d, path) = fresh_db();
        let item = todo_add(&path, "old content", TodoPriority::Low).unwrap();

        let updated = todo_update(
            &path,
            &item.id,
            Some("new content"),
            Some(TodoStatus::InProgress),
            None,
        )
        .unwrap();
        assert_eq!(updated.content, "new content");
        assert!(matches!(updated.status, TodoStatus::InProgress));

        // Verify persistence
        let items = todo_list(&path, None).unwrap();
        let found = items.iter().find(|i| i.id == item.id).unwrap();
        assert_eq!(found.content, "new content");
    }

    #[test]
    fn test_todo_update_not_found() {
        let (_d, path) = fresh_db();
        let result = todo_update(&path, "nonexistent", None, None, None);
        assert!(matches!(result.unwrap_err(), ToolError::TodoError(_)));
    }

    #[test]
    fn test_todo_delete() {
        let (_d, path) = fresh_db();
        let item = todo_add(&path, "delete me", TodoPriority::Medium).unwrap();
        todo_delete(&path, &item.id).unwrap();
        let items = todo_list(&path, None).unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn test_todo_delete_not_found() {
        let (_d, path) = fresh_db();
        let result = todo_delete(&path, "ghost");
        assert!(matches!(result.unwrap_err(), ToolError::TodoError(_)));
    }

    #[test]
    fn test_todo_status_display_and_parse() {
        assert_eq!(TodoStatus::Pending.to_string(), "pending");
        assert_eq!(TodoStatus::InProgress.to_string(), "in_progress");
        assert_eq!(TodoStatus::Completed.to_string(), "completed");
        assert_eq!(TodoStatus::Cancelled.to_string(), "cancelled");

        assert!(matches!("pending".parse::<TodoStatus>().unwrap(), TodoStatus::Pending));
        assert!("invalid".parse::<TodoStatus>().is_err());
    }

    #[test]
    fn test_todo_priority_display_and_parse() {
        assert_eq!(TodoPriority::Low.to_string(), "low");
        assert_eq!(TodoPriority::Medium.to_string(), "medium");
        assert_eq!(TodoPriority::High.to_string(), "high");

        assert!(matches!("high".parse::<TodoPriority>().unwrap(), TodoPriority::High));
        assert!("urgent".parse::<TodoPriority>().is_err());
    }
}
