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

// ---------------------------------------------------------------------------
// ActionType additional coverage
// ---------------------------------------------------------------------------

#[test]
fn action_type_debug_format() {
    for v in [
        ActionType::Fetch,
        ActionType::Process,
        ActionType::Sink,
        ActionType::Search,
        ActionType::Validate,
    ] {
        let _ = format!("{:?}", v);
    }
}

#[test]
fn action_type_clone() {
    let a = ActionType::Fetch;
    let b = a.clone();
    assert_eq!(a, b);
}

#[test]
fn dynamic_node_unicode_fields() {
    let n = DynamicNode {
        id: "节点-α".into(),
        name: "节点名".into(),
        tool_id: "工具-β".into(),
        args: serde_json::json!({"k": "v"}),
    };
    assert_eq!(n.id, "节点-α");
    let j = serde_json::to_string(&n).unwrap();
    let back: DynamicNode = serde_json::from_str(&j).unwrap();
    assert_eq!(back.id, "节点-α");
}

#[test]
fn dynamic_edge_unicode() {
    let e = DynamicEdge {
        from: "源-α".into(),
        to: "目-β".into(),
    };
    assert_eq!(e.from, "源-α");
    let j = serde_json::to_string(&e).unwrap();
    let back: DynamicEdge = serde_json::from_str(&j).unwrap();
    assert_eq!(back.to, "目-β");
}

#[test]
fn dynamic_sop_plan_empty() {
    let p = DynamicSopPlan {
        nodes: vec![],
        edges: vec![],
        start_node: "".into(),
    };
    assert!(p.nodes.is_empty());
    assert!(p.edges.is_empty());
    assert_eq!(p.start_node, "");
}

#[test]
fn dynamic_sop_plan_multi_node_serde() {
    let p = DynamicSopPlan {
        nodes: vec![
            DynamicNode {
                id: "n1".into(),
                name: "first".into(),
                tool_id: "t1".into(),
                args: serde_json::json!({}),
            },
            DynamicNode {
                id: "n2".into(),
                name: "second".into(),
                tool_id: "t2".into(),
                args: serde_json::json!({"x": 1}),
            },
        ],
        edges: vec![DynamicEdge {
            from: "n1".into(),
            to: "n2".into(),
        }],
        start_node: "n1".into(),
    };
    let j = serde_json::to_string(&p).unwrap();
    let back: DynamicSopPlan = serde_json::from_str(&j).unwrap();
    assert_eq!(back.nodes.len(), 2);
    assert_eq!(back.edges.len(), 1);
    assert_eq!(back.start_node, "n1");
}

#[test]
fn dynamic_sop_plan_clone() {
    let p = DynamicSopPlan {
        nodes: vec![],
        edges: vec![],
        start_node: "s".into(),
    };
    let cloned = p.clone();
    assert_eq!(cloned.start_node, "s");
}

#[test]
fn dynamic_node_with_complex_args() {
    let n = DynamicNode {
        id: "n".into(),
        name: "n".into(),
        tool_id: "t".into(),
        args: serde_json::json!({
            "nested": {"key": [1, 2, 3]},
            "null": null,
            "bool": true,
        }),
    };
    let j = serde_json::to_string(&n).unwrap();
    let back: DynamicNode = serde_json::from_str(&j).unwrap();
    assert_eq!(back.args["nested"]["key"][1], 2);
    assert_eq!(back.args["bool"], true);
}

#[test]
fn dynamic_edge_serde_round_trip() {
    let e = DynamicEdge {
        from: "a".into(),
        to: "b".into(),
    };
    let j = serde_json::to_string(&e).unwrap();
    let back: DynamicEdge = serde_json::from_str(&j).unwrap();
    assert_eq!(back.from, "a");
    assert_eq!(back.to, "b");
}

// ---------------------------------------------------------------------------
// Additional coverage
// ---------------------------------------------------------------------------

#[test]
fn synthesis_result_field_access_unicode() {
    use std::sync::Arc;
    let g = Arc::new(sisyphus_types::graph::SOPGraph::new());
    let r = SynthesisResult {
        graph: g,
        start_node: petgraph::graph::NodeIndex::new(0),
        plan_description: "计划-α".into(),
    };
    assert_eq!(r.plan_description, "计划-α");
}

#[test]
fn synthesis_result_plan_with_long_description() {
    use std::sync::Arc;
    let g = Arc::new(sisyphus_types::graph::SOPGraph::new());
    let long = "x".repeat(5000);
    let r = SynthesisResult {
        graph: g,
        start_node: petgraph::graph::NodeIndex::new(0),
        plan_description: long.clone(),
    };
    assert_eq!(r.plan_description.len(), 5000);
}

#[test]
fn dynamic_node_with_max_id_length() {
    let n = DynamicNode {
        id: "x".repeat(1000),
        name: "n".into(),
        tool_id: "t".into(),
        args: serde_json::json!({}),
    };
    let j = serde_json::to_string(&n).unwrap();
    let back: DynamicNode = serde_json::from_str(&j).unwrap();
    assert_eq!(back.id.len(), 1000);
}

#[test]
fn dynamic_node_with_null_args() {
    let n = DynamicNode {
        id: "n".into(),
        name: "name".into(),
        tool_id: "t".into(),
        args: serde_json::Value::Null,
    };
    let j = serde_json::to_string(&n).unwrap();
    let back: DynamicNode = serde_json::from_str(&j).unwrap();
    assert!(back.args.is_null());
}

#[test]
fn dynamic_sop_plan_with_unicode_start_node() {
    let p = DynamicSopPlan {
        nodes: vec![],
        edges: vec![],
        start_node: "起始节点-α".into(),
    };
    assert_eq!(p.start_node, "起始节点-α");
    let j = serde_json::to_string(&p).unwrap();
    let back: DynamicSopPlan = serde_json::from_str(&j).unwrap();
    assert_eq!(back.start_node, "起始节点-α");
}

#[test]
fn dynamic_sop_plan_clone_preserves_start() {
    let p = DynamicSopPlan {
        nodes: vec![],
        edges: vec![],
        start_node: "main".into(),
    };
    let cloned = p.clone();
    assert_eq!(cloned.start_node, "main");
}

#[test]
fn dynamic_sop_plan_debug() {
    let p = DynamicSopPlan {
        nodes: vec![],
        edges: vec![],
        start_node: "s".into(),
    };
    let dbg = format!("{:?}", p);
    assert!(dbg.contains("DynamicSopPlan"));
    assert!(dbg.contains("start_node"));
}

#[test]
fn action_type_inequality() {
    assert_ne!(ActionType::Fetch, ActionType::Process);
    assert_ne!(ActionType::Sink, ActionType::Search);
    assert_ne!(ActionType::Validate, ActionType::Fetch);
}
