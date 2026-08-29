//! Integration tests for sisyphus-intel `synthesis` module's pure data
//! types. Covers [`SynthesisResult`], [`ActionType`], [`DynamicNode`],
//! [`DynamicEdge`], and [`DynamicSopPlan`].

use sisyphus_intel::synthesis::{
    ActionType, DynamicEdge, DynamicNode, DynamicSopPlan, SynthesisResult,
};

// ---------------------------------------------------------------------------
// ActionType
// ---------------------------------------------------------------------------

#[test]
fn action_type_variants_distinct() {
    let variants = [
        ActionType::Fetch,
        ActionType::Process,
        ActionType::Sink,
        ActionType::Search,
        ActionType::Validate,
    ];
    let mut seen = std::collections::HashSet::new();
    for v in variants {
        let d = format!("{v:?}");
        assert!(seen.insert(d.clone()));
    }
}

#[test]
fn action_type_equality() {
    // ActionType derives PartialEq.
    assert_eq!(ActionType::Fetch, ActionType::Fetch);
    assert_ne!(ActionType::Fetch, ActionType::Process);
}

// ---------------------------------------------------------------------------
// DynamicNode
// ---------------------------------------------------------------------------

#[test]
fn dynamic_node_field_assignment() {
    let n = DynamicNode {
        id: "n1".into(),
        name: "First".into(),
        tool_id: "read".into(),
        args: serde_json::json!({"path": "/etc/hosts"}),
    };
    assert_eq!(n.id, "n1");
    assert_eq!(n.tool_id, "read");
}

#[test]
fn dynamic_node_serde() {
    let n = DynamicNode {
        id: "n".into(),
        name: "n".into(),
        tool_id: "t".into(),
        args: serde_json::json!({}),
    };
    let j = serde_json::to_string(&n).unwrap();
    let back: DynamicNode = serde_json::from_str(&j).unwrap();
    assert_eq!(back.id, "n");
}

// ---------------------------------------------------------------------------
// DynamicEdge
// ---------------------------------------------------------------------------

#[test]
fn dynamic_edge_field_assignment() {
    let e = DynamicEdge {
        from: "a".into(),
        to: "b".into(),
    };
    assert_eq!(e.from, "a");
    assert_eq!(e.to, "b");
}

#[test]
fn dynamic_edge_serde() {
    let e = DynamicEdge {
        from: "a".into(),
        to: "b".into(),
    };
    let j = serde_json::to_string(&e).unwrap();
    let back: DynamicEdge = serde_json::from_str(&j).unwrap();
    assert_eq!(back.from, "a");
}

// ---------------------------------------------------------------------------
// DynamicSopPlan
// ---------------------------------------------------------------------------

#[test]
fn dynamic_sop_plan_field_assignment() {
    let p = DynamicSopPlan {
        nodes: vec![],
        edges: vec![],
        start_node: "n0".into(),
    };
    assert!(p.nodes.is_empty());
    assert_eq!(p.start_node, "n0");
}

#[test]
fn dynamic_sop_plan_serde() {
    let p = DynamicSopPlan {
        nodes: vec![DynamicNode {
            id: "n".into(),
            name: "n".into(),
            tool_id: "t".into(),
            args: serde_json::json!(null),
        }],
        edges: vec![DynamicEdge {
            from: "a".into(),
            to: "b".into(),
        }],
        start_node: "n".into(),
    };
    let j = serde_json::to_string(&p).unwrap();
    let back: DynamicSopPlan = serde_json::from_str(&j).unwrap();
    assert_eq!(back.nodes.len(), 1);
    assert_eq!(back.edges.len(), 1);
}

// ---------------------------------------------------------------------------
// SynthesisResult (construct without SOPGraph)
// ---------------------------------------------------------------------------

#[test]
fn synthesis_result_construct() {
    // We can build a SynthesisResult from a default SOPGraph and
    // verify the field access.
    use std::sync::Arc;
    let g = Arc::new(sisyphus_types::graph::SOPGraph::new());
    let r = SynthesisResult {
        graph: g,
        start_node: petgraph::graph::NodeIndex::new(0),
        plan_description: "plan".into(),
    };
    assert_eq!(r.plan_description, "plan");
}
