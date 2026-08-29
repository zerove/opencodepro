//! Integration tests for sisyphus-types `graph` module's
//! `CompositeGraph`, `NodeKind`, and `SOPNodeConfig` data types.

use sisyphus_types::graph::{CompositeGraph, NodeKind, SOPNodeConfig};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// CompositeGraph
// ---------------------------------------------------------------------------

#[test]
fn composite_graph_new_is_empty() {
    let g = CompositeGraph::new();
    assert!(g.graphs.is_empty());
    assert!(g.dependencies.is_empty());
    assert!(g.execution_order.is_empty());
}

#[test]
fn composite_graph_default_trait() {
    let g: CompositeGraph = Default::default();
    assert!(g.graphs.is_empty());
}

#[test]
fn composite_graph_add_graph() {
    use sisyphus_types::graph::SOPGraphConfig;
    let mut g = CompositeGraph::new();
    g.add_graph("sop1".into(), SOPGraphConfig::default());
    assert!(g.graphs.contains_key("sop1"));
    assert_eq!(g.graphs.len(), 1);
}

#[test]
fn composite_graph_add_multiple_graphs() {
    use sisyphus_types::graph::SOPGraphConfig;
    let mut g = CompositeGraph::new();
    g.add_graph("a".into(), SOPGraphConfig::default());
    g.add_graph("b".into(), SOPGraphConfig::default());
    g.add_graph("c".into(), SOPGraphConfig::default());
    assert_eq!(g.graphs.len(), 3);
}

#[test]
fn composite_graph_add_dependency() {
    use sisyphus_types::graph::{DataMapping, GraphDependency};
    let mut g = CompositeGraph::new();
    g.add_dependency(GraphDependency {
        source_sop_id: "a".into(),
        target_sop_id: "b".into(),
        mappings: vec![],
    });
    assert_eq!(g.dependencies.len(), 1);
}

#[test]
fn composite_graph_add_dependency_with_mappings() {
    use sisyphus_types::graph::{DataMapping, GraphDependency};
    let mut g = CompositeGraph::new();
    g.add_dependency(GraphDependency {
        source_sop_id: "a".into(),
        target_sop_id: "b".into(),
        mappings: vec![DataMapping {
            source_node: "n1".into(),
            target_placeholder: "{n1}".into(),
        }],
    });
    assert_eq!(g.dependencies.len(), 1);
    assert_eq!(g.dependencies[0].mappings.len(), 1);
}

// ---------------------------------------------------------------------------
// NodeKind
// ---------------------------------------------------------------------------

#[test]
fn node_kind_wasm_serde() {
    let k = NodeKind::Wasm {
        tool_id: "t".into(),
        fuel_limit: 1000,
    };
    let j = serde_json::to_string(&k).unwrap();
    let back: NodeKind = serde_json::from_str(&j).unwrap();
    match back {
        NodeKind::Wasm {
            tool_id,
            fuel_limit,
        } => {
            assert_eq!(tool_id, "t");
            assert_eq!(fuel_limit, 1000);
        }
        _ => panic!("expected Wasm"),
    }
}

#[test]
fn node_kind_retry_serde() {
    let k = NodeKind::Retry {
        child: Box::new(NodeKind::Wasm {
            tool_id: "t".into(),
            fuel_limit: 100,
        }),
        max_retries: 3,
    };
    let j = serde_json::to_string(&k).unwrap();
    let back: NodeKind = serde_json::from_str(&j).unwrap();
    match back {
        NodeKind::Retry { max_retries, .. } => assert_eq!(max_retries, 3),
        _ => panic!("expected Retry"),
    }
}

#[test]
fn node_kind_branch_serde() {
    let mut branches = HashMap::new();
    branches.insert("ok".to_string(), 1);
    let k = NodeKind::Branch {
        branches,
        default_next: 0,
    };
    let j = serde_json::to_string(&k).unwrap();
    let back: NodeKind = serde_json::from_str(&j).unwrap();
    match back {
        NodeKind::Branch { default_next, .. } => assert_eq!(default_next, 0),
        _ => panic!("expected Branch"),
    }
}

#[test]
fn node_kind_human_intervention_serde() {
    let k = NodeKind::HumanIntervention {
        prompt: "approve?".into(),
        timeout_secs: 60,
    };
    let j = serde_json::to_string(&k).unwrap();
    let back: NodeKind = serde_json::from_str(&j).unwrap();
    match back {
        NodeKind::HumanIntervention { timeout_secs, .. } => assert_eq!(timeout_secs, 60),
        _ => panic!("expected HumanIntervention"),
    }
}

#[test]
fn node_kind_variants_distinct() {
    let variants: Vec<NodeKind> = vec![
        NodeKind::Wasm {
            tool_id: "t".into(),
            fuel_limit: 1,
        },
        NodeKind::Retry {
            child: Box::new(NodeKind::Wasm {
                tool_id: "t".into(),
                fuel_limit: 1,
            }),
            max_retries: 1,
        },
        NodeKind::Branch {
            branches: HashMap::new(),
            default_next: 0,
        },
        NodeKind::SubGraph {
            config: Box::new(sisyphus_types::graph::SOPGraphConfig::default()),
        },
        NodeKind::Test {
            command: "ls".into(),
            workdir: None,
        },
        NodeKind::VisualAnalysis {
            prompt: "x".into(),
            image_source: "y".into(),
        },
        NodeKind::VisualAction {
            action_type: "click".into(),
            target_selector: "a".into(),
            params: serde_json::json!(null),
        },
        NodeKind::HumanIntervention {
            prompt: "p".into(),
            timeout_secs: 1,
        },
    ];
    let mut seen = std::collections::HashSet::new();
    for v in variants {
        let d = format!("{v:?}");
        assert!(seen.insert(d.clone()));
    }
}

// ---------------------------------------------------------------------------
// SOPNodeConfig
// ---------------------------------------------------------------------------

#[test]
fn sop_node_config_field_assignment() {
    let c = SOPNodeConfig {
        name: "n".into(),
        kind: NodeKind::Wasm {
            tool_id: "t".into(),
            fuel_limit: 100,
        },
    };
    assert_eq!(c.name, "n");
}

#[test]
fn sop_node_config_serde() {
    let c = SOPNodeConfig {
        name: "n".into(),
        kind: NodeKind::Test {
            command: "ls".into(),
            workdir: Some("/tmp".into()),
        },
    };
    let j = serde_json::to_string(&c).unwrap();
    let back: SOPNodeConfig = serde_json::from_str(&j).unwrap();
    assert_eq!(back.name, "n");
    match back.kind {
        NodeKind::Test { command, workdir } => {
            assert_eq!(command, "ls");
            assert_eq!(workdir.as_deref(), Some("/tmp"));
        }
        _ => panic!("expected Test"),
    }
}
