//! Integration tests for sisyphus-knowledge `KnowledgeGraph` pure methods.

use sisyphus_knowledge::KnowledgeGraph;
use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};

fn make_asset(id: &str, kind: AssetKind) -> CognitiveAsset {
    let content = match kind {
        AssetKind::Insight => AssetContent::Insight(InsightData {
            observation: format!("obs-{id}"),
            reasoning: "r".into(),
            evidence_ids: vec![],
            confidence: 0.5,
        }),
        _ => AssetContent::SOP(sisyphus_types::graph::SOPGraphConfig::default()),
    };
    CognitiveAsset {
        id: id.into(),
        kind,
        name: format!("name-{id}"),
        description: format!("desc-{id}"),
        content,
        metadata: Default::default(),
        version: 1,
        created_at: 100,
    }
}

#[test]
fn knowledge_graph_new_is_empty() {
    let g = KnowledgeGraph::new();
    assert!(g.get_asset("any").is_none());
    let _ = g.find_assets_by_kind(AssetKind::Tool);
}

#[test]
fn knowledge_graph_default_trait() {
    let g = KnowledgeGraph::default();
    assert!(g.get_asset("x").is_none());
}

#[test]
fn knowledge_graph_add_asset() {
    let mut g = KnowledgeGraph::new();
    let a = make_asset("a1", AssetKind::Insight);
    g.add_asset(a);
    let got = g.get_asset("a1");
    assert!(got.is_some());
    assert_eq!(got.unwrap().id, "a1");
}

#[test]
fn knowledge_graph_add_multiple_assets() {
    let mut g = KnowledgeGraph::new();
    g.add_asset(make_asset("a1", AssetKind::Insight));
    g.add_asset(make_asset("a2", AssetKind::Tool));
    g.add_asset(make_asset("a3", AssetKind::SOP));
    assert!(g.get_asset("a1").is_some());
    assert!(g.get_asset("a2").is_some());
    assert!(g.get_asset("a3").is_some());
    assert!(g.get_asset("a4").is_none());
}

#[test]
fn knowledge_graph_add_asset_replaces() {
    let mut g = KnowledgeGraph::new();
    g.add_asset(make_asset("a1", AssetKind::Insight));
    // Re-adding with same ID should replace (no panic)
    g.add_asset(make_asset("a1", AssetKind::Tool));
    let got = g.get_asset("a1").unwrap();
    assert!(matches!(got.kind, AssetKind::Tool));
}

#[test]
fn knowledge_graph_find_assets_by_kind() {
    let mut g = KnowledgeGraph::new();
    g.add_asset(make_asset("a1", AssetKind::Insight));
    g.add_asset(make_asset("a2", AssetKind::Insight));
    g.add_asset(make_asset("b1", AssetKind::Tool));
    let insights = g.find_assets_by_kind(AssetKind::Insight);
    assert_eq!(insights.len(), 2);
    let tools = g.find_assets_by_kind(AssetKind::Tool);
    assert_eq!(tools.len(), 1);
}

#[test]
fn knowledge_graph_find_assets_by_kind_empty() {
    let g = KnowledgeGraph::new();
    let insights = g.find_assets_by_kind(AssetKind::Insight);
    assert!(insights.is_empty());
}

#[test]
fn knowledge_graph_find_assets_by_kind_no_matches() {
    let mut g = KnowledgeGraph::new();
    g.add_asset(make_asset("a1", AssetKind::Insight));
    let sops = g.find_assets_by_kind(AssetKind::SOP);
    assert!(sops.is_empty());
}

#[test]
fn knowledge_graph_get_related_unknown() {
    let g = KnowledgeGraph::new();
    let r = g.get_related_assets("nope", sisyphus_types::asset::RelationType::Solves);
    assert!(r.is_empty());
}

#[test]
fn knowledge_graph_add_relation_missing_from() {
    let mut g = KnowledgeGraph::new();
    g.add_asset(make_asset("to1", AssetKind::Insight));
    let rel = sisyphus_types::asset::AssetRelation {
        from_id: "missing".into(),
        to_id: "to1".into(),
        relation_type: sisyphus_types::asset::RelationType::Solves,
        weight: 1.0,
    };
    assert!(g.add_relation(rel).is_err());
}

#[test]
fn knowledge_graph_add_relation_missing_to() {
    let mut g = KnowledgeGraph::new();
    g.add_asset(make_asset("from1", AssetKind::Insight));
    let rel = sisyphus_types::asset::AssetRelation {
        from_id: "from1".into(),
        to_id: "missing".into(),
        relation_type: sisyphus_types::asset::RelationType::Solves,
        weight: 1.0,
    };
    assert!(g.add_relation(rel).is_err());
}

#[test]
fn knowledge_graph_add_relation_succeeds() {
    let mut g = KnowledgeGraph::new();
    g.add_asset(make_asset("from1", AssetKind::Insight));
    g.add_asset(make_asset("to1", AssetKind::Insight));
    let rel = sisyphus_types::asset::AssetRelation {
        from_id: "from1".into(),
        to_id: "to1".into(),
        relation_type: sisyphus_types::asset::RelationType::Solves,
        weight: 1.0,
    };
    assert!(g.add_relation(rel).is_ok());
    let related = g.get_related_assets("from1", sisyphus_types::asset::RelationType::Solves);
    assert_eq!(related.len(), 1);
    assert_eq!(related[0].id, "to1");
}

#[test]
fn knowledge_graph_get_related_wrong_relation_type() {
    let mut g = KnowledgeGraph::new();
    g.add_asset(make_asset("a", AssetKind::Insight));
    g.add_asset(make_asset("b", AssetKind::Insight));
    let rel = sisyphus_types::asset::AssetRelation {
        from_id: "a".into(),
        to_id: "b".into(),
        relation_type: sisyphus_types::asset::RelationType::Solves,
        weight: 1.0,
    };
    g.add_relation(rel).unwrap();
    // Query with different relation type → empty
    let related = g.get_related_assets("a", sisyphus_types::asset::RelationType::Requires);
    assert!(related.is_empty());
}

#[test]
fn knowledge_graph_to_snapshot_round_trip() {
    let mut g = KnowledgeGraph::new();
    g.add_asset(make_asset("a1", AssetKind::Insight));
    g.add_asset(make_asset("a2", AssetKind::Tool));
    let snap = g.to_snapshot();
    let g2 = KnowledgeGraph::from_snapshot(snap);
    assert!(g2.get_asset("a1").is_some());
    assert!(g2.get_asset("a2").is_some());
}
