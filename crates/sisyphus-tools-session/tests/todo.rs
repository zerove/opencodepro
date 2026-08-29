//! Integration tests for sisyphus-tools-session `todo` module.
//!
//! Exercises the public `todo_add` / `todo_list` / `todo_update` /
//! `todo_delete` API and the `TodoStatus` / `TodoPriority` `Display` /
//! `FromStr` impls. The `NEXT_ID` counter is a process-global
//! `AtomicU64`, so concurrent test runs would race on it; these
//! tests use unique file paths per test to ensure isolation.

use sisyphus_tools_session::todo::{
    todo_add, todo_delete, todo_list, todo_update, TodoPriority, TodoStatus,
};
use std::path::PathBuf;
use tempfile::TempDir;

fn fresh_db() -> (TempDir, PathBuf) {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("todos.json");
    (dir, path)
}

// ---------------------------------------------------------------------------
// Display / FromStr
// ---------------------------------------------------------------------------

#[test]
fn todo_status_display_and_fromstr_roundtrip() {
    for s in [
        TodoStatus::Pending,
        TodoStatus::InProgress,
        TodoStatus::Completed,
        TodoStatus::Cancelled,
    ] {
        let s_str = s.to_string();
        let parsed: TodoStatus = s_str.parse().expect("parse");
        assert_eq!(parsed.to_string(), s_str);
    }
}

#[test]
fn todo_status_fromstr_invalid_returns_error() {
    let result: Result<TodoStatus, _> = "bogus".parse();
    assert!(result.is_err());
}

#[test]
fn todo_priority_display_and_fromstr_roundtrip() {
    for p in [TodoPriority::Low, TodoPriority::Medium, TodoPriority::High] {
        let p_str = p.to_string();
        let parsed: TodoPriority = p_str.parse().expect("parse");
        assert_eq!(parsed.to_string(), p_str);
    }
}

#[test]
fn todo_priority_fromstr_invalid_returns_error() {
    let result: Result<TodoPriority, _> = "urgent".parse();
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// todo_add
// ---------------------------------------------------------------------------

#[test]
fn todo_add_creates_pending_item() {
    let (_d, path) = fresh_db();
    let item = todo_add(&path, "do thing", TodoPriority::High).expect("add");
    assert_eq!(item.content, "do thing");
    assert_eq!(item.priority.to_string(), "high");
    assert_eq!(item.status.to_string(), TodoStatus::Pending.to_string());
    assert!(!item.id.is_empty());
    assert!(!item.created_at.is_empty());
}

#[test]
fn todo_add_assigns_unique_ids() {
    let (_d, path) = fresh_db();
    let a = todo_add(&path, "a", TodoPriority::Low).expect("a");
    let b = todo_add(&path, "b", TodoPriority::Low).expect("b");
    let c = todo_add(&path, "c", TodoPriority::Low).expect("c");
    let ids: Vec<String> = vec![a.id.clone(), b.id.clone(), c.id.clone()];
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), 3, "expected unique ids, got {ids:?}");
}

#[test]
fn todo_add_preserves_content() {
    let (_d, path) = fresh_db();
    let item = todo_add(&path, "exact text with unicode: 你好 🎉", TodoPriority::Medium)
        .expect("add");
    assert!(item.content.contains("你好"));
    assert!(item.content.contains("🎉"));
}

// ---------------------------------------------------------------------------
// todo_list
// ---------------------------------------------------------------------------

#[test]
fn todo_list_empty_db_returns_empty() {
    let (_d, path) = fresh_db();
    let items = todo_list(&path, None).expect("list");
    assert!(items.is_empty());
}

#[test]
fn todo_list_no_filter_returns_all() {
    let (_d, path) = fresh_db();
    todo_add(&path, "a", TodoPriority::Low).expect("a");
    todo_add(&path, "b", TodoPriority::Low).expect("b");
    let items = todo_list(&path, None).expect("list");
    assert_eq!(items.len(), 2);
}

#[test]
fn todo_list_filter_by_pending() {
    let (_d, path) = fresh_db();
    let a = todo_add(&path, "a", TodoPriority::Low).expect("a");
    let b = todo_add(&path, "b", TodoPriority::Low).expect("b");
    todo_update(&path, &b.id, None, Some(TodoStatus::Completed), None).expect("update");
    let pending = todo_list(&path, Some(TodoStatus::Pending)).expect("list");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, a.id);
}

#[test]
fn todo_list_filter_by_completed() {
    let (_d, path) = fresh_db();
    let a = todo_add(&path, "a", TodoPriority::Low).expect("a");
    todo_update(&path, &a.id, None, Some(TodoStatus::Completed), None).expect("update");
    let completed = todo_list(&path, Some(TodoStatus::Completed)).expect("list");
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0].id, a.id);
}

#[test]
fn todo_list_filter_by_cancelled() {
    let (_d, path) = fresh_db();
    let a = todo_add(&path, "a", TodoPriority::Low).expect("a");
    todo_update(&path, &a.id, None, Some(TodoStatus::Cancelled), None).expect("update");
    let cancelled = todo_list(&path, Some(TodoStatus::Cancelled)).expect("list");
    assert_eq!(cancelled.len(), 1);
}

// ---------------------------------------------------------------------------
// todo_update
// ---------------------------------------------------------------------------

#[test]
fn todo_update_content_only() {
    let (_d, path) = fresh_db();
    let item = todo_add(&path, "old", TodoPriority::Low).expect("add");
    let updated = todo_update(&path, &item.id, Some("new"), None, None).expect("update");
    assert_eq!(updated.content, "new");
    // status unchanged from initial Pending
    assert_eq!(updated.status.to_string(), TodoStatus::Pending.to_string());
}

#[test]
fn todo_update_priority_only() {
    let (_d, path) = fresh_db();
    let item = todo_add(&path, "x", TodoPriority::Low).expect("add");
    let updated = todo_update(&path, &item.id, None, None, Some(TodoPriority::High)).expect("upd");
    assert_eq!(updated.priority.to_string(), "high");
    assert_eq!(updated.content, "x");
}

#[test]
fn todo_update_none_keeps_existing() {
    let (_d, path) = fresh_db();
    let item = todo_add(&path, "stable", TodoPriority::Medium).expect("add");
    let updated = todo_update(&path, &item.id, None, None, None).expect("upd");
    assert_eq!(updated.content, "stable");
    assert_eq!(updated.priority.to_string(), TodoPriority::Medium.to_string());
}

#[test]
fn todo_update_unknown_id_returns_error() {
    let (_d, path) = fresh_db();
    let result = todo_update(&path, "nonexistent-id", None, None, None);
    assert!(result.is_err());
}

#[test]
fn todo_update_persists_to_disk() {
    let (_d, path) = fresh_db();
    let item = todo_add(&path, "x", TodoPriority::Low).expect("add");
    todo_update(&path, &item.id, Some("y"), Some(TodoStatus::InProgress), None).expect("upd");
    // Re-list and verify.
    let items = todo_list(&path, None).expect("list");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].content, "y");
    assert_eq!(items[0].status.to_string(), TodoStatus::InProgress.to_string());
}

// ---------------------------------------------------------------------------
// todo_delete
// ---------------------------------------------------------------------------

#[test]
fn todo_delete_removes_item() {
    let (_d, path) = fresh_db();
    let item = todo_add(&path, "x", TodoPriority::Low).expect("add");
    todo_delete(&path, &item.id).expect("delete");
    let items = todo_list(&path, None).expect("list");
    assert!(items.is_empty());
}

#[test]
fn todo_delete_unknown_id_returns_error() {
    let (_d, path) = fresh_db();
    let result = todo_delete(&path, "nope");
    assert!(result.is_err());
}

#[test]
fn todo_delete_only_removes_target() {
    let (_d, path) = fresh_db();
    let a = todo_add(&path, "a", TodoPriority::Low).expect("a");
    let _b = todo_add(&path, "b", TodoPriority::Low).expect("b");
    todo_delete(&path, &a.id).expect("delete");
    let items = todo_list(&path, None).expect("list");
    assert_eq!(items.len(), 1);
    assert_ne!(items[0].id, a.id);
}
