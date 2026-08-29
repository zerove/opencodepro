//! Integration tests for sisyphus-knowledge asset graph.
//!
//! Exercises the in-memory `KnowledgeGraph` API used by the metacognition
//! loop to track cognitive assets and their typed relations. The graph is
//! purely synchronous and in-memory; these tests guard the public
//! invariants an external consumer depends on.

use sisyphus_knowledge::{AssetKind, AssetRelation, KnowledgeGraph, RelationType};
use sisyphus_types::asset::{AssetContent, ConstraintData, InsightData};

fn insight_asset(id: &str) -> sisyphus_knowledge::CognitiveAsset {
    sisyphus_knowledge::CognitiveAsset {
        id: id.to_string(),
        kind: AssetKind::Insight,
        name: id.to_string(),
        description: format!("insight: {id}"),
        content: AssetContent::Insight(InsightData {
            observation: format!("obs {id}"),
            reasoning: format!("reasoning {id}"),
            evidence_ids: vec![],
            confidence: 0.5,
        }),
        metadata: Default::default(),
        version: 1,
        created_at: 1_700_000_000,
    }
}

fn constraint_asset(id: &str) -> sisyphus_knowledge::CognitiveAsset {
    sisyphus_knowledge::CognitiveAsset {
        id: id.to_string(),
        kind: AssetKind::Constraint,
        name: id.to_string(),
        description: format!("constraint: {id}"),
        content: AssetContent::Constraint(ConstraintData {
            description: format!("c-{id}"),
            strictness: 0.8,
            violation_penalty: 1.0,
        }),
        metadata: Default::default(),
        version: 1,
        created_at: 1_700_000_000,
    }
}

#[test]
fn new_graph_is_empty() {
    let g = KnowledgeGraph::new();
    assert_eq!(g.asset_count(), 0);
    assert_eq!(g.relation_count(), 0);
    assert!(g.get_asset("nope").is_none());
}

#[test]
fn add_then_get_asset_round_trip() {
    let mut g = KnowledgeGraph::new();
    g.add_asset(insight_asset("a1"));

    let got = g.get_asset("a1").expect("a1 present");
    assert_eq!(got.id, "a1");
    assert_eq!(got.description, "insight: a1");
    assert_eq!(g.asset_count(), 1);
}

#[test]
fn find_assets_by_kind_filters_correctly() {
    let mut g = KnowledgeGraph::new();
    g.add_asset(insight_asset("i1"));
    g.add_asset(insight_asset("i2"));
    g.add_asset(constraint_asset("c1"));

    let insights = g.find_assets_by_kind(AssetKind::Insight);
    let constraints = g.find_assets_by_kind(AssetKind::Constraint);
    assert_eq!(insights.len(), 2);
    assert_eq!(constraints.len(), 1);
    assert!(insights.iter().all(|a| a.id.starts_with("i")));
}

#[test]
fn add_and_query_relations() {
    let mut g = KnowledgeGraph::new();
    g.add_asset(insight_asset("src"));
    g.add_asset(constraint_asset("dst"));
    g.add_relation(AssetRelation {
        from_id: "src".to_string(),
        to_id: "dst".to_string(),
        relation_type: RelationType::Refines,
        weight: 1.0,
    })
    .expect("relation between existing assets");

    assert_eq!(g.relation_count(), 1);
    let related = g.get_related_assets("src", RelationType::Refines);
    assert_eq!(related.len(), 1);
    assert_eq!(related[0].id, "dst");
}

#[test]
fn add_relation_to_missing_asset_returns_error() {
    let mut g = KnowledgeGraph::new();
    g.add_asset(insight_asset("only-src"));
    let result = g.add_relation(AssetRelation {
        from_id: "only-src".to_string(),
        to_id: "nonexistent".to_string(),
        relation_type: RelationType::ConflictsWith,
        weight: 1.0,
    });
    assert!(result.is_err(), "relation with unknown target must fail");
}

#[test]
fn snapshot_round_trip_preserves_assets() {
    let mut g = KnowledgeGraph::new();
    g.add_asset(insight_asset("a"));
    g.add_asset(constraint_asset("b"));
    g.add_relation(AssetRelation {
        from_id: "a".to_string(),
        to_id: "b".to_string(),
        relation_type: RelationType::Refines,
        weight: 1.0,
    })
    .expect("rel");

    let snap = g.to_snapshot();
    let restored = KnowledgeGraph::from_snapshot(snap);
    assert_eq!(restored.asset_count(), 2);
    assert_eq!(restored.relation_count(), 1);
    assert!(restored.get_asset("a").is_some());
    assert!(restored.get_asset("b").is_some());
}
