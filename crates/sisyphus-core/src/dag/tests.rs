use super::*;
use dashmap::DashMap;
use sisyphus_types::{
    CompositeGraph, GraphDependency, DataMapping, NodeOutput, SOPGraphConfig,
};
use std::collections::HashMap;

#[test]
fn test_dependency_resolver_resolve() {
    // Happy path: A -> B
    let mut graphs = HashMap::new();
    graphs.insert(
        "A".to_string(),
        SOPGraphConfig {
            nodes: vec![],
            edges: vec![],
        },
    );
    graphs.insert(
        "B".to_string(),
        SOPGraphConfig {
            nodes: vec![],
            edges: vec![],
        },
    );

    let deps = vec![GraphDependency {
        source_sop_id: "A".to_string(),
        target_sop_id: "B".to_string(),
        mappings: vec![],
    }];

    let composite = CompositeGraph {
        graphs,
        dependencies: deps,
        execution_order: vec![],
    };

    let order = DependencyResolver::resolve(&composite).unwrap();
    assert_eq!(order, vec!["A", "B"]);

    // Happy path: No deps
    let mut graphs_no_deps = HashMap::new();
    graphs_no_deps.insert(
        "A".to_string(),
        SOPGraphConfig {
            nodes: vec![],
            edges: vec![],
        },
    );
    graphs_no_deps.insert(
        "B".to_string(),
        SOPGraphConfig {
            nodes: vec![],
            edges: vec![],
        },
    );
    let composite_no_deps = CompositeGraph {
        graphs: graphs_no_deps,
        dependencies: vec![],
        execution_order: vec![],
    };
    let order_no_deps = DependencyResolver::resolve(&composite_no_deps).unwrap();
    assert!(order_no_deps.contains(&"A".to_string()));
    assert!(order_no_deps.contains(&"B".to_string()));
    assert_eq!(order_no_deps.len(), 2);

    // Cycle detection: A -> B -> A
    let mut graphs_cycle = HashMap::new();
    graphs_cycle.insert(
        "A".to_string(),
        SOPGraphConfig {
            nodes: vec![],
            edges: vec![],
        },
    );
    graphs_cycle.insert(
        "B".to_string(),
        SOPGraphConfig {
            nodes: vec![],
            edges: vec![],
        },
    );
    let deps_cycle = vec![
        GraphDependency {
            source_sop_id: "A".to_string(),
            target_sop_id: "B".to_string(),
            mappings: vec![],
        },
        GraphDependency {
            source_sop_id: "B".to_string(),
            target_sop_id: "A".to_string(),
            mappings: vec![],
        },
    ];
    let composite_cycle = CompositeGraph {
        graphs: graphs_cycle,
        dependencies: deps_cycle,
        execution_order: vec![],
    };
    let res_cycle = DependencyResolver::resolve(&composite_cycle);
    assert!(matches!(res_cycle, Err(CoreError::DependencyCycleDetected)));

    // Missing source node
    let mut graphs_missing = HashMap::new();
    graphs_missing.insert(
        "B".to_string(),
        SOPGraphConfig {
            nodes: vec![],
            edges: vec![],
        },
    );
    let deps_missing = vec![GraphDependency {
        source_sop_id: "A".to_string(),
        target_sop_id: "B".to_string(),
        mappings: vec![],
    }];
    let composite_missing = CompositeGraph {
        graphs: graphs_missing,
        dependencies: deps_missing,
        execution_order: vec![],
    };
    let order_missing = DependencyResolver::resolve(&composite_missing).unwrap();
    assert_eq!(order_missing, vec!["B"]);

    // Empty composite
    let composite_empty = CompositeGraph {
        graphs: HashMap::new(),
        dependencies: vec![],
        execution_order: vec![],
    };
    let order_empty = DependencyResolver::resolve(&composite_empty).unwrap();
    assert!(order_empty.is_empty());
}

#[test]
fn test_build_dep_ctx_for() {
    let mut graphs = HashMap::new();
    graphs.insert(
        "A".to_string(),
        SOPGraphConfig {
            nodes: vec![],
            edges: vec![],
        },
    );
    graphs.insert(
        "B".to_string(),
        SOPGraphConfig {
            nodes: vec![],
            edges: vec![],
        },
    );

    let deps = vec![GraphDependency {
        source_sop_id: "A".to_string(),
        target_sop_id: "B".to_string(),
        mappings: vec![DataMapping {
            source_node: "node_a".to_string(),
            target_placeholder: "placeholder_b".to_string(),
        }],
    }];

    let composite = CompositeGraph {
        graphs,
        dependencies: deps,
        execution_order: vec![],
    };

    let accumulated = DashMap::new();
    accumulated.insert(
        "node_a".to_string(),
        NodeOutput::from(serde_json::Value::String("val_a".into())),
    );

    // Test with deps
    let ctx = build_dep_ctx_for("B", &composite, &accumulated);
    assert!(ctx.is_some());
    let ctx = ctx.unwrap();
    assert_eq!(
        ctx.get_result("placeholder_b").unwrap().to_string(),
        "\"val_a\""
    );

    // Test without deps (wrong target)
    let ctx_none = build_dep_ctx_for("A", &composite, &accumulated);
    assert!(ctx_none.is_none());

    // Test with deps but missing source output
    let accumulated_empty = DashMap::new();
    let ctx_missing = build_dep_ctx_for("B", &composite, &accumulated_empty);
    assert!(ctx_missing.is_none());
}

#[test]
fn test_dependency_resolver_diamond() {
    // A -> B, A -> C  (fan-out)
    let mut graphs = HashMap::new();
    graphs.insert(
        "A".into(),
        SOPGraphConfig {
            nodes: vec![],
            edges: vec![],
        },
    );
    graphs.insert(
        "B".into(),
        SOPGraphConfig {
            nodes: vec![],
            edges: vec![],
        },
    );
    graphs.insert(
        "C".into(),
        SOPGraphConfig {
            nodes: vec![],
            edges: vec![],
        },
    );

    let deps = vec![
        GraphDependency {
            source_sop_id: "A".into(),
            target_sop_id: "B".into(),
            mappings: vec![],
        },
        GraphDependency {
            source_sop_id: "A".into(),
            target_sop_id: "C".into(),
            mappings: vec![],
        },
    ];

    let composite = CompositeGraph {
        graphs,
        dependencies: deps,
        execution_order: vec![],
    };
    let order = DependencyResolver::resolve(&composite).unwrap();

    assert_eq!(order[0], "A");
    assert!(order.contains(&"B".into()));
    assert!(order.contains(&"C".into()));
    assert_eq!(order.len(), 3);
}

#[test]
fn test_dependency_resolver_chain() {
    // A -> B -> C
    let mut graphs = HashMap::new();
    graphs.insert(
        "A".into(),
        SOPGraphConfig {
            nodes: vec![],
            edges: vec![],
        },
    );
    graphs.insert(
        "B".into(),
        SOPGraphConfig {
            nodes: vec![],
            edges: vec![],
        },
    );
    graphs.insert(
        "C".into(),
        SOPGraphConfig {
            nodes: vec![],
            edges: vec![],
        },
    );

    let deps = vec![
        GraphDependency {
            source_sop_id: "A".into(),
            target_sop_id: "B".into(),
            mappings: vec![],
        },
        GraphDependency {
            source_sop_id: "B".into(),
            target_sop_id: "C".into(),
            mappings: vec![],
        },
    ];

    let composite = CompositeGraph {
        graphs,
        dependencies: deps,
        execution_order: vec![],
    };
    let order = DependencyResolver::resolve(&composite).unwrap();
    assert_eq!(order, vec!["A", "B", "C"]);
}

#[test]
fn test_build_dep_ctx_for_multiple_sources() {
    // SOP C depends on both A and B with different mappings
    let mut graphs = HashMap::new();
    graphs.insert(
        "A".into(),
        SOPGraphConfig {
            nodes: vec![],
            edges: vec![],
        },
    );
    graphs.insert(
        "B".into(),
        SOPGraphConfig {
            nodes: vec![],
            edges: vec![],
        },
    );
    graphs.insert(
        "C".into(),
        SOPGraphConfig {
            nodes: vec![],
            edges: vec![],
        },
    );

    let deps = vec![
        GraphDependency {
            source_sop_id: "A".into(),
            target_sop_id: "C".into(),
            mappings: vec![DataMapping {
                source_node: "a_node".into(),
                target_placeholder: "c_from_a".into(),
            }],
        },
        GraphDependency {
            source_sop_id: "B".into(),
            target_sop_id: "C".into(),
            mappings: vec![DataMapping {
                source_node: "b_node".into(),
                target_placeholder: "c_from_b".into(),
            }],
        },
    ];

    let composite = CompositeGraph {
        graphs,
        dependencies: deps,
        execution_order: vec![],
    };

    let accumulated = DashMap::new();
    accumulated.insert(
        "a_node".into(),
        NodeOutput::from(serde_json::Value::String("val_a".into())),
    );
    accumulated.insert(
        "b_node".into(),
        NodeOutput::from(serde_json::Value::String("val_b".into())),
    );

    let ctx = build_dep_ctx_for("C", &composite, &accumulated).unwrap();
    assert_eq!(ctx.get_result("c_from_a").unwrap().to_string(), "\"val_a\"");
    assert_eq!(ctx.get_result("c_from_b").unwrap().to_string(), "\"val_b\"");
}

#[test]
fn test_build_dep_ctx_for_single_source_multi_mapping() {
    // A -> B with two DataMapping entries from different source nodes
    let mut graphs = HashMap::new();
    graphs.insert(
        "A".into(),
        SOPGraphConfig {
            nodes: vec![],
            edges: vec![],
        },
    );
    graphs.insert(
        "B".into(),
        SOPGraphConfig {
            nodes: vec![],
            edges: vec![],
        },
    );

    let deps = vec![GraphDependency {
        source_sop_id: "A".into(),
        target_sop_id: "B".into(),
        mappings: vec![
            DataMapping {
                source_node: "n1".into(),
                target_placeholder: "p1".into(),
            },
            DataMapping {
                source_node: "n2".into(),
                target_placeholder: "p2".into(),
            },
        ],
    }];

    let composite = CompositeGraph {
        graphs,
        dependencies: deps,
        execution_order: vec![],
    };

    let accumulated = DashMap::new();
    accumulated.insert(
        "n1".into(),
        NodeOutput::from(serde_json::Value::String("v1".into())),
    );
    accumulated.insert(
        "n2".into(),
        NodeOutput::from(serde_json::Value::String("v2".into())),
    );

    let ctx = build_dep_ctx_for("B", &composite, &accumulated).unwrap();
    assert_eq!(ctx.get_result("p1").unwrap().to_string(), "\"v1\"");
    assert_eq!(ctx.get_result("p2").unwrap().to_string(), "\"v2\"");
}
