//! Integration tests for sisyphus-knowledge pure error type `KnowledgeError`.

use sisyphus_knowledge::KnowledgeError;

#[test]
fn knowledge_error_asset_not_found_display() {
    let e = KnowledgeError::AssetNotFound("asset-42".into());
    let s = e.to_string();
    assert!(s.contains("Asset not found"));
    assert!(s.contains("asset-42"));
}

#[test]
fn knowledge_error_storage_error_display() {
    let e = KnowledgeError::StorageError("disk full".into());
    let s = e.to_string();
    assert!(s.contains("Storage error"));
    assert!(s.contains("disk full"));
}

#[test]
fn knowledge_error_internal_display() {
    let e = KnowledgeError::Internal("corrupt state".into());
    let s = e.to_string();
    assert!(s.contains("Internal error"));
    assert!(s.contains("corrupt state"));
}

#[test]
fn knowledge_error_variants_distinct() {
    let errs = vec![
        KnowledgeError::AssetNotFound("a".into()),
        KnowledgeError::StorageError("s".into()),
        KnowledgeError::Internal("i".into()),
    ];
    let mut seen = std::collections::HashSet::new();
    for e in &errs {
        let s = format!("{e:?}");
        assert!(seen.insert(s));
    }
    assert_eq!(seen.len(), 3);
}

#[test]
fn knowledge_error_debug() {
    let e = KnowledgeError::AssetNotFound("x".into());
    let s = format!("{e:?}");
    assert!(s.contains("AssetNotFound"));
}

#[test]
fn knowledge_error_empty_id() {
    let e = KnowledgeError::AssetNotFound(String::new());
    let s = e.to_string();
    assert!(s.contains("Asset not found"));
}

#[test]
fn knowledge_error_unicode() {
    let e = KnowledgeError::Internal("错误信息".into());
    let s = e.to_string();
    assert!(s.contains("错误信息"));
}
