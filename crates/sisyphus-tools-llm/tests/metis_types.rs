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
