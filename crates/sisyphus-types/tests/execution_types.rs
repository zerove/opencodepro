//! Integration tests for sisyphus-types `execution` module (more
//! types beyond the already-covered `NodeOutput` and `ExecutionEvent`).

use sisyphus_types::execution::{
    CognitiveAction, CognitiveStep, CognitiveTraceSummary, CritiqueResult, EvolutionResponse,
    ExecutionContext, ExecutionFeedback, ExecutionHistory, ExecutionTrace, FeedbackStatus,
    HistoryRecord, NodeOutput, PlanEdge, SupervisorDecision, TracePoint, VerificationCriteria,
};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// SupervisorDecision
// ---------------------------------------------------------------------------

#[test]
fn supervisor_decision_continue_serde() {
    let d = SupervisorDecision::Continue { deviation: 0.1 };
    let j = serde_json::to_string(&d).unwrap();
    let back: SupervisorDecision = serde_json::from_str(&j).unwrap();
    match back {
        SupervisorDecision::Continue { deviation } => assert!((deviation - 0.1).abs() < 1e-9),
        _ => panic!("expected Continue"),
    }
}

#[test]
fn supervisor_decision_pivot_serde() {
    let d = SupervisorDecision::Pivot {
        reason: "stuck".into(),
        suggested_correction: None,
    };
    let j = serde_json::to_string(&d).unwrap();
    let back: SupervisorDecision = serde_json::from_str(&j).unwrap();
    match back {
        SupervisorDecision::Pivot { reason, .. } => assert_eq!(reason, "stuck"),
        _ => panic!("expected Pivot"),
    }
}

#[test]
fn supervisor_decision_variants_distinct() {
    let d1 = SupervisorDecision::Continue { deviation: 0.0 };
    let d2 = SupervisorDecision::Pivot {
        reason: "x".into(),
        suggested_correction: None,
    };
    let s1 = format!("{d1:?}");
    let s2 = format!("{d2:?}");
    assert_ne!(s1, s2);
}

// ---------------------------------------------------------------------------
// NodeOutput (more coverage)
// ---------------------------------------------------------------------------

#[test]
fn node_output_default_constructs_null() {
    let n = NodeOutput(Arc::new(serde_json::Value::Null));
    assert_eq!(n.to_string(), "null");
}

#[test]
fn node_output_serde_via_inner_value() {
    let n = NodeOutput(Arc::new(serde_json::json!({"k": "v"})));
    let j = serde_json::to_string(&n).unwrap();
    assert!(j.contains("k"));
}

// ---------------------------------------------------------------------------
// CritiqueResult
// ---------------------------------------------------------------------------

#[test]
fn critique_result_field_assignment() {
    let r = CritiqueResult {
        critical_findings: vec!["x".into()],
        risk_score: 0.9,
        suggested_refinement: "refine".into(),
        should_proceed: true,
    };
    assert!(r.should_proceed);
    assert_eq!(r.critical_findings.len(), 1);
}

#[test]
fn critique_result_serde() {
    let r = CritiqueResult {
        critical_findings: vec![],
        risk_score: 0.5,
        suggested_refinement: "x".into(),
        should_proceed: false,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: CritiqueResult = serde_json::from_str(&j).unwrap();
    assert!(!back.should_proceed);
}

// ---------------------------------------------------------------------------
// CognitiveStep + CognitiveAction
// ---------------------------------------------------------------------------

#[test]
fn cognitive_step_field_assignment() {
    let s = CognitiveStep {
        id: "s1".into(),
        name: "First step".into(),
        action: CognitiveAction::ToolCall {
            tool_id: "t".into(),
            args: serde_json::json!({}),
        },
        verification: None,
        priority: 1,
    };
    assert_eq!(s.id, "s1");
    assert_eq!(s.priority, 1);
}

#[test]
fn cognitive_step_serde() {
    let s = CognitiveStep {
        id: "s".into(),
        name: "n".into(),
        action: CognitiveAction::ShellExecute {
            command: "ls".into(),
            timeout: 10,
        },
        verification: Some(VerificationCriteria {
            check_command: None,
            expected_pattern: None,
            description: "d".into(),
        }),
        priority: 0,
    };
    let j = serde_json::to_string(&s).unwrap();
    let back: CognitiveStep = serde_json::from_str(&j).unwrap();
    assert_eq!(back.id, "s");
}

#[test]
fn cognitive_action_variants_distinct() {
    let variants = [
        CognitiveAction::ToolCall {
            tool_id: "t".into(),
            args: serde_json::json!(null),
        },
        CognitiveAction::FileEdit {
            path: "p".into(),
            patch: "patch".into(),
        },
        CognitiveAction::ShellExecute {
            command: "c".into(),
            timeout: 1,
        },
    ];
    let mut seen = std::collections::HashSet::new();
    for v in variants {
        let d = format!("{v:?}");
        assert!(seen.insert(d.clone()));
    }
}

#[test]
fn cognitive_action_serde_tool_call() {
    let a = CognitiveAction::ToolCall {
        tool_id: "t".into(),
        args: serde_json::json!({"k": "v"}),
    };
    let j = serde_json::to_string(&a).unwrap();
    let back: CognitiveAction = serde_json::from_str(&j).unwrap();
    match back {
        CognitiveAction::ToolCall { tool_id, .. } => assert_eq!(tool_id, "t"),
        _ => panic!("expected ToolCall"),
    }
}

// ---------------------------------------------------------------------------
// VerificationCriteria / PlanEdge
// ---------------------------------------------------------------------------

#[test]
fn verification_criteria_field_assignment() {
    let v = VerificationCriteria {
        check_command: Some("cargo build".into()),
        expected_pattern: Some(r"warning:".into()),
        description: "d".into(),
    };
    assert!(v.check_command.is_some());
    assert!(v.expected_pattern.is_some());
}

#[test]
fn plan_edge_field_assignment() {
    let e = PlanEdge {
        from: "a".into(),
        to: "b".into(),
    };
    assert_eq!(e.from, "a");
    assert_eq!(e.to, "b");
}

#[test]
fn plan_edge_serde() {
    let e = PlanEdge {
        from: "a".into(),
        to: "b".into(),
    };
    let j = serde_json::to_string(&e).unwrap();
    let back: PlanEdge = serde_json::from_str(&j).unwrap();
    assert_eq!(back.from, "a");
}

// ---------------------------------------------------------------------------
// HistoryRecord / ExecutionHistory
// ---------------------------------------------------------------------------

#[test]
fn history_record_field_assignment() {
    let h = HistoryRecord {
        timestamp: 12345,
        trace: "t".into(),
        deviation_score: 0.1,
        success: true,
        cost: 100,
    };
    assert_eq!(h.timestamp, 12345);
    assert!(h.success);
    assert_eq!(h.cost, 100);
}

#[test]
fn history_record_serde() {
    let h = HistoryRecord {
        timestamp: 0,
        trace: "".into(),
        deviation_score: 0.0,
        success: false,
        cost: 0,
    };
    let j = serde_json::to_string(&h).unwrap();
    let back: HistoryRecord = serde_json::from_str(&j).unwrap();
    assert!(!back.success);
}

#[test]
fn execution_history_field_assignment() {
    let h = ExecutionHistory {
        sop_id: "s1".into(),
        records: vec![],
    };
    assert_eq!(h.sop_id, "s1");
    assert!(h.records.is_empty());
}

// ---------------------------------------------------------------------------
// TracePoint / ExecutionTrace
// ---------------------------------------------------------------------------

#[test]
fn trace_point_field_assignment() {
    let t = TracePoint {
        node_name: "n".into(),
        timestamp: 1,
        output_summary: "summary".into(),
        deviation_score: 0.0,
        status: Some("ok".into()),
    };
    assert_eq!(t.node_name, "n");
    assert_eq!(t.status.as_deref(), Some("ok"));
}

#[test]
fn execution_trace_field_assignment() {
    let t = ExecutionTrace {
        sop_id: "s1".into(),
        iteration: 3,
        points: vec![],
        final_decision: SupervisorDecision::Continue { deviation: 0.0 },
    };
    assert_eq!(t.iteration, 3);
    assert!(t.points.is_empty());
}

// ---------------------------------------------------------------------------
// ExecutionFeedback / FeedbackStatus
// ---------------------------------------------------------------------------

#[test]
fn execution_feedback_field_assignment() {
    let f = ExecutionFeedback {
        plan_id: "p1".into(),
        step_id: "s1".into(),
        status: FeedbackStatus::Success,
        output: "ok".into(),
        error: None,
        state_snapshot: None,
    };
    assert_eq!(f.plan_id, "p1");
}

#[test]
fn feedback_status_variants_distinct() {
    let variants = [
        FeedbackStatus::Success,
        FeedbackStatus::Failed,
        FeedbackStatus::Ambiguous,
    ];
    let mut seen = std::collections::HashSet::new();
    for v in variants {
        let d = format!("{v:?}");
        assert!(seen.insert(d.clone()));
    }
}

// ---------------------------------------------------------------------------
// EvolutionResponse
// ---------------------------------------------------------------------------

#[test]
fn evolution_response_field_assignment() {
    let e = EvolutionResponse {
        should_pivot: true,
        revised_plan: None,
        reflection_report: "x".into(),
    };
    assert!(e.should_pivot);
    assert!(e.revised_plan.is_none());
}

#[test]
fn evolution_response_serde() {
    let e = EvolutionResponse {
        should_pivot: false,
        revised_plan: None,
        reflection_report: "report".into(),
    };
    let j = serde_json::to_string(&e).unwrap();
    let back: EvolutionResponse = serde_json::from_str(&j).unwrap();
    assert!(!back.should_pivot);
    assert_eq!(back.reflection_report, "report");
}

// ---------------------------------------------------------------------------
// ExecutionContext
// ---------------------------------------------------------------------------

#[test]
fn execution_context_default_works() {
    let _c = ExecutionContext::default();
}

#[test]
fn execution_context_with_drop() {
    // Create and drop a context to ensure no panics.
    let c = ExecutionContext::default();
    drop(c);
}

// ---------------------------------------------------------------------------
// CognitiveTraceSummary
// ---------------------------------------------------------------------------

#[test]
fn cognitive_trace_summary_field_assignment() {
    let s = CognitiveTraceSummary {
        total_decisions: 10,
        step_count: 3,
        reflection_count: 5,
        avg_deviation: 0.1,
        avg_confidence: 0.9,
        avg_stability: 0.95,
        avg_info_gain: 0.2,
        total_token_cost: 1000,
        decision_counts: Default::default(),
        first_timestamp_ms: 0,
        last_timestamp_ms: 5000,
    };
    assert_eq!(s.total_decisions, 10);
    assert_eq!(s.step_count, 3);
    assert_eq!(s.total_token_cost, 1000);
}

#[test]
fn cognitive_trace_summary_serde() {
    let s = CognitiveTraceSummary {
        total_decisions: 1,
        step_count: 0,
        reflection_count: 0,
        avg_deviation: 0.0,
        avg_confidence: 1.0,
        avg_stability: 1.0,
        avg_info_gain: 0.0,
        total_token_cost: 0,
        decision_counts: Default::default(),
        first_timestamp_ms: 0,
        last_timestamp_ms: 0,
    };
    let j = serde_json::to_string(&s).unwrap();
    let back: CognitiveTraceSummary = serde_json::from_str(&j).unwrap();
    assert_eq!(back.total_decisions, 1);
}
