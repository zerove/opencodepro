//! Integration tests for sisyphus-intel `pattern_analyzer` module's
//! pure data types: `SubgraphPattern`, `ClusterAnalysis`,
//! `SemanticPatternAnalyzer::new`.

use sisyphus_intel::pattern_analyzer::{
    ClusterAnalysis, SemanticPatternAnalyzer, SubgraphPattern,
};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// SubgraphPattern
// ---------------------------------------------------------------------------

#[test]
fn subgraph_pattern_new() {
    let p = SubgraphPattern {
        nodes: vec![("a".into(), "t1".into())],
        edges: vec![],
    };
    assert_eq!(p.nodes.len(), 1);
    assert_eq!(p.edges.len(), 0);
}

#[test]
fn subgraph_pattern_field_access() {
    let p = SubgraphPattern {
        nodes: vec![("a".into(), "t1".into()), ("b".into(), "t2".into())],
        edges: vec![(0, 1)],
    };
    assert_eq!(p.nodes[0].0, "a");
    assert_eq!(p.nodes[0].1, "t1");
    assert_eq!(p.edges[0], (0, 1));
}

#[test]
fn subgraph_pattern_empty() {
    let p = SubgraphPattern {
        nodes: vec![],
        edges: vec![],
    };
    assert!(p.nodes.is_empty());
    assert!(p.edges.is_empty());
}

#[test]
fn subgraph_pattern_in_hashmap_uses_full_eq() {
    let mut map: HashMap<SubgraphPattern, usize> = HashMap::new();
    let p1 = SubgraphPattern {
        nodes: vec![("a".into(), "t".into())],
        edges: vec![],
    };
    let p2 = SubgraphPattern {
        nodes: vec![("a".into(), "t".into())],
        edges: vec![],
    };
    *map.entry(p1).or_insert(0) += 1;
    let back: SubgraphPattern = map.keys().next().unwrap().clone();
    let _ = map.entry(back).or_insert(0);
    // p1 == p2 (equal Hash/Eq), so the count should be combined
    map.entry(p2).and_modify(|c| *c += 1).or_insert(0);
    let total: usize = map.values().sum();
    assert_eq!(total, 2);
}

// ---------------------------------------------------------------------------
// ClusterAnalysis
// ---------------------------------------------------------------------------

#[test]
fn cluster_analysis_field_access() {
    let c = ClusterAnalysis {
        goal_cluster: "build".into(),
        common_failure_mode: "OOM".into(),
        evidence_count: 5,
        suggested_shift: "use iterators".into(),
    };
    assert_eq!(c.goal_cluster, "build");
    assert_eq!(c.common_failure_mode, "OOM");
    assert_eq!(c.evidence_count, 5);
    assert_eq!(c.suggested_shift, "use iterators");
}

#[test]
fn cluster_analysis_clone() {
    let c = ClusterAnalysis {
        goal_cluster: "g".into(),
        common_failure_mode: "f".into(),
        evidence_count: 1,
        suggested_shift: "s".into(),
    };
    let c2 = c.clone();
    assert_eq!(c2.goal_cluster, c.goal_cluster);
    assert_eq!(c2.evidence_count, c.evidence_count);
}

#[test]
fn cluster_analysis_debug_format() {
    let c = ClusterAnalysis {
        goal_cluster: "g".into(),
        common_failure_mode: "f".into(),
        evidence_count: 0,
        suggested_shift: "s".into(),
    };
    let s = format!("{c:?}");
    assert!(s.contains("g"));
    assert!(s.contains("f"));
    assert!(s.contains("s"));
}

// ---------------------------------------------------------------------------
// SemanticPatternAnalyzer
// ---------------------------------------------------------------------------

#[test]
fn semantic_pattern_analyzer_new() {
    let a = SemanticPatternAnalyzer::new(3);
    assert_eq!(a.min_cluster_size, 3);
}

#[test]
fn semantic_pattern_analyzer_with_zero_size() {
    let a = SemanticPatternAnalyzer::new(0);
    assert_eq!(a.min_cluster_size, 0);
}

#[test]
fn semantic_pattern_analyzer_large_cluster_size() {
    let a = SemanticPatternAnalyzer::new(1_000_000);
    assert_eq!(a.min_cluster_size, 1_000_000);
}

// ---------------------------------------------------------------------------
// Additional coverage
// ---------------------------------------------------------------------------

#[test]
fn subgraph_pattern_clone() {
    let p = SubgraphPattern {
        nodes: vec![("a".into(), "t1".into())],
        edges: vec![],
    };
    let cloned = p.clone();
    assert_eq!(cloned.nodes, p.nodes);
}

#[test]
fn subgraph_pattern_debug() {
    let p = SubgraphPattern {
        nodes: vec![],
        edges: vec![],
    };
    let dbg = format!("{:?}", p);
    assert!(dbg.contains("SubgraphPattern"));
}

#[test]
fn subgraph_pattern_unicode_node_names() {
    let p = SubgraphPattern {
        nodes: vec![
            ("节点-α".into(), "工具-1".into()),
            ("节点-β".into(), "工具-2".into()),
        ],
        edges: vec![(0, 1)],
    };
    assert_eq!(p.nodes[0].0, "节点-α");
    assert_eq!(p.edges[0], (0, 1));
}

#[test]
fn cluster_analysis_unicode() {
    let c = ClusterAnalysis {
        goal_cluster: "构建任务".into(),
        common_failure_mode: "内存溢出".into(),
        evidence_count: 10,
        suggested_shift: "使用迭代器".into(),
    };
    assert_eq!(c.goal_cluster, "构建任务");
    assert_eq!(c.evidence_count, 10);
}

#[test]
fn cluster_analysis_zero_evidence() {
    let c = ClusterAnalysis {
        goal_cluster: "".into(),
        common_failure_mode: "".into(),
        evidence_count: 0,
        suggested_shift: "".into(),
    };
    assert_eq!(c.evidence_count, 0);
    assert_eq!(c.goal_cluster, "");
}

#[test]
fn cluster_analysis_max_evidence() {
    let c = ClusterAnalysis {
        goal_cluster: "g".into(),
        common_failure_mode: "f".into(),
        evidence_count: usize::MAX,
        suggested_shift: "s".into(),
    };
    assert_eq!(c.evidence_count, usize::MAX);
}

#[test]
fn cluster_analysis_debug_contains_all_fields() {
    let c = ClusterAnalysis {
        goal_cluster: "g".into(),
        common_failure_mode: "f".into(),
        evidence_count: 1,
        suggested_shift: "s".into(),
    };
    let dbg = format!("{:?}", c);
    assert!(dbg.contains("ClusterAnalysis"));
    assert!(dbg.contains("goal_cluster"));
    assert!(dbg.contains("common_failure_mode"));
    assert!(dbg.contains("evidence_count"));
    assert!(dbg.contains("suggested_shift"));
}

#[test]
fn subgraph_pattern_in_hashmap_with_different_values() {
    // Different patterns should produce different hashmap entries
    let mut map: HashMap<SubgraphPattern, usize> = HashMap::new();
    let p1 = SubgraphPattern {
        nodes: vec![("a".into(), "t1".into())],
        edges: vec![],
    };
    let p2 = SubgraphPattern {
        nodes: vec![("b".into(), "t1".into())],
        edges: vec![],
    };
    map.insert(p1, 1);
    map.insert(p2, 2);
    assert_eq!(map.len(), 2);
}

#[test]
fn cluster_analysis_with_long_strings() {
    let long = "x".repeat(1000);
    let c = ClusterAnalysis {
        goal_cluster: long.clone(),
        common_failure_mode: long.clone(),
        evidence_count: 1,
        suggested_shift: long.clone(),
    };
    assert_eq!(c.goal_cluster.len(), 1000);
}
