//! Integration tests for sisyphus-tools-llm `metis` module's
//! pure data types: `MetisError`, `Ambiguity`, `AmbiguitySeverity`,
//! `ImplicitRequirement`.

use sisyphus_tools_llm::metis::{Ambiguity, AmbiguitySeverity, ImplicitRequirement, MetisError};

// ---------------------------------------------------------------------------
// MetisError
// ---------------------------------------------------------------------------

#[test]
fn metis_error_llm_error_display() {
    let e = MetisError::LlmError("connection refused".into());
    let s = e.to_string();
    assert!(s.contains("LLM call failed"));
    assert!(s.contains("connection refused"));
}

#[test]
fn metis_error_parse_error_display() {
    // Build a ParseError via serde_json
    let bad = serde_json::from_str::<i32>("not_a_number");
    let json_err = bad.unwrap_err();
    let e: MetisError = json_err.into();
    let s = e.to_string();
    assert!(s.contains("Failed to parse"));
}

#[test]
fn metis_error_from_types_error() {
    let te = sisyphus_types::TypesError::InternalError("boom".into());
    let e: MetisError = te.into();
    match e {
        MetisError::LlmError(msg) => assert!(msg.contains("boom")),
        _ => panic!("expected LlmError variant"),
    }
}

#[test]
fn metis_error_debug() {
    let e = MetisError::LlmError("x".into());
    let s = format!("{e:?}");
    assert!(s.contains("LlmError"));
}

// ---------------------------------------------------------------------------
// AmbiguitySeverity
// ---------------------------------------------------------------------------

#[test]
fn ambiguity_severity_variants_distinct() {
    let variants = vec![
        AmbiguitySeverity::Blocking,
        AmbiguitySeverity::Significant,
        AmbiguitySeverity::Minor,
    ];
    let mut seen = std::collections::HashSet::new();
    for v in &variants {
        let s = format!("{v:?}");
        assert!(seen.insert(s));
    }
    assert_eq!(seen.len(), 3);
}

#[test]
fn ambiguity_severity_clone() {
    let a = AmbiguitySeverity::Blocking;
    let b = a.clone();
    assert_eq!(format!("{a:?}"), format!("{b:?}"));
}

// ---------------------------------------------------------------------------
// Ambiguity
// ---------------------------------------------------------------------------

#[test]
fn ambiguity_field_access() {
    let a = Ambiguity {
        aspect: "scope".into(),
        description: "What's the target audience?".into(),
        severity: AmbiguitySeverity::Blocking,
        options: vec!["developers".into(), "end-users".into()],
        recommended: Some("developers".into()),
    };
    assert_eq!(a.aspect, "scope");
    assert_eq!(a.description, "What's the target audience?");
    assert!(matches!(a.severity, AmbiguitySeverity::Blocking));
    assert_eq!(a.options.len(), 2);
    assert_eq!(a.recommended.as_deref(), Some("developers"));
}

#[test]
fn ambiguity_no_recommendation() {
    let a = Ambiguity {
        aspect: "timeline".into(),
        description: "Deadline?".into(),
        severity: AmbiguitySeverity::Minor,
        options: vec![],
        recommended: None,
    };
    assert!(a.recommended.is_none());
    assert!(a.options.is_empty());
}

#[test]
fn ambiguity_clone() {
    let a = Ambiguity {
        aspect: "tech".into(),
        description: "DB?".into(),
        severity: AmbiguitySeverity::Significant,
        options: vec!["postgres".into()],
        recommended: Some("postgres".into()),
    };
    let b = a.clone();
    assert_eq!(a.aspect, b.aspect);
    assert_eq!(a.options, b.options);
}

// ---------------------------------------------------------------------------
// ImplicitRequirement
// ---------------------------------------------------------------------------

#[test]
fn implicit_requirement_field_access() {
    let r = ImplicitRequirement {
        description: "Need authentication".into(),
        rationale: "Multi-user system".into(),
        confidence: 0.9,
    };
    assert_eq!(r.description, "Need authentication");
    assert_eq!(r.rationale, "Multi-user system");
    assert!((r.confidence - 0.9).abs() < 1e-9);
}

#[test]
fn implicit_requirement_zero_confidence() {
    let r = ImplicitRequirement {
        description: "x".into(),
        rationale: "y".into(),
        confidence: 0.0,
    };
    assert_eq!(r.confidence, 0.0);
}

#[test]
fn implicit_requirement_full_confidence() {
    let r = ImplicitRequirement {
        description: "x".into(),
        rationale: "y".into(),
        confidence: 1.0,
    };
    assert_eq!(r.confidence, 1.0);
}

#[test]
fn implicit_requirement_clone() {
    let a = ImplicitRequirement {
        description: "x".into(),
        rationale: "y".into(),
        confidence: 0.5,
    };
    let b = a.clone();
    assert_eq!(a.description, b.description);
    assert_eq!(a.confidence, b.confidence);
}

// ---------------------------------------------------------------------------
// Additional coverage
// ---------------------------------------------------------------------------

#[test]
fn ambiguity_serde() {
    let a = sisyphus_tools_llm::metis::Ambiguity {
        aspect: "scope".into(),
        description: "unclear scope".into(),
        severity: sisyphus_tools_llm::metis::AmbiguitySeverity::Blocking,
        options: vec!["A".into(), "B".into()],
        recommended: Some("A".into()),
    };
    let j = serde_json::to_string(&a).unwrap();
    let back: sisyphus_tools_llm::metis::Ambiguity = serde_json::from_str(&j).unwrap();
    assert_eq!(back.aspect, "scope");
    assert_eq!(back.options.len(), 2);
    assert_eq!(back.recommended, Some("A".into()));
}

#[test]
fn ambiguity_serde_no_recommended() {
    let a = sisyphus_tools_llm::metis::Ambiguity {
        aspect: "tech".into(),
        description: "no recommendation".into(),
        severity: sisyphus_tools_llm::metis::AmbiguitySeverity::Minor,
        options: vec![],
        recommended: None,
    };
    let j = serde_json::to_string(&a).unwrap();
    let back: sisyphus_tools_llm::metis::Ambiguity = serde_json::from_str(&j).unwrap();
    assert!(back.recommended.is_none());
    assert!(back.options.is_empty());
}

#[test]
fn ambiguity_severity_equality() {
    use sisyphus_tools_llm::metis::AmbiguitySeverity;
    assert_eq!(AmbiguitySeverity::Blocking, AmbiguitySeverity::Blocking);
    assert_ne!(AmbiguitySeverity::Blocking, AmbiguitySeverity::Minor);
}

#[test]
fn ambiguity_severity_serde() {
    use sisyphus_tools_llm::metis::AmbiguitySeverity;
    for v in [
        AmbiguitySeverity::Blocking,
        AmbiguitySeverity::Significant,
        AmbiguitySeverity::Minor,
    ] {
        let j = serde_json::to_string(&v).unwrap();
        let back: AmbiguitySeverity = serde_json::from_str(&j).unwrap();
        assert_eq!(back, v);
    }
}

#[test]
fn ambiguity_severity_display() {
    use sisyphus_tools_llm::metis::AmbiguitySeverity;
    let _ = format!("{}", AmbiguitySeverity::Blocking);
    let _ = format!("{}", AmbiguitySeverity::Minor);
}

#[test]
fn ambiguity_unicode() {
    let a = sisyphus_tools_llm::metis::Ambiguity {
        aspect: "范围".into(),
        description: "范围不明确".into(),
        severity: sisyphus_tools_llm::metis::AmbiguitySeverity::Blocking,
        options: vec!["选项-α".into()],
        recommended: Some("选项-β".into()),
    };
    assert_eq!(a.aspect, "范围");
}

#[test]
fn implicit_requirement_serde() {
    let r = sisyphus_tools_llm::metis::ImplicitRequirement {
        description: "d".into(),
        rationale: "r".into(),
        confidence: 0.7,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: sisyphus_tools_llm::metis::ImplicitRequirement = serde_json::from_str(&j).unwrap();
    assert_eq!(back.confidence, 0.7);
}

#[test]
fn implicit_requirement_serde_unicode() {
    let r = sisyphus_tools_llm::metis::ImplicitRequirement {
        description: "需求-α".into(),
        rationale: "原因-β".into(),
        confidence: 0.5,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: sisyphus_tools_llm::metis::ImplicitRequirement = serde_json::from_str(&j).unwrap();
    assert_eq!(back.description, "需求-α");
}

#[test]
fn implicit_requirement_clone_preserves_confidence() {
    let r = sisyphus_tools_llm::metis::ImplicitRequirement {
        description: "d".into(),
        rationale: "r".into(),
        confidence: 0.5,
    };
    let cloned = r.clone();
    assert_eq!(cloned.confidence, 0.5);
}

#[test]
fn implicit_requirement_above_one_confidence() {
    // Out-of-range confidence - just verify it doesn't panic
    let r = sisyphus_tools_llm::metis::ImplicitRequirement {
        description: "d".into(),
        rationale: "r".into(),
        confidence: 1.5,
    };
    assert_eq!(r.confidence, 1.5);
}
