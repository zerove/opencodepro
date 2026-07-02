use super::*;
use crate::AdaptiveFuelPricer;
use std::sync::Arc;

fn make_pred(estimated_cost: u64) -> SimulationResult {
    SimulationResult {
        path_id: "t".into(),
        success_probability: 0.5,
        estimated_cost,
        predicted_outcome: "?".into(),
    }
}

fn make_record(node: &str, status: &str, output: &str) -> RecordedNode {
    RecordedNode {
        node: node.into(),
        status: status.into(),
        output: output.into(),
    }
}

// StandardReflector::parse_trace
#[test]
fn test_parse_valid_json_lines() {
    let trace = r#"{"node":"a","status":"ok","output":"done"}
{"node":"b","status":"error","output":"crash"}"#;
    let records = StandardReflector::parse_trace(trace);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].node, "a");
    assert_eq!(records[1].status, "error");
}

#[test]
fn test_parse_skip_invalid() {
    let trace = "bad\n{\"node\":\"a\",\"status\":\"ok\",\"output\":\"\"}\ngarbage";
    let records = StandardReflector::parse_trace(trace);
    assert_eq!(records.len(), 1);
}

#[test]
fn test_parse_empty() {
    assert!(StandardReflector::parse_trace("").is_empty());
}

// StandardReflector::analyze
#[test]
fn test_analyze_empty() {
    let (dev, s) = StandardReflector::analyze(&[]);
    assert_eq!(dev, 0.0);
    assert!(s.iter().any(|x| x.contains("No execution records")));
}

#[test]
fn test_analyze_all_ok() {
    let r = vec![make_record("a", "ok", "out"), make_record("b", "ok", "out")];
    let (dev, s) = StandardReflector::analyze(&r);
    assert_eq!(dev, 0.0);
    assert!(s.iter().any(|x| x.contains("within expected parameters")));
}

#[test]
fn test_analyze_errors() {
    let r = vec![
        make_record("a", "ok", "out"),
        make_record("b", "error", "boom"),
    ];
    let (dev, s) = StandardReflector::analyze(&r);
    assert!(dev > 0.3);
    assert!(s.iter().any(|x| x.contains("Node failures")));
}

#[test]
fn test_analyze_resource_exhausted() {
    let r = vec![make_record("a", "error", "ResourceExhausted")];
    let (_, s) = StandardReflector::analyze(&r);
    assert!(s.iter().any(|x| x.contains("Resource exhaustion")));
}

#[test]
fn test_analyze_empty_outputs() {
    let r = vec![
        make_record("a", "ok", ""),
        make_record("b", "ok", "null"),
        make_record("c", "ok", "real"),
    ];
    let (_, s) = StandardReflector::analyze(&r);
    assert!(s.iter().any(|x| x.contains("empty output")));
}

// StandardReflector::reflect (async)
#[tokio::test]
async fn test_reflect_structured() {
    let trace = r#"{"node":"x","status":"ok","output":"data"}"#;
    let report = StandardReflector.reflect(trace, &make_pred(10)).await.unwrap();
    assert_eq!(report.deviation_score, 0.0);
}

#[tokio::test]
async fn test_reflect_fallback() {
    let trace = "ResourceExhausted";
    let report = StandardReflector.reflect(trace, &make_pred(100)).await.unwrap();
    assert_eq!(report.deviation_score, 0.8);
}

// ErrorCategory::classify
#[test]
fn test_error_category_classify() {
    assert_eq!(
        ErrorCategory::classify("ResourceExhausted: fuel limit"),
        ErrorCategory::ResourceExhausted
    );
    assert_eq!(
        ErrorCategory::classify("fuel exhausted"),
        ErrorCategory::ResourceExhausted
    );
    assert_eq!(
        ErrorCategory::classify("out of memory error"),
        ErrorCategory::ResourceExhausted
    );

    assert_eq!(
        ErrorCategory::classify("timeout exceeded"),
        ErrorCategory::Timeout
    );
    assert_eq!(
        ErrorCategory::classify("deadline exceeded"),
        ErrorCategory::Timeout
    );
    assert_eq!(
        ErrorCategory::classify("Timeout after 30s"),
        ErrorCategory::Timeout
    );

    assert_eq!(
        ErrorCategory::classify("TypeError: mismatch"),
        ErrorCategory::TypeError
    );
    assert_eq!(
        ErrorCategory::classify("type mismatch"),
        ErrorCategory::TypeError
    );

    assert_eq!(
        ErrorCategory::classify("panic: index out of bounds"),
        ErrorCategory::Crash
    );
    assert_eq!(ErrorCategory::classify("segfault"), ErrorCategory::Crash);
    assert_eq!(
        ErrorCategory::classify("crash in module"),
        ErrorCategory::Crash
    );

    assert_eq!(
        ErrorCategory::classify("unknown error"),
        ErrorCategory::Unknown
    );
}

#[test]
fn test_error_category_severity() {
    assert_eq!(ErrorCategory::Crash.severity(), 1.0);
    assert_eq!(ErrorCategory::ResourceExhausted.severity(), 0.8);
    assert_eq!(ErrorCategory::TypeError.severity(), 0.6);
    assert_eq!(ErrorCategory::Timeout.severity(), 0.5);
    assert_eq!(ErrorCategory::Unknown.severity(), 0.7);
}

// StandardEvolver
#[test]
fn test_standard_evolver_with_adaptive_pricer() {
    let evolver = StandardEvolver::with_fuel_pricer(Arc::new(AdaptiveFuelPricer::new()));
    assert!(std::ptr::addr_of!(evolver.fuel_pricer) as usize != 0);
}
