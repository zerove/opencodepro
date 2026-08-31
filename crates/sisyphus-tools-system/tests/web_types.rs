//! Integration tests for sisyphus-tools-system `web` module's
//! pure data types: `SearchResult`, `FetchResult`.

use sisyphus_tools_system::web::{FetchResult, SearchResult};

// ---------------------------------------------------------------------------
// SearchResult
// ---------------------------------------------------------------------------

#[test]
fn search_result_field_access() {
    let s = SearchResult {
        title: "Sisyphus Engine".into(),
        url: "https://sisyphus.example".into(),
        snippet: "A cognitive engine".into(),
    };
    assert_eq!(s.title, "Sisyphus Engine");
    assert_eq!(s.url, "https://sisyphus.example");
    assert_eq!(s.snippet, "A cognitive engine");
}

#[test]
fn search_result_empty_fields() {
    let s = SearchResult {
        title: String::new(),
        url: String::new(),
        snippet: String::new(),
    };
    assert!(s.title.is_empty());
    assert!(s.url.is_empty());
    assert!(s.snippet.is_empty());
}

#[test]
fn search_result_clone() {
    let a = SearchResult {
        title: "t".into(),
        url: "u".into(),
        snippet: "s".into(),
    };
    let b = a.clone();
    assert_eq!(a.title, b.title);
    assert_eq!(a.url, b.url);
}

#[test]
fn search_result_long_snippet() {
    let snippet = "a".repeat(1000);
    let s = SearchResult {
        title: "t".into(),
        url: "u".into(),
        snippet: snippet.clone(),
    };
    assert_eq!(s.snippet.len(), 1000);
}

#[test]
fn search_result_debug() {
    let s = SearchResult {
        title: "t".into(),
        url: "u".into(),
        snippet: "s".into(),
    };
    let d = format!("{s:?}");
    assert!(d.contains("t"));
}

// ---------------------------------------------------------------------------
// FetchResult
// ---------------------------------------------------------------------------

#[test]
fn fetch_result_field_access() {
    let f = FetchResult {
        url: "https://example.com".into(),
        content: "<html></html>".into(),
        content_type: "text/html".into(),
    };
    assert_eq!(f.url, "https://example.com");
    assert_eq!(f.content, "<html></html>");
    assert_eq!(f.content_type, "text/html");
}

#[test]
fn fetch_result_empty() {
    let f = FetchResult {
        url: String::new(),
        content: String::new(),
        content_type: String::new(),
    };
    assert!(f.url.is_empty());
    assert!(f.content.is_empty());
    assert!(f.content_type.is_empty());
}

#[test]
fn fetch_result_clone() {
    let a = FetchResult {
        url: "u".into(),
        content: "c".into(),
        content_type: "text/plain".into(),
    };
    let b = a.clone();
    assert_eq!(a.content, b.content);
}

#[test]
fn fetch_result_common_content_types() {
    let types = vec![
        "text/html",
        "text/plain",
        "application/json",
        "application/xml",
        "text/markdown",
    ];
    for ct in types {
        let f = FetchResult {
            url: "u".into(),
            content: "c".into(),
            content_type: ct.into(),
        };
        assert_eq!(f.content_type, ct);
    }
}

#[test]
fn fetch_result_debug() {
    let f = FetchResult {
        url: "u".into(),
        content: "c".into(),
        content_type: "t".into(),
    };
    let d = format!("{f:?}");
    assert!(d.contains("u"));
}
