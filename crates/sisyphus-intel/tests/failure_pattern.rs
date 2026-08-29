//! Integration tests for sisyphus-intel `failure_pattern_evolver` and
//! `reflect` modules. Covers [`FailureCategory`], the
//! [`FailurePatternLibrary`] / [`FailurePatternEvolver`] public API,
//! and the [`ErrorCategory`] classifier from `reflect`.

use sisyphus_intel::failure_pattern_evolver::{
    FailureCategory, FailurePatternEvolver, FailurePatternLibrary, MutationSuggestion,
};
use sisyphus_intel::reflect::{ErrorCategory, RecordedNode, ReflectionReport};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// FailureCategory classifier (classify_failure is private, so we drive it
// indirectly through analyze_and_suggest).
// ---------------------------------------------------------------------------

fn make_report(records: Vec<RecordedNode>) -> ReflectionReport {
    ReflectionReport {
        original_sop_id: "sop-1".to_string(),
        deviation_score: 0.0,
        suggested_improvements: vec![],
        improvement_signals: vec![],
        evolved_sop_structure: None,
        records,
    }
}

fn rec(node: &str, status: &str, output: &str) -> RecordedNode {
    RecordedNode {
        node: node.to_string(),
        status: status.to_string(),
        output: output.to_string(),
    }
}

// ---------------------------------------------------------------------------
// FailurePatternLibrary
// ---------------------------------------------------------------------------

#[test]
fn library_default_is_empty() {
    let lib = FailurePatternLibrary::new();
    assert!(lib.get_node_patterns("any").is_empty());
    assert!(lib.most_common_failure("any").is_none());
    assert!(lib.get_hotspots().is_empty());
}

#[test]
fn library_default_trait() {
    let lib = FailurePatternLibrary::default();
    assert!(lib.get_hotspots().is_empty());
}

#[test]
fn library_records_same_node_same_category() {
    let mut lib = FailurePatternLibrary::new();
    lib.record_failure("a", FailureCategory::FuelExhaustion, "out of fuel");
    lib.record_failure("a", FailureCategory::FuelExhaustion, "fuel exhausted");
    // Same node + same category → coalesced into one pattern.
    assert_eq!(lib.get_node_patterns("a").len(), 1);
    assert_eq!(lib.get_node_patterns("a")[0].occurrence_count, 2);
}

#[test]
fn library_records_same_node_different_categories() {
    let mut lib = FailurePatternLibrary::new();
    lib.record_failure("a", FailureCategory::FuelExhaustion, "x");
    lib.record_failure("a", FailureCategory::Timeout, "y");
    assert_eq!(lib.get_node_patterns("a").len(), 2);
}

#[test]
fn library_most_common_failure_returns_max() {
    let mut lib = FailurePatternLibrary::new();
    lib.record_failure("a", FailureCategory::Timeout, "x");
    lib.record_failure("a", FailureCategory::Timeout, "y");
    lib.record_failure("a", FailureCategory::LogicError, "z");
    assert_eq!(
        lib.most_common_failure("a"),
        Some(FailureCategory::Timeout)
    );
}

#[test]
fn library_most_common_failure_unknown_node() {
    let lib = FailurePatternLibrary::new();
    assert!(lib.most_common_failure("nonexistent").is_none());
}

#[test]
fn library_get_hotspots_sorted_by_count() {
    let mut lib = FailurePatternLibrary::new();
    lib.record_failure("a", FailureCategory::Timeout, "x");
    lib.record_failure("a", FailureCategory::Timeout, "y");
    lib.record_failure("a", FailureCategory::Timeout, "z");
    lib.record_failure("b", FailureCategory::LogicError, "x");
    let hotspots = lib.get_hotspots();
    assert_eq!(hotspots.len(), 2);
    // First hotspot should be the one with count 3.
    assert_eq!(hotspots[0].affected_node, "a");
    assert_eq!(hotspots[0].occurrence_count, 3);
}

#[test]
fn library_get_node_patterns_isolated_per_node() {
    let mut lib = FailurePatternLibrary::new();
    lib.record_failure("a", FailureCategory::Timeout, "x");
    lib.record_failure("b", FailureCategory::Timeout, "y");
    assert_eq!(lib.get_node_patterns("a").len(), 1);
    assert_eq!(lib.get_node_patterns("b").len(), 1);
    assert_eq!(lib.get_node_patterns("a")[0].affected_node, "a");
    assert_eq!(lib.get_node_patterns("b")[0].affected_node, "b");
}

// ---------------------------------------------------------------------------
// FailurePatternEvolver
// ---------------------------------------------------------------------------

#[test]
fn evolver_new_works() {
    let lib = Arc::new(parking_lot::RwLock::new(FailurePatternLibrary::new()));
    let _ = FailurePatternEvolver::new(lib);
}

#[test]
fn evolver_analyze_empty_report_yields_no_suggestions() {
    let lib = Arc::new(parking_lot::RwLock::new(FailurePatternLibrary::new()));
    let evolver = FailurePatternEvolver::new(lib);
    let report = make_report(vec![]);
    let suggestions = evolver.analyze_and_suggest(&report);
    assert!(suggestions.is_empty());
}

#[test]
fn evolver_analyze_first_failure_yields_low_confidence_suggestion() {
    let lib = Arc::new(parking_lot::RwLock::new(FailurePatternLibrary::new()));
    let evolver = FailurePatternEvolver::new(lib);
    let report = make_report(vec![rec("node_a", "failed", "out of fuel")]);
    let suggestions = evolver.analyze_and_suggest(&report);
    assert_eq!(suggestions.len(), 1);
    assert_eq!(suggestions[0].node_name, "node_a");
    assert_eq!(suggestions[0].category, FailureCategory::FuelExhaustion);
    assert!((suggestions[0].confidence - 0.3).abs() < 1e-9);
}

#[test]
fn evolver_classifies_fuel_keyword() {
    let lib = Arc::new(parking_lot::RwLock::new(FailurePatternLibrary::new()));
    let evolver = FailurePatternEvolver::new(lib);
    let report = make_report(vec![rec("n", "failed", "fuel budget exceeded")]);
    let s = evolver.analyze_and_suggest(&report);
    assert_eq!(s[0].category, FailureCategory::FuelExhaustion);
}

#[test]
fn evolver_classifies_timeout_keyword() {
    let lib = Arc::new(parking_lot::RwLock::new(FailurePatternLibrary::new()));
    let evolver = FailurePatternEvolver::new(lib);
    let report = make_report(vec![rec("n", "failed", "operation timed out")]);
    let s = evolver.analyze_and_suggest(&report);
    assert_eq!(s[0].category, FailureCategory::Timeout);
}

#[test]
fn evolver_classifies_missing_keyword() {
    let lib = Arc::new(parking_lot::RwLock::new(FailurePatternLibrary::new()));
    let evolver = FailurePatternEvolver::new(lib);
    let report = make_report(vec![rec("n", "failed", "dependency not found")]);
    let s = evolver.analyze_and_suggest(&report);
    assert_eq!(s[0].category, FailureCategory::DependencyMissing);
}

#[test]
fn evolver_classifies_tool_error_keyword() {
    let lib = Arc::new(parking_lot::RwLock::new(FailurePatternLibrary::new()));
    let evolver = FailurePatternEvolver::new(lib);
    let report = make_report(vec![rec("n", "failed", "tool error: invalid response")]);
    let s = evolver.analyze_and_suggest(&report);
    assert_eq!(s[0].category, FailureCategory::ToolError);
}

#[test]
fn evolver_classifies_unknown_as_logic_error() {
    let lib = Arc::new(parking_lot::RwLock::new(FailurePatternLibrary::new()));
    let evolver = FailurePatternEvolver::new(lib);
    let report = make_report(vec![rec("n", "failed", "weird custom error")]);
    let s = evolver.analyze_and_suggest(&report);
    assert_eq!(s[0].category, FailureCategory::LogicError);
}

#[test]
fn evolver_ignores_non_failure_status() {
    let lib = Arc::new(parking_lot::RwLock::new(FailurePatternLibrary::new()));
    let evolver = FailurePatternEvolver::new(lib);
    let report = make_report(vec![rec("n", "ok", "out of fuel")]);
    let s = evolver.analyze_and_suggest(&report);
    // "ok" status — not failure, not error — produces no suggestions.
    assert!(s.is_empty());
}

#[test]
fn evolver_sorts_suggestions_by_confidence_desc() {
    let lib = Arc::new(parking_lot::RwLock::new(FailurePatternLibrary::new()));
    // Pre-populate the library with several occurrences of the same pattern.
    {
        let mut l = lib.write();
        for _ in 0..5 {
            l.record_failure("hot_node", FailureCategory::Timeout, "timed out");
        }
    }
    let evolver = FailurePatternEvolver::new(lib);
    let report = make_report(vec![
        rec("hot_node", "failed", "timed out"),
        rec("new_node", "failed", "fuel exhausted"),
    ]);
    let s = evolver.analyze_and_suggest(&report);
    assert!(s.len() >= 2);
    // Sorted by confidence desc.
    for pair in s.windows(2) {
        assert!(pair[0].confidence >= pair[1].confidence);
    }
    assert_eq!(s[0].node_name, "hot_node");
}

// ---------------------------------------------------------------------------
// generate_evolution_ops
// ---------------------------------------------------------------------------

#[test]
fn evolver_generate_ops_no_failures_empty() {
    let lib = Arc::new(parking_lot::RwLock::new(FailurePatternLibrary::new()));
    let evolver = FailurePatternEvolver::new(lib);
    let report = make_report(vec![]);
    assert!(evolver.generate_evolution_ops(&report).is_empty());
}

#[test]
fn evolver_generate_ops_low_confidence_no_ops() {
    // First-time failure has confidence 0.3 → no ops.
    let lib = Arc::new(parking_lot::RwLock::new(FailurePatternLibrary::new()));
    let evolver = FailurePatternEvolver::new(lib);
    let report = make_report(vec![rec("n", "failed", "out of fuel")]);
    assert!(evolver.generate_evolution_ops(&report).is_empty());
}

#[test]
fn evolver_generate_ops_high_confidence_yields_op() {
    let lib = Arc::new(parking_lot::RwLock::new(FailurePatternLibrary::new()));
    {
        let mut l = lib.write();
        // 8 occurrences → confidence = 8/10 = 0.8 > 0.5.
        for _ in 0..8 {
            l.record_failure("hot", FailureCategory::Timeout, "timed out");
        }
    }
    let evolver = FailurePatternEvolver::new(lib);
    let report = make_report(vec![rec("hot", "failed", "timed out")]);
    let ops = evolver.generate_evolution_ops(&report);
    // Timeout → AdjustFuel + AddRetry.
    assert!(!ops.is_empty());
}

// ---------------------------------------------------------------------------
// MutationSuggestion
// ---------------------------------------------------------------------------

#[test]
fn mutation_suggestion_clone_preserves_fields() {
    let s = MutationSuggestion {
        node_name: "n".to_string(),
        category: FailureCategory::LogicError,
        description: "x".to_string(),
        confidence: 0.7,
    };
    let c = s.clone();
    assert_eq!(c.node_name, "n");
    assert_eq!(c.category, FailureCategory::LogicError);
    assert_eq!(c.confidence, 0.7);
}

// ---------------------------------------------------------------------------
// ErrorCategory (from reflect module)
// ---------------------------------------------------------------------------

#[test]
fn error_category_classify_resource() {
    assert_eq!(
        ErrorCategory::classify("ResourceExhausted happened"),
        ErrorCategory::ResourceExhausted
    );
    assert_eq!(
        ErrorCategory::classify("fuel budget exceeded"),
        ErrorCategory::ResourceExhausted
    );
    assert_eq!(
        ErrorCategory::classify("out of memory"),
        ErrorCategory::ResourceExhausted
    );
}

#[test]
fn error_category_classify_timeout() {
    assert_eq!(
        ErrorCategory::classify("request timeout"),
        ErrorCategory::Timeout
    );
    assert_eq!(
        ErrorCategory::classify("operation Timeout exceeded"),
        ErrorCategory::Timeout
    );
    assert_eq!(
        ErrorCategory::classify("deadline missed"),
        ErrorCategory::Timeout
    );
}

#[test]
fn error_category_classify_type_error() {
    assert_eq!(
        ErrorCategory::classify("type mismatch detected"),
        ErrorCategory::TypeError
    );
}

#[test]
fn error_category_classify_crash() {
    assert_eq!(
        ErrorCategory::classify("node crash detected"),
        ErrorCategory::Crash
    );
}

#[test]
fn error_category_classify_unknown() {
    assert_eq!(
        ErrorCategory::classify("mysterious error"),
        ErrorCategory::Unknown
    );
}

#[test]
fn error_category_severity() {
    assert!((ErrorCategory::ResourceExhausted.severity() - 0.8).abs() < 1e-9);
    assert!((ErrorCategory::Timeout.severity() - 0.5).abs() < 1e-9);
    assert!((ErrorCategory::TypeError.severity() - 0.6).abs() < 1e-9);
    assert!((ErrorCategory::Crash.severity() - 1.0).abs() < 1e-9);
    assert!((ErrorCategory::Unknown.severity() - 0.7).abs() < 1e-9);
}
