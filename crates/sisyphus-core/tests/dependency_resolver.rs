//! Integration tests for sisyphus-core `dag::DependencyResolver`.
//!
//! Exercises the public [`DependencyResolver::resolve`] function —
//! takes a [`CompositeGraph`] and returns a topologically ordered
//! vector of SOP IDs. The function builds a petgraph DiGraph from
//! the composite's graphs and dependencies, then performs a
//! topological sort to derive the execution order.

use sisyphus_core::dag::DependencyResolver;
use sisyphus_types::graph::{
    CompositeGraph, DataMapping, GraphDependency, SOPGraphConfig, SOPNodeConfig,
};
use sisyphus_types::NodeKind;

fn make_cfg(name: &str) -> SOPGraphConfig {
    SOPGraphConfig {
        nodes: vec![SOPNodeConfig {
            name: name.into(),
            kind: NodeKind::Test {
                command: "true".into(),
                workdir: None,
            },
        }],
        edges: vec![],
    }
}

fn make_dep(src: &str, tgt: &str) -> GraphDependency {
    GraphDependency {
        source_sop_id: src.into(),
        target_sop_id: tgt.into(),
        mappings: vec![],
    }
}

fn make_mapping(src_node: &str, tgt_placeholder: &str) -> DataMapping {
    DataMapping {
        source_node: src_node.into(),
        target_placeholder: tgt_placeholder.into(),
    }
}

// ---------------------------------------------------------------------------
// Empty / single
// ---------------------------------------------------------------------------

#[test]
fn resolve_empty_composite_returns_empty_order() {
    let c = CompositeGraph::new();
    let order = DependencyResolver::resolve(&c).expect("ok");
    assert!(order.is_empty());
}

#[test]
fn resolve_single_sop_with_no_deps() {
    let mut c = CompositeGraph::new();
    c.add_graph("solo", make_cfg("solo"));
    let order = DependencyResolver::resolve(&c).expect("ok");
    assert_eq!(order, vec!["solo".to_string()]);
}

#[test]
fn resolve_multiple_independent_sops() {
    let mut c = CompositeGraph::new();
    c.add_graph("a", make_cfg("a"));
    c.add_graph("b", make_cfg("b"));
    c.add_graph("c", make_cfg("c"));
    let order = DependencyResolver::resolve(&c).expect("ok");
    // All 3 SOPs are present, no specific order required.
    assert_eq!(order.len(), 3);
    assert!(order.contains(&"a".to_string()));
    assert!(order.contains(&"b".to_string()));
    assert!(order.contains(&"c".to_string()));
}

// ---------------------------------------------------------------------------
// Linear chain
// ---------------------------------------------------------------------------

#[test]
fn resolve_linear_chain() {
    let mut c = CompositeGraph::new();
    c.add_graph("a", make_cfg("a"));
    c.add_graph("b", make_cfg("b"));
    c.add_graph("c", make_cfg("c"));
    c.add_dependency(make_dep("a", "b"));
    c.add_dependency(make_dep("b", "c"));
    let order = DependencyResolver::resolve(&c).expect("ok");
    // a must come before b, b before c.
    let pos_a = order.iter().position(|x| x == "a").expect("a");
    let pos_b = order.iter().position(|x| x == "b").expect("b");
    let pos_c = order.iter().position(|x| x == "c").expect("c");
    assert!(pos_a < pos_b);
    assert!(pos_b < pos_c);
}

#[test]
fn resolve_long_linear_chain() {
    let mut c = CompositeGraph::new();
    for ch in "abcdefgh".chars() {
        c.add_graph(&ch.to_string(), make_cfg(&ch.to_string()));
    }
    for i in 0..7 {
        let a = ((b'a' + i) as char).to_string();
        let b = ((b'a' + i + 1) as char).to_string();
        c.add_dependency(make_dep(&a, &b));
    }
    let order = DependencyResolver::resolve(&c).expect("ok");
    assert_eq!(order.len(), 8);
    // First should be 'a', last should be 'h'.
    assert_eq!(order[0], "a");
    assert_eq!(order[7], "h");
}

// ---------------------------------------------------------------------------
// Diamond
// ---------------------------------------------------------------------------

#[test]
fn resolve_diamond() {
    let mut c = CompositeGraph::new();
    c.add_graph("root", make_cfg("root"));
    c.add_graph("left", make_cfg("left"));
    c.add_graph("right", make_cfg("right"));
    c.add_graph("merge", make_cfg("merge"));
    c.add_dependency(make_dep("root", "left"));
    c.add_dependency(make_dep("root", "right"));
    c.add_dependency(make_dep("left", "merge"));
    c.add_dependency(make_dep("right", "merge"));
    let order = DependencyResolver::resolve(&c).expect("ok");
    assert_eq!(order.len(), 4);
    let pos_root = order.iter().position(|x| x == "root").expect("");
    let pos_merge = order.iter().position(|x| x == "merge").expect("");
    assert!(pos_root < pos_merge);
}

// ---------------------------------------------------------------------------
// Wide fanout
// ---------------------------------------------------------------------------

#[test]
fn resolve_wide_fanout() {
    let mut c = CompositeGraph::new();
    c.add_graph("root", make_cfg("root"));
    for i in 0..10 {
        c.add_graph(&format!("leaf_{i}"), make_cfg(&format!("leaf_{i}")));
        c.add_dependency(make_dep("root", &format!("leaf_{i}")));
    }
    let order = DependencyResolver::resolve(&c).expect("ok");
    assert_eq!(order.len(), 11);
    // Root must be first.
    assert_eq!(order[0], "root");
}

// ---------------------------------------------------------------------------
// Error cases
// ---------------------------------------------------------------------------

#[test]
fn resolve_cycle_returns_error() {
    let mut c = CompositeGraph::new();
    c.add_graph("a", make_cfg("a"));
    c.add_graph("b", make_cfg("b"));
    c.add_dependency(make_dep("a", "b"));
    c.add_dependency(make_dep("b", "a"));
    let result = DependencyResolver::resolve(&c);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// With mappings (mappings don't affect order)
// ---------------------------------------------------------------------------

#[test]
fn resolve_with_mappings() {
    let mut c = CompositeGraph::new();
    c.add_graph("src", make_cfg("src"));
    c.add_graph("dst", make_cfg("dst"));
    let mut dep = make_dep("src", "dst");
    dep.mappings = vec![make_mapping("out1", "{x}"), make_mapping("out2", "{y}")];
    c.add_dependency(dep);
    let order = DependencyResolver::resolve(&c).expect("ok");
    assert_eq!(order, vec!["src".to_string(), "dst".to_string()]);
}

// ---------------------------------------------------------------------------
// resolve consistency
// ---------------------------------------------------------------------------

#[test]
fn resolve_is_deterministic_for_same_input() {
    let mut c = CompositeGraph::new();
    c.add_graph("a", make_cfg("a"));
    c.add_graph("b", make_cfg("b"));
    c.add_graph("c", make_cfg("c"));
    c.add_dependency(make_dep("a", "b"));
    c.add_dependency(make_dep("b", "c"));
    let order1 = DependencyResolver::resolve(&c).expect("");
    let order2 = DependencyResolver::resolve(&c).expect("");
    assert_eq!(order1, order2);
}

// ---------------------------------------------------------------------------
// Additional tests: empty/single/chain/diamond/mappings
// ---------------------------------------------------------------------------

#[test]
fn dep_resolver_resolve_empty_graph() {
    let cg = CompositeGraph::new();
    let order = DependencyResolver::resolve(&cg).unwrap();
    assert!(order.is_empty());
}

#[test]
fn dep_resolver_resolve_single_graph_no_deps() {
    let mut cg = CompositeGraph::new();
    cg.add_graph("a", SOPGraphConfig::default());
    let order = DependencyResolver::resolve(&cg).unwrap();
    assert_eq!(order.len(), 1);
    assert!(order.contains(&"a".to_string()));
}

#[test]
fn dep_resolver_resolve_two_graphs_no_deps() {
    let mut cg = CompositeGraph::new();
    cg.add_graph("a", SOPGraphConfig::default());
    cg.add_graph("b", SOPGraphConfig::default());
    let order = DependencyResolver::resolve(&cg).unwrap();
    assert_eq!(order.len(), 2);
    assert!(order.contains(&"a".to_string()));
    assert!(order.contains(&"b".to_string()));
}

#[test]
fn dep_resolver_resolve_three_graphs_no_deps() {
    let mut cg = CompositeGraph::new();
    for id in ["x", "y", "z"] {
        cg.add_graph(id, SOPGraphConfig::default());
    }
    let order = DependencyResolver::resolve(&cg).unwrap();
    assert_eq!(order.len(), 3);
}

#[test]
fn dep_resolver_resolve_with_chain_deps() {
    let mut cg = CompositeGraph::new();
    cg.add_graph("a", SOPGraphConfig::default());
    cg.add_graph("b", SOPGraphConfig::default());
    cg.add_graph("c", SOPGraphConfig::default());
    cg.add_dependency(GraphDependency {
        source_sop_id: "a".into(),
        target_sop_id: "b".into(),
        mappings: vec![],
    });
    cg.add_dependency(GraphDependency {
        source_sop_id: "b".into(),
        target_sop_id: "c".into(),
        mappings: vec![],
    });
    let order = DependencyResolver::resolve(&cg).unwrap();
    assert_eq!(order.len(), 3);
    let pos_a = order.iter().position(|s| s == "a").unwrap();
    let pos_b = order.iter().position(|s| s == "b").unwrap();
    let pos_c = order.iter().position(|s| s == "c").unwrap();
    assert!(pos_a < pos_b);
    assert!(pos_b < pos_c);
}

#[test]
fn dep_resolver_resolve_with_diamond_deps() {
    let mut cg = CompositeGraph::new();
    for id in ["top", "left", "right", "bottom"] {
        cg.add_graph(id, SOPGraphConfig::default());
    }
    cg.add_dependency(GraphDependency {
        source_sop_id: "top".into(),
        target_sop_id: "left".into(),
        mappings: vec![],
    });
    cg.add_dependency(GraphDependency {
        source_sop_id: "top".into(),
        target_sop_id: "right".into(),
        mappings: vec![],
    });
    cg.add_dependency(GraphDependency {
        source_sop_id: "left".into(),
        target_sop_id: "bottom".into(),
        mappings: vec![],
    });
    cg.add_dependency(GraphDependency {
        source_sop_id: "right".into(),
        target_sop_id: "bottom".into(),
        mappings: vec![],
    });
    let order = DependencyResolver::resolve(&cg).unwrap();
    assert_eq!(order.len(), 4);
    let pos = |id: &str| order.iter().position(|s| s == id).unwrap();
    assert!(pos("top") < pos("left"));
    assert!(pos("top") < pos("right"));
    assert!(pos("left") < pos("bottom"));
    assert!(pos("right") < pos("bottom"));
}

#[test]
fn dep_resolver_resolve_with_data_mapping() {
    let mut cg = CompositeGraph::new();
    cg.add_graph("a", SOPGraphConfig::default());
    cg.add_graph("b", SOPGraphConfig::default());
    cg.add_dependency(GraphDependency {
        source_sop_id: "a".into(),
        target_sop_id: "b".into(),
        mappings: vec![DataMapping {
            source_node: "n1".into(),
            target_placeholder: "{n1}".into(),
        }],
    });
    let order = DependencyResolver::resolve(&cg).unwrap();
    assert_eq!(order.len(), 2);
    let pos_a = order.iter().position(|s| s == "a").unwrap();
    let pos_b = order.iter().position(|s| s == "b").unwrap();
    assert!(pos_a < pos_b);
}
