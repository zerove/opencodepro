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
    use std::collections::HashMap;
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
    use std::collections::HashMap;
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
    use std::collections::HashMap;
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
