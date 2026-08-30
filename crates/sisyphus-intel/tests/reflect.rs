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

// ---------------------------------------------------------------------------
// ErrorCategory
// ---------------------------------------------------------------------------

#[test]
fn error_category_classify_resource_exhausted() {
    use sisyphus_intel::reflect::ErrorCategory;
    assert_eq!(
        ErrorCategory::classify("ResourceExhausted: out of fuel"),
        ErrorCategory::ResourceExhausted
    );
    assert_eq!(
        ErrorCategory::classify("fuel budget exceeded"),
        ErrorCategory::ResourceExhausted
    );
    assert_eq!(
        ErrorCategory::classify("out of memory while compiling"),
        ErrorCategory::ResourceExhausted
    );
}

#[test]
fn error_category_classify_timeout() {
    use sisyphus_intel::reflect::ErrorCategory;
    assert_eq!(
        ErrorCategory::classify("Operation timeout"),
        ErrorCategory::Timeout
    );
    assert_eq!(
        ErrorCategory::classify("Timeout exceeded"),
        ErrorCategory::Timeout
    );
    assert_eq!(
        ErrorCategory::classify("deadline missed"),
        ErrorCategory::Timeout
    );
}

#[test]
fn error_category_classify_type_error() {
    use sisyphus_intel::reflect::ErrorCategory;
    assert_eq!(
        ErrorCategory::classify("TypeError: cannot coerce"),
        ErrorCategory::TypeError
    );
    assert_eq!(
        ErrorCategory::classify("type mismatch"),
        ErrorCategory::TypeError
    );
}

#[test]
fn error_category_classify_crash() {
    use sisyphus_intel::reflect::ErrorCategory;
    assert_eq!(
        ErrorCategory::classify("thread 'main' panicked"),
        ErrorCategory::Crash
    );
    assert_eq!(
        ErrorCategory::classify("process crash detected"),
        ErrorCategory::Crash
    );
    assert_eq!(
        ErrorCategory::classify("segfault at 0x1234"),
        ErrorCategory::Crash
    );
}

#[test]
fn error_category_classify_unknown() {
    use sisyphus_intel::reflect::ErrorCategory;
    assert_eq!(
        ErrorCategory::classify("nothing matched"),
        ErrorCategory::Unknown
    );
    assert_eq!(ErrorCategory::classify(""), ErrorCategory::Unknown);
}

#[test]
fn error_category_severity() {
    use sisyphus_intel::reflect::ErrorCategory;
    assert!((ErrorCategory::Crash.severity() - 1.0).abs() < 1e-9);
    assert!((ErrorCategory::ResourceExhausted.severity() - 0.8).abs() < 1e-9);
    assert!((ErrorCategory::TypeError.severity() - 0.6).abs() < 1e-9);
    assert!((ErrorCategory::Timeout.severity() - 0.5).abs() < 1e-9);
    assert!((ErrorCategory::Unknown.severity() - 0.7).abs() < 1e-9);
}

#[test]
fn error_category_equality() {
    use sisyphus_intel::reflect::ErrorCategory;
    assert_eq!(ErrorCategory::Crash, ErrorCategory::Crash);
    assert_ne!(ErrorCategory::Crash, ErrorCategory::Timeout);
}

#[test]
fn error_category_clone() {
    use sisyphus_intel::reflect::ErrorCategory;
    let a = ErrorCategory::TypeError;
    let b = a.clone();
    assert_eq!(a, b);
}

#[test]
fn error_category_debug() {
    use sisyphus_intel::reflect::ErrorCategory;
    let _ = format!("{:?}", ErrorCategory::Crash);
    let _ = format!("{:?}", ErrorCategory::Unknown);
}

// ---------------------------------------------------------------------------
// RecordedNode
// ---------------------------------------------------------------------------

#[test]
fn recorded_node_field_assignment() {
    use sisyphus_intel::reflect::RecordedNode;
    let n = RecordedNode {
        node: "n1".into(),
        status: "completed".into(),
        output: "result".into(),
    };
    assert_eq!(n.node, "n1");
    assert_eq!(n.status, "completed");
    assert_eq!(n.output, "result");
}

#[test]
fn recorded_node_serde() {
    use sisyphus_intel::reflect::RecordedNode;
    let n = RecordedNode {
        node: "n".into(),
        status: "failed".into(),
        output: "err".into(),
    };
    let j = serde_json::to_string(&n).unwrap();
    let back: RecordedNode = serde_json::from_str(&j).unwrap();
    assert_eq!(back.node, "n");
    assert_eq!(back.status, "failed");
}

#[test]
fn recorded_node_clone() {
    use sisyphus_intel::reflect::RecordedNode;
    let n = RecordedNode {
        node: "n".into(),
        status: "s".into(),
        output: "o".into(),
    };
    let cloned = n.clone();
    assert_eq!(cloned.node, n.node);
}

#[test]
fn recorded_node_unicode() {
    use sisyphus_intel::reflect::RecordedNode;
    let n = RecordedNode {
        node: "节点-α".into(),
        status: "完成".into(),
        output: "结果".into(),
    };
    assert_eq!(n.node, "节点-α");
    let j = serde_json::to_string(&n).unwrap();
    let back: RecordedNode = serde_json::from_str(&j).unwrap();
    assert_eq!(back.output, "结果");
}

#[test]
fn recorded_node_empty_strings() {
    use sisyphus_intel::reflect::RecordedNode;
    let n = RecordedNode {
        node: "".into(),
        status: "".into(),
        output: "".into(),
    };
    let j = serde_json::to_string(&n).unwrap();
    let back: RecordedNode = serde_json::from_str(&j).unwrap();
    assert_eq!(back.node, "");
}

// ---------------------------------------------------------------------------
// ReflectionReport
// ---------------------------------------------------------------------------

#[test]
fn reflection_report_field_assignment() {
    use sisyphus_intel::reflect::{ImprovementKind, RecordedNode, ReflectionReport};
    let r = ReflectionReport {
        original_sop_id: "sop-1".into(),
        deviation_score: 0.3,
        suggested_improvements: vec!["retry".into()],
        improvement_signals: vec![ImprovementKind::Backoff],
        evolved_sop_structure: Some("evolved".into()),
        records: vec![RecordedNode {
            node: "n1".into(),
            status: "completed".into(),
            output: "ok".into(),
        }],
    };
    assert_eq!(r.original_sop_id, "sop-1");
    assert_eq!(r.suggested_improvements.len(), 1);
    assert_eq!(r.improvement_signals.len(), 1);
    assert_eq!(r.records.len(), 1);
    assert!(r.evolved_sop_structure.is_some());
}

#[test]
fn reflection_report_serde() {
    use sisyphus_intel::reflect::ReflectionReport;
    let r = ReflectionReport {
        original_sop_id: "sop".into(),
        deviation_score: 0.0,
        suggested_improvements: vec![],
        improvement_signals: vec![],
        evolved_sop_structure: None,
        records: vec![],
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: ReflectionReport = serde_json::from_str(&j).unwrap();
    assert_eq!(back.original_sop_id, "sop");
    assert!(back.evolved_sop_structure.is_none());
}

#[test]
fn reflection_report_default_signals_empty() {
    // Old serializations without improvement_signals should deserialize with empty Vec
    use sisyphus_intel::reflect::ReflectionReport;
    let j = r#"{"original_sop_id":"sop","deviation_score":0.5,"suggested_improvements":[],"evolved_sop_structure":null,"records":[]}"#;
    let r: ReflectionReport = serde_json::from_str(j).unwrap();
    assert_eq!(r.original_sop_id, "sop");
    assert!(r.improvement_signals.is_empty());
}

#[test]
fn reflection_report_clone() {
    use sisyphus_intel::reflect::ReflectionReport;
    let r = ReflectionReport {
        original_sop_id: "sop".into(),
        deviation_score: 0.5,
        suggested_improvements: vec![],
        improvement_signals: vec![],
        evolved_sop_structure: None,
        records: vec![],
    };
    let cloned = r.clone();
    assert_eq!(cloned.original_sop_id, "sop");
    assert_eq!(cloned.deviation_score, 0.5);
}

#[test]
fn reflection_report_with_max_deviation() {
    use sisyphus_intel::reflect::ReflectionReport;
    let r = ReflectionReport {
        original_sop_id: "sop".into(),
        deviation_score: 1.0,
        suggested_improvements: vec![],
        improvement_signals: vec![],
        evolved_sop_structure: None,
        records: vec![],
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: ReflectionReport = serde_json::from_str(&j).unwrap();
    assert_eq!(back.deviation_score, 1.0);
}

// ---------------------------------------------------------------------------
// Additional ImprovementKind tests
// ---------------------------------------------------------------------------

#[test]
fn improvement_kind_retry_with_zero_max_retries() {
    use sisyphus_intel::reflect::ImprovementKind;
    let k = ImprovementKind::Retry { max_retries: 0 };
    let j = serde_json::to_string(&k).unwrap();
    let back: ImprovementKind = serde_json::from_str(&j).unwrap();
    match back {
        ImprovementKind::Retry { max_retries } => assert_eq!(max_retries, 0),
        _ => panic!("wrong"),
    }
}

#[test]
fn improvement_kind_fuel_increase_zero_factor() {
    use sisyphus_intel::reflect::ImprovementKind;
    let k = ImprovementKind::FuelIncrease { factor: 0.0 };
    let j = serde_json::to_string(&k).unwrap();
    let back: ImprovementKind = serde_json::from_str(&j).unwrap();
    match back {
        ImprovementKind::FuelIncrease { factor } => assert_eq!(factor, 0.0),
        _ => panic!("wrong"),
    }
}

#[test]
fn improvement_kind_fuel_increase_above_one_factor() {
    use sisyphus_intel::reflect::ImprovementKind;
    let k = ImprovementKind::FuelIncrease { factor: 5.0 };
    let j = serde_json::to_string(&k).unwrap();
    let back: ImprovementKind = serde_json::from_str(&j).unwrap();
    match back {
        ImprovementKind::FuelIncrease { factor } => assert_eq!(factor, 5.0),
        _ => panic!("wrong"),
    }
}

#[test]
fn improvement_kind_unclassified_with_unicode_note() {
    use sisyphus_intel::reflect::ImprovementKind;
    let k = ImprovementKind::Unclassified {
        note: "无法分类-α".into(),
    };
    let j = serde_json::to_string(&k).unwrap();
    let back: ImprovementKind = serde_json::from_str(&j).unwrap();
    match back {
        ImprovementKind::Unclassified { note } => assert_eq!(note, "无法分类-α"),
        _ => panic!("wrong"),
    }
}

#[test]
fn improvement_kind_unclassified_with_empty_note() {
    use sisyphus_intel::reflect::ImprovementKind;
    let k = ImprovementKind::Unclassified { note: "".into() };
    let j = serde_json::to_string(&k).unwrap();
    let back: ImprovementKind = serde_json::from_str(&j).unwrap();
    match back {
        ImprovementKind::Unclassified { note } => assert_eq!(note, ""),
        _ => panic!("wrong"),
    }
}

#[test]
fn improvement_kind_serialize_to_distinct_strings() {
    use sisyphus_intel::reflect::ImprovementKind;
    let variants = [
        ImprovementKind::Backoff,
        ImprovementKind::NodeSplit,
        ImprovementKind::Retry { max_retries: 1 },
        ImprovementKind::FuelIncrease { factor: 1.0 },
        ImprovementKind::Unclassified { note: "x".into() },
    ];
    let serialized: Vec<String> = variants
        .iter()
        .map(|v| serde_json::to_string(v).unwrap())
        .collect();
    let unique: std::collections::HashSet<_> = serialized.iter().collect();
    assert_eq!(unique.len(), 5);
}

#[test]
fn improvement_kind_unclassified_with_long_note() {
    use sisyphus_intel::reflect::ImprovementKind;
    let long = "x".repeat(10000);
    let k = ImprovementKind::Unclassified { note: long.clone() };
    let j = serde_json::to_string(&k).unwrap();
    let back: ImprovementKind = serde_json::from_str(&j).unwrap();
    match back {
        ImprovementKind::Unclassified { note } => assert_eq!(note.len(), 10000),
        _ => panic!("wrong"),
    }
}
