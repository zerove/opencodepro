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

// ---------------------------------------------------------------------------
// ImprovementKind Serde / Clone
// ---------------------------------------------------------------------------

#[test]
fn improvement_kind_retry_serde() {
    let k = ImprovementKind::Retry { max_retries: 5 };
    let j = serde_json::to_string(&k).unwrap();
    let back: ImprovementKind = serde_json::from_str(&j).unwrap();
    match back {
        ImprovementKind::Retry { max_retries } => assert_eq!(max_retries, 5),
        _ => panic!("wrong variant"),
    }
}

#[test]
fn improvement_kind_fuel_increase_serde() {
    let k = ImprovementKind::FuelIncrease { factor: 2.0 };
    let j = serde_json::to_string(&k).unwrap();
    let back: ImprovementKind = serde_json::from_str(&j).unwrap();
    match back {
        ImprovementKind::FuelIncrease { factor } => assert!((factor - 2.0).abs() < 1e-9),
        _ => panic!("wrong variant"),
    }
}

#[test]
fn improvement_kind_node_split_serde() {
    let k = ImprovementKind::NodeSplit;
    let j = serde_json::to_string(&k).unwrap();
    let back: ImprovementKind = serde_json::from_str(&j).unwrap();
    assert_eq!(back, ImprovementKind::NodeSplit);
}

#[test]
fn improvement_kind_backoff_serde() {
    let k = ImprovementKind::Backoff;
    let j = serde_json::to_string(&k).unwrap();
    let back: ImprovementKind = serde_json::from_str(&j).unwrap();
    assert_eq!(back, ImprovementKind::Backoff);
}

#[test]
fn improvement_kind_unclassified_serde() {
    let k = ImprovementKind::Unclassified {
        note: "uncertain pattern".into(),
    };
    let j = serde_json::to_string(&k).unwrap();
    let back: ImprovementKind = serde_json::from_str(&j).unwrap();
    match back {
        ImprovementKind::Unclassified { note } => assert_eq!(note, "uncertain pattern"),
        _ => panic!("wrong variant"),
    }
}

#[test]
fn improvement_kind_retry_default_uses_serde_default() {
    // If max_retries is missing, serde default is 3
    let j = r#"{"Retry":{}}"#;
    let k: ImprovementKind = serde_json::from_str(j).unwrap();
    match k {
        ImprovementKind::Retry { max_retries } => assert_eq!(max_retries, 3),
        _ => panic!("wrong variant"),
    }
}

#[test]
fn improvement_kind_clone() {
    let a = ImprovementKind::Retry { max_retries: 7 };
    let b = a.clone();
    assert_eq!(a, b);
}

#[test]
fn improvement_kind_distinct_variants() {
    let a = ImprovementKind::Backoff;
    let b = ImprovementKind::NodeSplit;
    let c = ImprovementKind::Retry { max_retries: 1 };
    let d = ImprovementKind::FuelIncrease { factor: 1.0 };
    let e = ImprovementKind::Unclassified { note: "x".into() };
    assert_ne!(a, b);
    assert_ne!(a, c);
    assert_ne!(a, d);
    assert_ne!(a, e);
    assert_ne!(b, c);
    assert_ne!(c, d);
    assert_ne!(d, e);
}

#[test]
fn improvement_kind_equality_within_variant() {
    let a = ImprovementKind::Retry { max_retries: 3 };
    let b = ImprovementKind::Retry { max_retries: 3 };
    assert_eq!(a, b);
    let c = ImprovementKind::Retry { max_retries: 4 };
    assert_ne!(a, c);
}

#[test]
fn from_text_empty_returns_none() {
    assert!(ImprovementKind::from_text("").is_none());
}

#[test]
fn from_text_whitespace_only_returns_none() {
    assert!(ImprovementKind::from_text("   \n\t  ").is_none());
}

#[test]
fn from_text_numbers_only_returns_none() {
    assert!(ImprovementKind::from_text("12345 678 9.0").is_none());
}
