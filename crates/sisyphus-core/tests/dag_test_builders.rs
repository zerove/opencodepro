//! Integration tests for sisyphus-core `chaos_testing::dag_test`.
//!
//! Exercises the public DAG builders used by the chaos testing
//! framework: `single_node`, `linear_chain`, `diamond`, and
//! `wide_fanout`. These produce `SOPGraphConfig` instances that
//! downstream tests can execute end-to-end.

use sisyphus_core::chaos_testing::dag_test::{diamond, linear_chain, single_node, wide_fanout};

// ---------------------------------------------------------------------------
// single_node
// ---------------------------------------------------------------------------

#[test]
fn single_node_creates_one_node_graph() {
    let cfg = single_node("solo", "tool_x").expect("ok");
    assert_eq!(cfg.nodes.len(), 1);
    assert_eq!(cfg.nodes[0].name, "solo");
    assert!(cfg.edges.is_empty());
}

#[test]
fn single_node_with_different_names() {
    let cfg_a = single_node("alpha", "tool_a").expect("ok");
    let cfg_b = single_node("beta", "tool_b").expect("ok");
    assert_eq!(cfg_a.nodes[0].name, "alpha");
    assert_eq!(cfg_b.nodes[0].name, "beta");
}

// ---------------------------------------------------------------------------
// linear_chain
// ---------------------------------------------------------------------------

#[test]
fn linear_chain_creates_n_nodes() {
    for n in [1, 2, 3, 5, 10] {
        let cfg = linear_chain(n).expect("ok");
        assert_eq!(cfg.nodes.len(), n, "n={n}");
        // Chain has n-1 edges.
        assert_eq!(cfg.edges.len(), n - 1, "n={n}");
    }
}

#[test]
fn linear_chain_edges_form_proper_sequence() {
    let cfg = linear_chain(4).expect("ok");
    // Edges should connect node i to i+1: (0,1), (1,2), (2,3).
    assert_eq!(cfg.edges, vec![(0, 1), (1, 2), (2, 3)]);
}

#[test]
fn linear_chain_node_names_are_indexed() {
    let cfg = linear_chain(3).expect("ok");
    assert_eq!(cfg.nodes[0].name, "node_0");
    assert_eq!(cfg.nodes[1].name, "node_1");
    assert_eq!(cfg.nodes[2].name, "node_2");
}

// ---------------------------------------------------------------------------
// diamond
// ---------------------------------------------------------------------------

#[test]
fn diamond_creates_four_nodes() {
    let cfg = diamond().expect("ok");
    assert_eq!(cfg.nodes.len(), 4);
    // Diamond has 4 edges: root→left, root→right, left→merge, right→merge.
    assert_eq!(cfg.edges.len(), 4);
}

#[test]
fn diamond_has_expected_node_names() {
    let cfg = diamond().expect("ok");
    let names: Vec<&str> = cfg.nodes.iter().map(|n| n.name.as_str()).collect();
    assert!(names.contains(&"root"));
    assert!(names.contains(&"left"));
    assert!(names.contains(&"right"));
    assert!(names.contains(&"merge"));
}

#[test]
fn diamond_edges_form_proper_structure() {
    let cfg = diamond().expect("ok");
    // Verify all expected edges are present.
    let edge_set: std::collections::HashSet<(usize, usize)> = cfg.edges.iter().copied().collect();
    // root→left, root→right, left→merge, right→merge
    let root_idx = cfg.nodes.iter().position(|n| n.name == "root").expect("");
    let left_idx = cfg.nodes.iter().position(|n| n.name == "left").expect("");
    let right_idx = cfg.nodes.iter().position(|n| n.name == "right").expect("");
    let merge_idx = cfg.nodes.iter().position(|n| n.name == "merge").expect("");
    assert!(edge_set.contains(&(root_idx, left_idx)));
    assert!(edge_set.contains(&(root_idx, right_idx)));
    assert!(edge_set.contains(&(left_idx, merge_idx)));
    assert!(edge_set.contains(&(right_idx, merge_idx)));
}

// ---------------------------------------------------------------------------
// wide_fanout
// ---------------------------------------------------------------------------

#[test]
fn wide_fanout_creates_n_plus_one_nodes() {
    for n in [1, 5, 10] {
        let cfg = wide_fanout(n).expect("ok");
        assert_eq!(cfg.nodes.len(), n + 1, "n={n}");
        // Each leaf is connected from root → n edges total.
        assert_eq!(cfg.edges.len(), n, "n={n}");
    }
}

#[test]
fn wide_fanout_creates_root_and_leaves() {
    let cfg = wide_fanout(3).expect("ok");
    let names: Vec<&str> = cfg.nodes.iter().map(|n| n.name.as_str()).collect();
    assert!(names.contains(&"root"));
    assert!(names.contains(&"leaf_0"));
    assert!(names.contains(&"leaf_1"));
    assert!(names.contains(&"leaf_2"));
}

#[test]
fn wide_fanout_root_connects_to_all_leaves() {
    let cfg = wide_fanout(5).expect("ok");
    let root_idx = cfg.nodes.iter().position(|n| n.name == "root").expect("");
    let edge_set: std::collections::HashSet<(usize, usize)> = cfg.edges.iter().copied().collect();
    for i in 0..5 {
        let leaf_idx = cfg.nodes.iter().position(|n| n.name == format!("leaf_{i}")).expect("");
        assert!(edge_set.contains(&(root_idx, leaf_idx)));
    }
}

// ---------------------------------------------------------------------------
// Cross-builder consistency
// ---------------------------------------------------------------------------

#[test]
fn all_builders_produce_valid_configs() {
    let _ = single_node("x", "y").expect("ok");
    let _ = linear_chain(2).expect("ok");
    let _ = diamond().expect("ok");
    let _ = wide_fanout(2).expect("ok");
}
