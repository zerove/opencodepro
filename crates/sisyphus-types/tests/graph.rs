//! Integration tests for sisyphus-types `graph` types.
//!
//! Exercises the public [`CompositeGraph`] / [`SOPGraphConfig`] /
//! [`SOPNodeConfig`] / [`NodeKind`] / [`SOPGraph`] APIs. These are
//! the data structures that describe a workflow before it is
//! executed: nodes, edges, kinds, and conversion into a
//! [`CognitivePlan`].

use sisyphus_types::execution::CognitiveAction;
use sisyphus_types::graph::{
    CompositeGraph, DataMapping, GraphDependency, NodeKind, SOPGraphConfig, SOPNodeConfig,
};

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
fn composite_graph_add_graph_inserts() {
    let mut g = CompositeGraph::new();
    let cfg = SOPGraphConfig::default();
    g.add_graph("sop_a", cfg);
    assert!(g.graphs.contains_key("sop_a"));
    assert_eq!(g.graphs.len(), 1);
}

#[test]
fn composite_graph_add_graph_replaces_existing() {
    let mut g = CompositeGraph::new();
    g.add_graph("sop", SOPGraphConfig::default());
    g.add_graph(
        "sop",
        SOPGraphConfig {
            nodes: vec![SOPNodeConfig {
                name: "x".into(),
                kind: NodeKind::Test {
                    command: "echo".into(),
                    workdir: None,
                },
            }],
            edges: vec![],
        },
    );
    let cfg = g.graphs.get("sop").expect("present");
    assert_eq!(cfg.nodes.len(), 1);
    assert_eq!(cfg.nodes[0].name, "x");
}

#[test]
fn composite_graph_add_dependency_appends() {
    let mut g = CompositeGraph::new();
    g.add_dependency(GraphDependency {
        source_sop_id: "a".into(),
        target_sop_id: "b".into(),
        mappings: vec![],
    });
    assert_eq!(g.dependencies.len(), 1);
    assert_eq!(g.dependencies[0].source_sop_id, "a");
    assert_eq!(g.dependencies[0].target_sop_id, "b");
}

#[test]
fn composite_graph_default_matches_new() {
    let g = CompositeGraph::default();
    assert!(g.graphs.is_empty());
    assert!(g.dependencies.is_empty());
}

// ---------------------------------------------------------------------------
// GraphDependency
// ---------------------------------------------------------------------------

#[test]
fn graph_dependency_with_mappings() {
    let dep = GraphDependency {
        source_sop_id: "src".into(),
        target_sop_id: "dst".into(),
        mappings: vec![
            DataMapping {
                source_node: "out_a".into(),
                target_placeholder: "{x}".into(),
            },
            DataMapping {
                source_node: "out_b".into(),
                target_placeholder: "{y}".into(),
            },
        ],
    };
    assert_eq!(dep.mappings.len(), 2);
    assert_eq!(dep.mappings[0].source_node, "out_a");
    assert_eq!(dep.mappings[0].target_placeholder, "{x}");
}

// ---------------------------------------------------------------------------
// NodeKind variants
// ---------------------------------------------------------------------------

#[test]
fn node_kind_wasm_carries_tool_id() {
    let kind = NodeKind::Wasm {
        tool_id: "tool-1".into(),
        fuel_limit: 1000,
    };
    match kind {
        NodeKind::Wasm { tool_id, fuel_limit } => {
            assert_eq!(tool_id, "tool-1");
            assert_eq!(fuel_limit, 1000);
        }
        _ => panic!("expected Wasm"),
    }
}

#[test]
fn node_kind_retry_with_max_retries() {
    let kind = NodeKind::Retry {
        child: Box::new(NodeKind::Test {
            command: "ls".into(),
            workdir: None,
        }),
        max_retries: 5,
    };
    match kind {
        NodeKind::Retry { max_retries, .. } => assert_eq!(max_retries, 5),
        _ => panic!("expected Retry"),
    }
}

#[test]
fn node_kind_branch_with_branches() {
    let mut branches = std::collections::HashMap::new();
    branches.insert("yes".to_string(), 1);
    branches.insert("no".to_string(), 2);
    let kind = NodeKind::Branch {
        branches,
        default_next: 3,
    };
    match kind {
        NodeKind::Branch {
            branches,
            default_next,
        } => {
            assert_eq!(branches.len(), 2);
            assert_eq!(default_next, 3);
        }
        _ => panic!("expected Branch"),
    }
}

#[test]
fn node_kind_human_intervention_carries_timeout() {
    let kind = NodeKind::HumanIntervention {
        prompt: "approve?".into(),
        timeout_secs: 60,
    };
    match kind {
        NodeKind::HumanIntervention { timeout_secs, .. } => assert_eq!(timeout_secs, 60),
        _ => panic!("expected HumanIntervention"),
    }
}

// ---------------------------------------------------------------------------
// SOPNodeConfig / SOPGraphConfig
// ---------------------------------------------------------------------------

#[test]
fn sop_node_config_stores_name_and_kind() {
    let node = SOPNodeConfig {
        name: "step1".into(),
        kind: NodeKind::Wasm {
            tool_id: "t".into(),
            fuel_limit: 100,
        },
    };
    assert_eq!(node.name, "step1");
}

#[test]
fn sop_graph_config_default_is_empty() {
    let g = SOPGraphConfig::default();
    assert!(g.nodes.is_empty());
    assert!(g.edges.is_empty());
}

#[test]
fn sop_graph_config_serializes_to_json() {
    let g = SOPGraphConfig {
        nodes: vec![SOPNodeConfig {
            name: "n1".into(),
            kind: NodeKind::Test {
                command: "true".into(),
                workdir: None,
            },
        }],
        edges: vec![(0, 0)],
    };
    let json = serde_json::to_string(&g).expect("serialize");
    assert!(json.contains("n1"));
    assert!(json.contains("true"));
}

// ---------------------------------------------------------------------------
// into_cognitive_plan
// ---------------------------------------------------------------------------

#[test]
fn into_cognitive_plan_with_no_nodes() {
    let g = SOPGraphConfig::default();
    let plan = g.into_cognitive_plan("plan1".to_string(), "goal".to_string());
    assert_eq!(plan.plan_id, "plan1");
    assert_eq!(plan.goal, "goal");
    assert!(plan.steps.is_empty());
    assert!(plan.edges.is_empty());
    assert_eq!(plan.start_node_id, "0");
}

#[test]
fn into_cognitive_plan_maps_wasm_to_tool_call() {
    let g = SOPGraphConfig {
        nodes: vec![SOPNodeConfig {
            name: "step_a".into(),
            kind: NodeKind::Wasm {
                tool_id: "wasm_tool".into(),
                fuel_limit: 100,
            },
        }],
        edges: vec![],
    };
    let plan = g.into_cognitive_plan("p".into(), "g".into());
    assert_eq!(plan.steps.len(), 1);
    assert_eq!(plan.steps[0].name, "step_a");
    match &plan.steps[0].action {
        CognitiveAction::ToolCall { tool_id, .. } => {
            assert_eq!(tool_id, "wasm_tool");
        }
        CognitiveAction::FileEdit { .. } | CognitiveAction::ShellExecute { .. } => {
            panic!("expected ToolCall")
        }
    }
}

#[test]
fn into_cognitive_plan_maps_retry_to_retry_wrapper() {
    let g = SOPGraphConfig {
        nodes: vec![SOPNodeConfig {
            name: "step".into(),
            kind: NodeKind::Retry {
                child: Box::new(NodeKind::Test {
                    command: "ls".into(),
                    workdir: None,
                }),
                max_retries: 3,
            },
        }],
        edges: vec![],
    };
    let plan = g.into_cognitive_plan("p".into(), "g".into());
    match &plan.steps[0].action {
        CognitiveAction::ToolCall { tool_id, .. } => {
            assert_eq!(tool_id, "retry_wrapper");
        }
        CognitiveAction::FileEdit { .. } | CognitiveAction::ShellExecute { .. } => {
            panic!("expected ToolCall")
        }
    }
}

#[test]
fn into_cognitive_plan_maps_branch_to_branch_logic() {
    let g = SOPGraphConfig {
        nodes: vec![SOPNodeConfig {
            name: "b".into(),
            kind: NodeKind::Branch {
                branches: Default::default(),
                default_next: 0,
            },
        }],
        edges: vec![],
    };
    let plan = g.into_cognitive_plan("p".into(), "g".into());
    match &plan.steps[0].action {
        CognitiveAction::ToolCall { tool_id, .. } => {
            assert_eq!(tool_id, "branch_logic");
        }
        CognitiveAction::FileEdit { .. } | CognitiveAction::ShellExecute { .. } => {
            panic!("expected ToolCall")
        }
    }
}

#[test]
fn into_cognitive_plan_maps_visual_to_internal_handler() {
    let g = SOPGraphConfig {
        nodes: vec![SOPNodeConfig {
            name: "v".into(),
            kind: NodeKind::VisualAnalysis {
                prompt: "describe".into(),
                image_source: "screen".into(),
            },
        }],
        edges: vec![],
    };
    let plan = g.into_cognitive_plan("p".into(), "g".into());
    match &plan.steps[0].action {
        CognitiveAction::ToolCall { tool_id, .. } => {
            assert_eq!(tool_id, "internal_handler");
        }
        CognitiveAction::FileEdit { .. } | CognitiveAction::ShellExecute { .. } => {
            panic!("expected ToolCall")
        }
    }
}

#[test]
fn into_cognitive_plan_preserves_edges() {
    let g = SOPGraphConfig {
        nodes: vec![
            SOPNodeConfig {
                name: "a".into(),
                kind: NodeKind::Test {
                    command: "echo a".into(),
                    workdir: None,
                },
            },
            SOPNodeConfig {
                name: "b".into(),
                kind: NodeKind::Test {
                    command: "echo b".into(),
                    workdir: None,
                },
            },
        ],
        edges: vec![(0, 1)],
    };
    let plan = g.into_cognitive_plan("p".into(), "g".into());
    assert_eq!(plan.steps.len(), 2);
    assert_eq!(plan.edges.len(), 1);
    assert_eq!(plan.edges[0].from, "0");
    assert_eq!(plan.edges[0].to, "1");
}
