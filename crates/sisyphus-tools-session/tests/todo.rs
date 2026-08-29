//! Integration tests for sisyphus-tools-session `todo` module.
//!
//! Exercises the pure-data types [`TodoItem`], [`TodoStatus`],
//! [`TodoPriority`] and the `todo_*` CRUD functions. The CRUD functions
//! use a per-test tempdir to keep state isolated.

use sisyphus_tools_session::todo::{todo_add, todo_delete, todo_list, todo_update, TodoItem, TodoPriority, TodoStatus};
use std::path::PathBuf;

fn tmp_path() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sisyphus-todo-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("todos.json")
}

// ---------------------------------------------------------------------------
// TodoStatus / TodoPriority
// ---------------------------------------------------------------------------

#[test]
fn todo_status_variants_distinct() {
    // Use Debug strings (no PartialEq).
    let mut seen = std::collections::HashSet::new();
    for s in [
        TodoStatus::Pending,
        TodoStatus::InProgress,
        TodoStatus::Completed,
        TodoStatus::Cancelled,
    ] {
        let d = format!("{s:?}");
        assert!(seen.insert(d.clone()));
    }
}

#[test]
fn todo_priority_variants_distinct() {
    let mut seen = std::collections::HashSet::new();
    for p in [
        TodoPriority::Low,
        TodoPriority::Medium,
        TodoPriority::High,
    ] {
        let d = format!("{p:?}");
        assert!(seen.insert(d.clone()));
    }
}

#[test]
fn todo_status_serde() {
    for s in [
        TodoStatus::Pending,
        TodoStatus::InProgress,
        TodoStatus::Completed,
        TodoStatus::Cancelled,
    ] {
        let j = serde_json::to_string(&s).unwrap();
        let back: TodoStatus = serde_json::from_str(&j).unwrap();
        assert_eq!(format!("{back:?}"), format!("{s:?}"));
    }
}

#[test]
fn todo_priority_serde() {
    for p in [TodoPriority::Low, TodoPriority::Medium, TodoPriority::High] {
        let j = serde_json::to_string(&p).unwrap();
        let back: TodoPriority = serde_json::from_str(&j).unwrap();
        assert_eq!(format!("{back:?}"), format!("{p:?}"));
    }
}

// ---------------------------------------------------------------------------
// TodoItem field assignment + serde
// ---------------------------------------------------------------------------

#[test]
fn todo_item_field_assignment() {
    let item = TodoItem {
        id: "t-1".into(),
        content: "do the thing".into(),
        status: TodoStatus::Pending,
        priority: TodoPriority::High,
        created_at: "t0".into(),
        updated_at: "t1".into(),
    };
    assert_eq!(item.id, "t-1");
    assert_eq!(item.content, "do the thing");
}

#[test]
fn todo_item_serde_round_trip() {
    let item = TodoItem {
        id: "t-1".into(),
        content: "c".into(),
        status: TodoStatus::InProgress,
        priority: TodoPriority::Medium,
        created_at: "a".into(),
        updated_at: "b".into(),
    };
    let j = serde_json::to_string(&item).unwrap();
    let back: TodoItem = serde_json::from_str(&j).unwrap();
    assert_eq!(back.id, "t-1");
    assert_eq!(format!("{:?}", back.status), "InProgress");
}

// ---------------------------------------------------------------------------
// todo_add / todo_list (CRUD via tempdir)
// ---------------------------------------------------------------------------

#[test]
fn todo_add_creates_item() {
    let path = tmp_path();
    let item = todo_add(&path, "first todo", TodoPriority::Low).unwrap();
    assert!(!item.id.is_empty());
    assert_eq!(item.content, "first todo");
    assert_eq!(format!("{:?}", item.priority), "Low");
    assert_eq!(format!("{:?}", item.status), "Pending");
}

#[test]
fn todo_list_empty_initially() {
    let path = tmp_path();
    let list = todo_list(&path, None).unwrap();
    assert!(list.is_empty());
}

#[test]
fn todo_list_returns_added() {
    let path = tmp_path();
    todo_add(&path, "a", TodoPriority::Low).unwrap();
    todo_add(&path, "b", TodoPriority::High).unwrap();
    let list = todo_list(&path, None).unwrap();
    assert_eq!(list.len(), 2);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn todo_list_filter_by_status() {
    let path = tmp_path();
    let a = todo_add(&path, "a", TodoPriority::Low).unwrap();
    todo_add(&path, "b", TodoPriority::High).unwrap();
    // Update `a` to Completed.
    todo_update(&path, &a.id, None, Some(TodoStatus::Completed), None).unwrap();
    let pending = todo_list(&path, Some(TodoStatus::Pending)).unwrap();
    // b is pending, a is now completed.
    assert_eq!(pending.len(), 1);
    let completed = todo_list(&path, Some(TodoStatus::Completed)).unwrap();
    assert_eq!(completed.len(), 1);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn todo_update_changes_status() {
    let path = tmp_path();
    let a = todo_add(&path, "a", TodoPriority::Low).unwrap();
    todo_update(&path, &a.id, None, Some(TodoStatus::InProgress), None).unwrap();
    let list = todo_list(&path, Some(TodoStatus::InProgress)).unwrap();
    assert_eq!(list.len(), 1);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn todo_delete_removes_item() {
    let path = tmp_path();
    let a = todo_add(&path, "a", TodoPriority::Low).unwrap();
    assert_eq!(todo_list(&path, None).unwrap().len(), 1);
    todo_delete(&path, &a.id).unwrap();
    assert!(todo_list(&path, None).unwrap().is_empty());
    let _ = std::fs::remove_file(&path);
}
