//! Integration tests for sisyphus-intel `FailurePatternLibrary` and
//! `FailurePatternEvolver`.
//!
//! Exercises the public `FailurePatternLibrary::record_failure` /
//! `get_node_patterns` / `most_common_failure` / `get_hotspots`,
//! the `FailureCategory` enum, and `FailurePatternEvolver::analyze_and_suggest` /
//! `generate_evolution_ops`.

use parking_lot::RwLock;
use sisyphus_intel::failure_pattern_evolver::{
    FailureCategory, FailurePatternEvolver, FailurePatternLibrary,
};
use sisyphus_intel::reflect::{RecordedNode, ReflectionReport};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// FailurePatternLibrary
// ---------------------------------------------------------------------------

#[test]
fn library_starts_empty() {
    let lib = FailurePatternLibrary::new();
    assert!(lib.get_hotspots().is_empty());
    assert!(lib.get_node_patterns("any").is_empty());
    assert!(lib.most_common_failure("any").is_none());
}

#[test]
fn library_records_first_failure_creates_pattern() {
    let mut lib = FailurePatternLibrary::new();
    lib.record_failure("node1", FailureCategory::FuelExhaustion, "out of fuel");
    let patterns = lib.get_node_patterns("node1");
    assert_eq!(patterns.len(), 1);
    let p = patterns[0];
    assert_eq!(p.affected_node, "node1");
    assert_eq!(p.category, FailureCategory::FuelExhaustion);
    assert_eq!(p.description, "out of fuel");
    assert_eq!(p.occurrence_count, 1);
    assert_eq!(p.evolution_ops.len(), 0);
    assert!((p.success_rate_after_fix - 0.0).abs() < 1e-9);
}

#[test]
fn library_repeat_failure_increments_count() {
    let mut lib = FailurePatternLibrary::new();
    for _ in 0..5 {
        lib.record_failure("node1", FailureCategory::Timeout, "slow");
    }
    let patterns = lib.get_node_patterns("node1");
    assert_eq!(patterns.len(), 1, "same node+category merges into one pattern");
    assert_eq!(patterns[0].occurrence_count, 5);
}

#[test]
fn library_separates_categories_per_node() {
    let mut lib = FailurePatternLibrary::new();
    lib.record_failure("node1", FailureCategory::FuelExhaustion, "a");
    lib.record_failure("node1", FailureCategory::Timeout, "b");
    lib.record_failure("node1", FailureCategory::ToolError, "c");
    let patterns = lib.get_node_patterns("node1");
    assert_eq!(patterns.len(), 3);
}

#[test]
fn library_separates_nodes_per_category() {
    let mut lib = FailurePatternLibrary::new();
    lib.record_failure("nodeA", FailureCategory::LogicError, "x");
    lib.record_failure("nodeB", FailureCategory::LogicError, "y");
    assert_eq!(lib.get_node_patterns("nodeA").len(), 1);
    assert_eq!(lib.get_node_patterns("nodeB").len(), 1);
}

#[test]
fn library_most_common_failure_returns_highest_count() {
    let mut lib = FailurePatternLibrary::new();
    lib.record_failure("node1", FailureCategory::ToolError, "a");
    lib.record_failure("node1", FailureCategory::ToolError, "b");
    lib.record_failure("node1", FailureCategory::Timeout, "c");
    assert_eq!(
        lib.most_common_failure("node1"),
        Some(FailureCategory::ToolError)
    );
}

#[test]
fn library_most_common_failure_unknown_node_is_none() {
    let lib = FailurePatternLibrary::new();
    assert!(lib.most_common_failure("nope").is_none());
}

#[test]
fn library_hotspots_sorted_by_occurrence_desc() {
    let mut lib = FailurePatternLibrary::new();
    lib.record_failure("a", FailureCategory::ToolError, "");
    lib.record_failure("a", FailureCategory::ToolError, "");
    lib.record_failure("a", FailureCategory::ToolError, "");
    lib.record_failure("b", FailureCategory::Timeout, "");
    lib.record_failure("c", FailureCategory::LogicError, "");
    lib.record_failure("c", FailureCategory::LogicError, "");
    let hotspots = lib.get_hotspots();
    assert_eq!(hotspots.len(), 3);
    assert_eq!(hotspots[0].affected_node, "a");
    assert_eq!(hotspots[0].occurrence_count, 3);
    assert_eq!(hotspots[1].affected_node, "c");
    assert_eq!(hotspots[2].affected_node, "b");
}

#[test]
fn library_default_matches_new() {
    let lib: FailurePatternLibrary = Default::default();
    assert!(lib.get_hotspots().is_empty());
}

// ---------------------------------------------------------------------------
// FailurePatternEvolver::analyze_and_suggest
// ---------------------------------------------------------------------------

fn make_report(records: Vec<(&str, &str, &str)>) -> ReflectionReport {
    // (node, status, output)
    ReflectionReport {
        original_sop_id: "sop1".into(),
        deviation_score: 0.5,
        suggested_improvements: vec![],
        improvement_signals: vec![],
        evolved_sop_structure: None,
        records: records
            .into_iter()
            .map(|(node, status, output)| RecordedNode {
                node: node.into(),
                status: status.into(),
                output: output.into(),
            })
            .collect(),
    }
}

#[test]
fn evolver_no_records_returns_no_suggestions() {
    let lib = Arc::new(RwLock::new(FailurePatternLibrary::new()));
    let evolver = FailurePatternEvolver::new(lib);
    let report = make_report(vec![]);
    assert!(evolver.analyze_and_suggest(&report).is_empty());
}

#[test]
fn evolver_first_failure_for_node_suggests_low_confidence() {
    let lib = Arc::new(RwLock::new(FailurePatternLibrary::new()));
    let evolver = FailurePatternEvolver::new(lib);
    let report = make_report(vec![("n1", "failed", "boom")]);
    let suggestions = evolver.analyze_and_suggest(&report);
    assert_eq!(suggestions.len(), 1);
    assert_eq!(suggestions[0].node_name, "n1");
    // First-time pattern detection: confidence is 0.3
    assert!((suggestions[0].confidence - 0.3).abs() < 1e-9);
}

#[test]
fn evolver_repeated_pattern_yields_high_confidence_suggestion() {
    let mut lib = FailurePatternLibrary::new();
    // Pre-load 5 occurrences of the same failure so the next one exceeds
    // the 3-occurrence threshold. The report output must contain "fuel"
    // so classify_failure returns FuelExhaustion, matching the pre-loaded
    // pattern category.
    for _ in 0..5 {
        lib.record_failure("n1", FailureCategory::FuelExhaustion, "ran out");
    }
    let lib = Arc::new(RwLock::new(lib));
    let evolver = FailurePatternEvolver::new(lib);
    let report = make_report(vec![("n1", "failed", "fuel budget exceeded")]);
    let suggestions = evolver.analyze_and_suggest(&report);
    assert_eq!(suggestions.len(), 1);
    assert!(suggestions[0].confidence > 0.3);
    assert!(suggestions[0].confidence <= 0.95);
    assert!(suggestions[0].description.contains("n1"));
}

#[test]
fn evolver_ignores_successful_records() {
    let lib = Arc::new(RwLock::new(FailurePatternLibrary::new()));
    let evolver = FailurePatternEvolver::new(lib);
    let report = make_report(vec![
        ("n1", "ok", "all good"),
        ("n2", "success", "fine"),
        ("n3", "completed", "done"),
    ]);
    assert!(evolver.analyze_and_suggest(&report).is_empty());
}

#[test]
fn evolver_handles_error_status_as_failure() {
    let lib = Arc::new(RwLock::new(FailurePatternLibrary::new()));
    let evolver = FailurePatternEvolver::new(lib);
    let report = make_report(vec![("n1", "error", "boom")]);
    let suggestions = evolver.analyze_and_suggest(&report);
    assert_eq!(suggestions.len(), 1);
}

#[test]
fn evolver_multiple_failures_produces_multiple_suggestions() {
    let lib = Arc::new(RwLock::new(FailurePatternLibrary::new()));
    let evolver = FailurePatternEvolver::new(lib);
    let report = make_report(vec![
        ("n1", "failed", "a"),
        ("n2", "error", "b"),
        ("n3", "failed", "c"),
    ]);
    let suggestions = evolver.analyze_and_suggest(&report);
    assert_eq!(suggestions.len(), 3);
}

#[test]
fn evolver_suggestions_sorted_by_confidence_desc() {
    // n1: 10 occurrences → high confidence
    // n2: 1 occurrence → low confidence
    let mut lib = FailurePatternLibrary::new();
    for _ in 0..10 {
        lib.record_failure("n1", FailureCategory::FuelExhaustion, "x");
    }
    let lib = Arc::new(RwLock::new(lib));
    let evolver = FailurePatternEvolver::new(lib);
    let report = make_report(vec![
        ("n1", "failed", "x"),
        ("n2", "failed", "y"),
    ]);
    let suggestions = evolver.analyze_and_suggest(&report);
    assert_eq!(suggestions.len(), 2);
    assert!(suggestions[0].confidence >= suggestions[1].confidence);
}

// ---------------------------------------------------------------------------
// FailurePatternEvolver::generate_evolution_ops
// ---------------------------------------------------------------------------

#[test]
fn evolver_generate_evolution_ops_returns_empty_for_no_failures() {
    let lib = Arc::new(RwLock::new(FailurePatternLibrary::new()));
    let evolver = FailurePatternEvolver::new(lib);
    let report = make_report(vec![("n1", "ok", "fine")]);
    assert!(evolver.generate_evolution_ops(&report).is_empty());
}

#[test]
fn evolver_generate_evolution_ops_returns_ops_for_failures() {
    // Pre-record several occurrences to trip the threshold.
    let mut lib = FailurePatternLibrary::new();
    for _ in 0..6 {
        lib.record_failure("n1", FailureCategory::FuelExhaustion, "x");
    }
    let lib = Arc::new(RwLock::new(lib));
    let evolver = FailurePatternEvolver::new(lib);
    // Output must contain "fuel" so classify_failure returns FuelExhaustion
    // (matching the pre-loaded pattern). 6 occurrences → confidence 0.6,
    // which exceeds the 0.5 threshold required for EvolutionOp generation.
    let report = make_report(vec![("n1", "failed", "fuel budget exhausted")]);
    let ops = evolver.generate_evolution_ops(&report);
    // Should produce at least one op for the recurring failure.
    assert!(!ops.is_empty());
}
