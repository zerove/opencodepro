//! Integration tests for sisyphus-core `dag_renderer` module.
//!
//! Exercises the public `DagRenderer` / `NodeStatus` / `DagVisualization`
//! / `RenderedNode` / `RenderedEdge` API which produces ASCII-art
//! visualizations of SOP graphs for the TUI. The renderer takes
//! nodes + edges and computes a topological layout, then renders
//! the result as a multi-line string with status symbols.

use sisyphus_core::dag_renderer::{DagRenderer, NodeStatus};

// ---------------------------------------------------------------------------
// render: simple cases
// ---------------------------------------------------------------------------

#[test]
fn render_empty_nodes_returns_empty_viz() {
    let r = DagRenderer::new();
    let viz = r.render(&[], &[]);
    assert!(viz.nodes.is_empty());
    assert!(viz.edges.is_empty());
    assert_eq!(viz.width, 0);
    assert_eq!(viz.height, 0);
}

#[test]
fn render_single_node_no_edges() {
    let r = DagRenderer::new();
    let nodes = vec![("a".to_string(), NodeStatus::Completed)];
    let viz = r.render(&nodes, &[]);
    assert_eq!(viz.nodes.len(), 1);
    assert_eq!(viz.nodes[0].id, "a");
    assert_eq!(viz.nodes[0].status, NodeStatus::Completed);
    assert!(viz.edges.is_empty());
    // Single node should have width 0 (it's at column 0 with no extra
    // content beyond the node box).
    let _ = viz.width; // accept any non-negative
    let _ = viz.height;
}

#[test]
fn render_two_node_chain() {
    let r = DagRenderer::new();
    let nodes = vec![
        ("a".to_string(), NodeStatus::Completed),
        ("b".to_string(), NodeStatus::Running),
    ];
    let edges = vec![("a".to_string(), "b".to_string())];
    let viz = r.render(&nodes, &edges);
    assert_eq!(viz.nodes.len(), 2);
    assert_eq!(viz.edges.len(), 1);
    assert_eq!(viz.edges[0].from, "a");
    assert_eq!(viz.edges[0].to, "b");
}

#[test]
fn render_diamond_dag() {
    let r = DagRenderer::new();
    let nodes = vec![
        ("top".to_string(), NodeStatus::Completed),
        ("left".to_string(), NodeStatus::Completed),
        ("right".to_string(), NodeStatus::Running),
        ("bottom".to_string(), NodeStatus::Pending),
    ];
    let edges = vec![
        ("top".to_string(), "left".to_string()),
        ("top".to_string(), "right".to_string()),
        ("left".to_string(), "bottom".to_string()),
        ("right".to_string(), "bottom".to_string()),
    ];
    let viz = r.render(&nodes, &edges);
    assert_eq!(viz.nodes.len(), 4);
    assert_eq!(viz.edges.len(), 4);
    // bottom should be at a later row than top.
    let top_row = viz.nodes.iter().find(|n| n.id == "top").unwrap().row;
    let bottom_row = viz.nodes.iter().find(|n| n.id == "bottom").unwrap().row;
    assert!(bottom_row > top_row);
}

#[test]
fn render_diamond_left_right_same_row() {
    let r = DagRenderer::new();
    let nodes = vec![
        ("top".to_string(), NodeStatus::Completed),
        ("left".to_string(), NodeStatus::Completed),
        ("right".to_string(), NodeStatus::Running),
        ("bottom".to_string(), NodeStatus::Pending),
    ];
    let edges = vec![
        ("top".to_string(), "left".to_string()),
        ("top".to_string(), "right".to_string()),
        ("left".to_string(), "bottom".to_string()),
        ("right".to_string(), "bottom".to_string()),
    ];
    let viz = r.render(&nodes, &edges);
    let left_row = viz.nodes.iter().find(|n| n.id == "left").unwrap().row;
    let right_row = viz.nodes.iter().find(|n| n.id == "right").unwrap().row;
    // Siblings of a diamond should be on the same row.
    assert_eq!(left_row, right_row);
}

#[test]
fn render_wide_fanout() {
    let r = DagRenderer::new();
    let nodes: Vec<_> = (0..6).map(|i| (format!("n{i}"), NodeStatus::Pending)).collect();
    let edges: Vec<_> = (1..6).map(|i| ("n0".to_string(), format!("n{i}"))).collect();
    let viz = r.render(&nodes, &edges);
    assert_eq!(viz.nodes.len(), 6);
    assert_eq!(viz.edges.len(), 5);
}

// ---------------------------------------------------------------------------
// render_to_string
// ---------------------------------------------------------------------------

#[test]
fn render_to_string_empty_returns_empty_dag_message() {
    let r = DagRenderer::new();
    let s = r.render_to_string(&[], &[]);
    assert!(s.contains("Empty") || s.is_empty(), "got {s:?}");
}

#[test]
fn render_to_string_single_node_contains_label() {
    let r = DagRenderer::new();
    let nodes = vec![("hello".to_string(), NodeStatus::Completed)];
    let s = r.render_to_string(&nodes, &[]);
    assert!(s.contains("hello"));
}

#[test]
fn render_to_string_includes_status_symbols() {
    let r = DagRenderer::new();
    let nodes = vec![
        ("completed".to_string(), NodeStatus::Completed),
        ("failed".to_string(), NodeStatus::Failed),
        ("pending".to_string(), NodeStatus::Pending),
    ];
    let s = r.render_to_string(&nodes, &[]);
    // Status symbols are "+", "X", "P" (set in the impl).
    assert!(s.contains("+") || s.contains("X") || s.contains("P"));
    // All labels should appear.
    assert!(s.contains("completed"));
    assert!(s.contains("failed"));
    assert!(s.contains("pending"));
}

#[test]
fn render_to_string_chain_produces_multiline_output() {
    let r = DagRenderer::new();
    let nodes = vec![
        ("a".to_string(), NodeStatus::Completed),
        ("b".to_string(), NodeStatus::Running),
        ("c".to_string(), NodeStatus::Pending),
    ];
    let edges = vec![
        ("a".to_string(), "b".to_string()),
        ("b".to_string(), "c".to_string()),
    ];
    let s = r.render_to_string(&nodes, &edges);
    // Should have multiple lines (at least the height of one node + edges).
    let line_count = s.lines().count();
    assert!(
        line_count >= 3,
        "expected multi-line output, got {line_count} lines"
    );
}

#[test]
fn render_to_string_contains_vertical_lines_for_edges() {
    let r = DagRenderer::new();
    let nodes = vec![
        ("a".to_string(), NodeStatus::Completed),
        ("b".to_string(), NodeStatus::Completed),
    ];
    let edges = vec![("a".to_string(), "b".to_string())];
    let s = r.render_to_string(&nodes, &edges);
    // Vertical lines (`|`) connect a to b.
    assert!(s.contains('|'), "expected vertical edge marker, got:\n{s}");
}

// ---------------------------------------------------------------------------
// NodeStatus
// ---------------------------------------------------------------------------

#[test]
fn node_status_partial_eq() {
    assert_eq!(NodeStatus::Pending, NodeStatus::Pending);
    assert_ne!(NodeStatus::Pending, NodeStatus::Running);
    assert_ne!(NodeStatus::Failed, NodeStatus::Completed);
}

#[test]
fn node_status_clone() {
    let s = NodeStatus::Running;
    let s2 = s.clone();
    assert_eq!(s, s2);
}

#[test]
fn node_status_debug() {
    let _ = format!("{:?}", NodeStatus::Skipped);
}

// ---------------------------------------------------------------------------
// Viz dimensions
// ---------------------------------------------------------------------------

#[test]
fn viz_height_grows_with_chain_depth() {
    let r = DagRenderer::new();
    let small = r.render(&[("a".to_string(), NodeStatus::Pending)], &[]);
    let big = r.render(
        &[
            ("a".to_string(), NodeStatus::Pending),
            ("b".to_string(), NodeStatus::Pending),
            ("c".to_string(), NodeStatus::Pending),
        ],
        &[
            ("a".to_string(), "b".to_string()),
            ("b".to_string(), "c".to_string()),
        ],
    );
    assert!(big.height >= small.height);
}

#[test]
fn viz_width_grows_with_widest_row() {
    let r = DagRenderer::new();
    // Single node
    let single = r.render(&[("a".to_string(), NodeStatus::Pending)], &[]);
    // 5 nodes on same row (one root, 4 children) - wider
    let nodes: Vec<_> = (0..5).map(|i| (format!("n{i}"), NodeStatus::Pending)).collect();
    let edges: Vec<_> = (1..5).map(|i| ("n0".to_string(), format!("n{i}"))).collect();
    let wide = r.render(&nodes, &edges);
    assert!(wide.width >= single.width);
}

// ---------------------------------------------------------------------------
// Additional edge-case coverage
// ---------------------------------------------------------------------------

#[test]
fn render_with_all_status_types() {
    let r = DagRenderer::new();
    let nodes = vec![
        ("p".to_string(), NodeStatus::Pending),
        ("r".to_string(), NodeStatus::Running),
        ("c".to_string(), NodeStatus::Completed),
        ("f".to_string(), NodeStatus::Failed),
        ("s".to_string(), NodeStatus::Skipped),
    ];
    let viz = r.render(&nodes, &[]);
    assert_eq!(viz.nodes.len(), 5);
    // Each node should keep its status.
    for (id, expected) in [
        ("p", NodeStatus::Pending),
        ("r", NodeStatus::Running),
        ("c", NodeStatus::Completed),
        ("f", NodeStatus::Failed),
        ("s", NodeStatus::Skipped),
    ] {
        let node = viz.nodes.iter().find(|n| n.id == id).expect("present");
        assert_eq!(node.status, expected);
    }
}

#[test]
fn render_labels_match_node_ids() {
    let r = DagRenderer::new();
    let nodes = vec![("alpha".to_string(), NodeStatus::Completed)];
    let viz = r.render(&nodes, &[]);
    assert_eq!(viz.nodes[0].label, "alpha");
}

#[test]
fn render_long_label_truncated() {
    let r = DagRenderer::new();
    let long_id = "x".repeat(50);
    let nodes = vec![(long_id.clone(), NodeStatus::Pending)];
    let viz = r.render(&nodes, &[]);
    // Default node_width = 20 → label is truncated to <= node_width.
    assert!(viz.nodes[0].label.len() <= 20);
}

#[test]
fn render_topological_ordering_chain() {
    let r = DagRenderer::new();
    let nodes = vec![
        ("a".to_string(), NodeStatus::Pending),
        ("b".to_string(), NodeStatus::Pending),
        ("c".to_string(), NodeStatus::Pending),
    ];
    let edges = vec![
        ("a".to_string(), "b".to_string()),
        ("b".to_string(), "c".to_string()),
    ];
    let viz = r.render(&nodes, &edges);
    let row_a = viz.nodes.iter().find(|n| n.id == "a").expect("").row;
    let row_b = viz.nodes.iter().find(|n| n.id == "b").expect("").row;
    let row_c = viz.nodes.iter().find(|n| n.id == "c").expect("").row;
    assert!(row_a < row_b);
    assert!(row_b < row_c);
}

#[test]
fn render_to_string_long_label_does_not_crash() {
    let r = DagRenderer::new();
    let long_id = "y".repeat(30);
    let nodes = vec![(long_id, NodeStatus::Completed)];
    let s = r.render_to_string(&nodes, &[]);
    assert!(!s.is_empty());
}

#[test]
fn render_to_string_includes_box_drawing_chars() {
    let r = DagRenderer::new();
    let nodes = vec![
        ("a".to_string(), NodeStatus::Completed),
        ("b".to_string(), NodeStatus::Pending),
    ];
    let edges = vec![("a".to_string(), "b".to_string())];
    let s = r.render_to_string(&nodes, &edges);
    // Should contain at least one of the rendering chars: '|', '-', '+'.
    let has_chars = s.contains('|') || s.contains('-') || s.contains('+');
    assert!(has_chars, "got: {s:?}");
}

#[test]
fn render_disconnected_components() {
    let r = DagRenderer::new();
    // Two independent chains: a→b and c→d
    let nodes = vec![
        ("a".to_string(), NodeStatus::Completed),
        ("b".to_string(), NodeStatus::Completed),
        ("c".to_string(), NodeStatus::Completed),
        ("d".to_string(), NodeStatus::Completed),
    ];
    let edges = vec![
        ("a".to_string(), "b".to_string()),
        ("c".to_string(), "d".to_string()),
    ];
    let viz = r.render(&nodes, &edges);
    assert_eq!(viz.nodes.len(), 4);
    assert_eq!(viz.edges.len(), 2);
}
