//! Integration tests for sisyphus-intel `reflect` module.
//!
//! Exercises the public `ImprovementKind::from_text` classifier
//! that maps free-text reflection output to a structured
//! improvement kind (Retry, FuelIncrease, NodeSplit, Backoff,
//! or Unclassified). The classifier uses substring matching
//! against English and Chinese keywords.

use sisyphus_intel::reflect::ImprovementKind;

// ---------------------------------------------------------------------------
// FuelIncrease
// ---------------------------------------------------------------------------

#[test]
fn from_text_fuel_increase_english() {
    let k = ImprovementKind::from_text("We need to increase fuel for this node").expect("ok");
    match k {
        ImprovementKind::FuelIncrease { factor } => {
            assert!((factor - 1.5).abs() < 1e-9);
        }
        _ => panic!("expected FuelIncrease, got {k:?}"),
    }
}

#[test]
fn from_text_fuel_limit_phrase() {
    let k = ImprovementKind::from_text("Hit the fuel limit during execution").expect("ok");
    assert!(matches!(k, ImprovementKind::FuelIncrease { .. }));
}

#[test]
fn from_text_raise_fuel_phrase() {
    let k = ImprovementKind::from_text("Should raise fuel budget").expect("ok");
    assert!(matches!(k, ImprovementKind::FuelIncrease { .. }));
}

#[test]
fn from_text_chinese_resource_exhausted() {
    let k = ImprovementKind::from_text("资源耗尽，需要扩容").expect("ok");
    assert!(matches!(k, ImprovementKind::FuelIncrease { .. }));
}

#[test]
fn from_text_resource_exhausted_case_insensitive() {
    let k = ImprovementKind::from_text("Error: ResourceExhausted").expect("ok");
    assert!(matches!(k, ImprovementKind::FuelIncrease { .. }));
}

// ---------------------------------------------------------------------------
// Retry
// ---------------------------------------------------------------------------

#[test]
fn from_text_add_retry_phrase() {
    let k = ImprovementKind::from_text("Should add retry logic here").expect("ok");
    match k {
        ImprovementKind::Retry { max_retries } => {
            assert_eq!(max_retries, 3);
        }
        _ => panic!("expected Retry, got {k:?}"),
    }
}

#[test]
fn from_text_retry_logic_phrase() {
    let k = ImprovementKind::from_text("Need retry logic for flaky call").expect("ok");
    assert!(matches!(k, ImprovementKind::Retry { .. }));
}

#[test]
fn from_text_chinese_retry() {
    let k = ImprovementKind::from_text("需要增加重试机制").expect("ok");
    assert!(matches!(k, ImprovementKind::Retry { .. }));
}

// ---------------------------------------------------------------------------
// NodeSplit
// ---------------------------------------------------------------------------

#[test]
fn from_text_split_long_running() {
    let k = ImprovementKind::from_text("We should split long-running task").expect("ok");
    assert!(matches!(k, ImprovementKind::NodeSplit));
}

#[test]
fn from_text_split_the_node() {
    let k = ImprovementKind::from_text("Please split the node into smaller steps").expect("ok");
    assert!(matches!(k, ImprovementKind::NodeSplit));
}

#[test]
fn from_text_chinese_split() {
    let k = ImprovementKind::from_text("建议拆分节点").expect("ok");
    assert!(matches!(k, ImprovementKind::NodeSplit));
}

// ---------------------------------------------------------------------------
// Backoff
// ---------------------------------------------------------------------------

#[test]
fn from_text_backoff_phrase() {
    let k = ImprovementKind::from_text("Add exponential backoff between retries").expect("ok");
    assert!(matches!(k, ImprovementKind::Backoff));
}

#[test]
fn from_text_chinese_backoff() {
    let k = ImprovementKind::from_text("需要退避策略").expect("ok");
    assert!(matches!(k, ImprovementKind::Backoff));
}

// ---------------------------------------------------------------------------
// Unclassified / no match
// ---------------------------------------------------------------------------

#[test]
fn from_text_unrelated_returns_none() {
    assert!(ImprovementKind::from_text("All good, no changes needed").is_none());
    assert!(ImprovementKind::from_text("").is_none());
    assert!(ImprovementKind::from_text("Random text").is_none());
}

#[test]
fn from_text_arbitrary_phrase_returns_none() {
    assert!(ImprovementKind::from_text("Refactor the variable name").is_none());
}

// ---------------------------------------------------------------------------
// Case sensitivity
// ---------------------------------------------------------------------------

#[test]
fn from_text_is_case_insensitive() {
    let k_lower = ImprovementKind::from_text("add retry").expect("");
    let k_upper = ImprovementKind::from_text("ADD RETRY").expect("");
    let k_mixed = ImprovementKind::from_text("Add Retry").expect("");
    assert!(matches!(k_lower, ImprovementKind::Retry { .. }));
    assert!(matches!(k_upper, ImprovementKind::Retry { .. }));
    assert!(matches!(k_mixed, ImprovementKind::Retry { .. }));
}
