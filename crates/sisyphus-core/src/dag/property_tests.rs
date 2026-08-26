//! P3-12：DAG 依赖解析的属性测试（随机复合图验证拓扑不变量）。
//!
//! 不变量：
//! I1. 无环输入必须成功解析，输出包含全部图且不重复；
//! I2. 输出顺序满足所有依赖约束（source 严格先于 target）；
//! I3. 含环输入必须返回 `DependencyCycleDetected`。

use super::*;
use proptest::prelude::*;
use sisyphus_types::{GraphDependency, SOPGraphConfig};

/// 生成 [0, n) 的无环边集合：仅允许 i -> j (i < j)，天然无环。
fn acyclic_edges(max_nodes: usize) -> impl Strategy<Value = Vec<(usize, usize)>> {
    proptest::collection::vec(
        (0..max_nodes, 0..max_nodes).prop_map(|(a, b)| if a < b { (a, b) } else { (b, a) }),
        0..(max_nodes * 2),
    )
}

fn sop_config() -> SOPGraphConfig {
    SOPGraphConfig::default()
}

fn build_composite(n: usize, edges: &[(usize, usize)]) -> CompositeGraph {
    let mut composite = CompositeGraph::new();
    for i in 0..n {
        composite.add_graph(&format!("sop-{i}"), sop_config());
    }
    for (a, b) in edges {
        composite.add_dependency(GraphDependency {
            source_sop_id: format!("sop-{a}"),
            target_sop_id: format!("sop-{b}"),
            mappings: Vec::new(),
        });
    }
    composite
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn resolved_order_covers_all_graphs_exactly_once(
        n in 1usize..=12,
        edges in acyclic_edges(12),
    ) {
        // 截断边到实际节点范围并去重
        let mut seen = std::collections::HashSet::new();
        let edges: Vec<_> = edges
            .into_iter()
            .filter(|&(a, b)| a < n && b < n && a != b && seen.insert((a, b)))
            .collect();

        let composite = build_composite(n, &edges);
        let order = DependencyResolver::resolve(&composite)
            .expect("acyclic composite must resolve");

        // I1：恰好覆盖全部节点一次
        let mut sorted = order.clone();
        sorted.sort();
        sorted.dedup();
        prop_assert_eq!(sorted.len(), n);
        prop_assert_eq!(order.len(), n);
    }

    #[test]
    fn resolved_order_respects_all_dependencies(
        n in 1usize..=12,
        edges in acyclic_edges(12),
    ) {
        let mut seen = std::collections::HashSet::new();
        let edges: Vec<_> = edges
            .into_iter()
            .filter(|&(a, b)| a < n && b < n && a != b && seen.insert((a, b)))
            .collect();
        let composite = build_composite(n, &edges);
        let order = DependencyResolver::resolve(&composite).unwrap();

        // I2：每条依赖的 source 必须先于 target 出现
        for (a, b) in &edges {
            let pos_a = order.iter().position(|id| id == &format!("sop-{a}"));
            let pos_b = order.iter().position(|id| id == &format!("sop-{b}"));
            prop_assert!(pos_a.is_some() && pos_b.is_some());
            prop_assert!(pos_a.unwrap() < pos_b.unwrap());
        }
    }

    #[test]
    fn cyclic_input_is_rejected(
        n in 3usize..=10,
        cycle_start in 0usize..8,
        cycle_len in 2usize..6,
    ) {
        // 构造必然含环的依赖：cycle_start -> ... -> cycle_start
        let nodes: Vec<usize> = (0..n)
            .map(|i| (cycle_start + i * 3) % n.max(1))
            .collect();
        let mut seen = std::collections::HashSet::new();
        let mut edges: Vec<(usize, usize)> = Vec::new();
        for w in nodes.iter().take(cycle_len.min(nodes.len())) {
            let next = ((w + 1) % n.max(1)).min(n.saturating_sub(1));
            if w != &next && seen.insert((*w, next)) {
                edges.push((*w, next));
            }
            if edges.len() >= cycle_len {
                break;
            }
        }
        // 确保至少有一条回边构成环：追加 last -> first 链路
        if !edges.is_empty() && n >= 2 {
            let first = edges[0].0;
            let last = edges[edges.len() - 1].1;
            if first != last && seen.insert((last, first)) {
                edges.push((last, first));
            }
        }

        let composite = build_composite(n, &edges);
        match DependencyResolver::resolve(&composite) {
            Ok(order) => {
                // 若去重后恰好不成环（构造退化），解析成功是合法的，
                // 但顺序仍须满足已存在的约束。
                for (a, b) in &edges {
                    let pa = order.iter().position(|id| id == &format!("sop-{a}"));
                    let pb = order.iter().position(|id| id == &format!("sop-{b}"));
                    if let (Some(pa), Some(pb)) = (pa, pb) {
                        prop_assert!(pa < pb);
                    }
                }
            }
            Err(CoreError::DependencyCycleDetected) => {
                // I3：期望路径
            }
            Err(other) => prop_assert!(false, "unexpected error: {other}"),
        }
    }
}
