use super::*;
use serde_json::Value;

#[test]
fn test_to_file_uri_absolute() {
    let uri = to_file_uri("/home/user/file.rs");
    assert_eq!(uri, "file:///home/user/file.rs");
}

#[test]
fn test_to_file_uri_relative_does_not_crash() {
    let uri = to_file_uri("src/lib.rs");
    assert!(uri.starts_with("file://"));
    assert!(uri.contains("src/lib.rs"));
}

#[test]
fn test_severity_name() {
    assert_eq!(severity_name(1), "error");
    assert_eq!(severity_name(2), "warning");
    assert_eq!(severity_name(3), "info");
    assert_eq!(severity_name(4), "hint");
    assert_eq!(severity_name(99), "?");
}

#[test]
fn test_extract_hover_markup() {
    let json = serde_json::json!({"kind": "markdown", "value": "hello **world**"});
    assert_eq!(extract_hover_text(&json), "hello **world**");
}

#[test]
fn test_extract_hover_string() {
    let json = serde_json::json!("simple text");
    assert_eq!(extract_hover_text(&json), "simple text");
}

#[test]
fn test_extract_hover_array() {
    let json = serde_json::json!(["a", "b", "c"]);
    assert_eq!(extract_hover_text(&json), "a\nb\nc");
}

#[test]
fn test_deserialize_diagnostic() {
    let json = serde_json::json!({
        "range": { "start": { "line": 1, "character": 5 }, "end": { "line": 1, "character": 10 } },
        "severity": 1,
        "message": "unused variable"
    });
    let d: LspDiagnostic = serde_json::from_value(json).unwrap();
    assert_eq!(d.message, "unused variable");
    assert_eq!(d.severity, Some(1));
    assert_eq!(d.range.start.line, 1);
    assert_eq!(d.range.end.character, 10);
}

#[test]
fn test_deserialize_location() {
    let json = serde_json::json!({
        "uri": "file:///test.rs",
        "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 5 } }
    });
    let loc: LspLocation = serde_json::from_value(json).unwrap();
    assert_eq!(loc.uri, "file:///test.rs");
}

#[test]
fn test_serde_lsp_position() {
    let json = serde_json::json!({ "line": 42, "character": 7 });
    let pos: LspPosition = serde_json::from_value(json).unwrap();
    assert_eq!(pos.line, 42);
    assert_eq!(pos.character, 7);
}

#[test]
fn test_serde_lsp_range() {
    let json = serde_json::json!({
        "start": { "line": 1, "character": 2 },
        "end": { "line": 3, "character": 4 }
    });
    let r: LspRange = serde_json::from_value(json).unwrap();
    assert_eq!(r.start.line, 1);
    assert_eq!(r.end.character, 4);
}

#[test]
fn test_serde_lsp_completion_item() {
    let json = serde_json::json!({ "label": "foo", "detail": "fn()" });
    let item: LspCompletionItem = serde_json::from_value(json).unwrap();
    assert_eq!(item.label, "foo");
    assert_eq!(item.detail.unwrap(), "fn()");
}

#[test]
fn test_serde_lsp_completion_item_no_detail() {
    let json = serde_json::json!({ "label": "bar" });
    let item: LspCompletionItem = serde_json::from_value(json).unwrap();
    assert_eq!(item.label, "bar");
    assert!(item.detail.is_none());
}

#[test]
fn test_serde_lsp_document_symbol() {
    let json = serde_json::json!({
        "name": "my_func",
        "kind": 12,
        "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 5, "character": 0 } },
        "selection_range": { "start": { "line": 0, "character": 0 }, "end": { "line": 5, "character": 0 } }
    });
    let sym: LspDocumentSymbol = serde_json::from_value(json).unwrap();
    assert_eq!(sym.name, "my_func");
    assert_eq!(sym.kind, 12);
    assert!(sym.children.is_none());
}

#[test]
fn test_deserialize_diagnostic_no_severity() {
    let json = serde_json::json!({
        "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 1 } },
        "message": "info message"
    });
    let d: LspDiagnostic = serde_json::from_value(json).unwrap();
    assert_eq!(d.message, "info message");
    assert_eq!(d.severity, None);
}

#[test]
fn test_to_file_uri_windows_style() {
    let uri = to_file_uri("C:\\Users\\test\\file.rs");
    assert!(uri.starts_with("file:///"));
    assert!(uri.contains("file.rs"));
}

#[test]
fn test_to_file_uri_with_spaces() {
    let uri = to_file_uri("/home/user/my project/main.rs");
    assert_eq!(uri, "file:///home/user/my project/main.rs");
}

#[test]
fn test_to_file_uri_root() {
    let uri = to_file_uri("/");
    assert_eq!(uri, "file:///");
}

#[test]
fn test_extract_hover_markup_with_kind() {
    let json = serde_json::json!({"kind": "plaintext", "value": "fn foo()"});
    assert_eq!(extract_hover_text(&json), "fn foo()");
}

#[test]
fn test_extract_hover_object_no_value() {
    let json = serde_json::json!({"language": "rust", "foo": "bar"});
    let text = extract_hover_text(&json);
    assert!(!text.is_empty());
}

#[test]
fn test_extract_hover_mixed_array() {
    let json = serde_json::json!(["header", {"kind": "markdown", "value": "body"}]);
    let text = extract_hover_text(&json);
    assert_eq!(text, "header\nbody");
}

#[test]
fn test_extract_hover_null_and_number() {
    assert_eq!(extract_hover_text(&Value::Null), "null");
    assert_eq!(extract_hover_text(&serde_json::json!(42)), "42");
}

#[test]
fn test_lsp_error_display() {
    let e = LspError::Transport("connection refused".into());
    assert!(e.to_string().contains("connection refused"));

    let e = LspError::Protocol("bad header".into());
    assert!(e.to_string().contains("bad header"));

    let e = LspError::ServerError {
        code: -32601,
        message: "method not found".into(),
    };
    let s = e.to_string();
    assert!(s.contains("-32601"));
    assert!(s.contains("method not found"));

    let e = LspError::ServerDied;
    assert!(e.to_string().contains("unexpectedly"));
}
