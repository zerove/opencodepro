// Integration tests for sisyphus-knowledge
// Tests the KnowledgeGraph and AssetManager public APIs

use sisyphus_knowledge::{AssetManager, KnowledgeGraph};
use sisyphus_types::asset::{
    AssetContent, AssetKind, AssetRelation, CognitiveAsset, InsightData, RelationType,
};
use std::collections::HashMap;
use tempfile::TempDir;

fn make_test_asset(id: &str, name: &str, description: &str, kind: AssetKind) -> CognitiveAsset {
    CognitiveAsset {
        id: id.to_string(),
        kind,
        name: name.to_string(),
        description: description.to_string(),
        content: AssetContent::Insight(InsightData {
            observation: "test observation".into(),
            reasoning: "test reasoning".into(),
            evidence_ids: vec![],
            confidence: 0.9,
        }),
        metadata: HashMap::new(),
        version: 1,
        created_at: 1000,
    }
}

#[test]
fn test_knowledge_graph_basic_operations() {
    let mut graph = KnowledgeGraph::new();

    // Test adding assets
    let asset1 = make_test_asset("a1", "Test Asset 1", "Description 1", AssetKind::Insight);
    graph.add_asset(asset1);
    assert_eq!(graph.asset_count(), 1);
    assert!(graph.get_asset("a1").is_some());

    let asset2 = make_test_asset("a2", "Test Asset 2", "Description 2", AssetKind::Tool);
    graph.add_asset(asset2);
    assert_eq!(graph.asset_count(), 2);

    // Test find by kind
    let insights = graph.find_assets_by_kind(AssetKind::Insight);
    assert_eq!(insights.len(), 1);
    let tools = graph.find_assets_by_kind(AssetKind::Tool);
    assert_eq!(tools.len(), 1);
}

#[test]
fn test_knowledge_graph_relations() {
    let mut graph = KnowledgeGraph::new();

    graph.add_asset(make_test_asset(
        "a1",
        "Asset 1",
        "First",
        AssetKind::Insight,
    ));
    graph.add_asset(make_test_asset(
        "a2",
        "Asset 2",
        "Second",
        AssetKind::Insight,
    ));
    graph.add_asset(make_test_asset(
        "a3",
        "Asset 3",
        "Third",
        AssetKind::Insight,
    ));

    // Add relations
    let rel1 = AssetRelation {
        from_id: "a1".into(),
        to_id: "a2".into(),
        relation_type: RelationType::Solves,
        weight: 1.0,
    };
    graph.add_relation(rel1).unwrap();

    let rel2 = AssetRelation {
        from_id: "a2".into(),
        to_id: "a3".into(),
        relation_type: RelationType::Requires,
        weight: 0.8,
    };
    graph.add_relation(rel2).unwrap();

    assert_eq!(graph.relation_count(), 2);

    // Test get related assets
    let related = graph.get_related_assets("a1", RelationType::Solves);
    assert_eq!(related.len(), 1);
    assert_eq!(related[0].id, "a2");

    let related = graph.get_related_assets("a2", RelationType::Requires);
    assert_eq!(related.len(), 1);
    assert_eq!(related[0].id, "a3");

    // Test missing asset error
    let bad_rel = AssetRelation {
        from_id: "a1".into(),
        to_id: "missing".into(),
        relation_type: RelationType::Solves,
        weight: 1.0,
    };
    assert!(graph.add_relation(bad_rel).is_err());
}

#[test]
fn test_knowledge_graph_snapshot_roundtrip() {
    let mut graph = KnowledgeGraph::new();
    graph.add_asset(make_test_asset(
        "a1",
        "Asset 1",
        "Desc 1",
        AssetKind::Insight,
    ));
    graph.add_asset(make_test_asset("a2", "Asset 2", "Desc 2", AssetKind::SOP));

    let rel = AssetRelation {
        from_id: "a1".into(),
        to_id: "a2".into(),
        relation_type: RelationType::Requires,
        weight: 0.8,
    };
    graph.add_relation(rel).unwrap();

    // Serialize and restore
    let snapshot = graph.to_snapshot();
    let restored = KnowledgeGraph::from_snapshot(snapshot);

    assert_eq!(restored.asset_count(), 2);
    assert_eq!(restored.relation_count(), 1);
    assert!(restored.get_asset("a1").is_some());
    assert!(restored.get_asset("a2").is_some());
}

#[tokio::test]
async fn test_asset_manager_persistence() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("knowledge.json");

    // Create manager and save assets
    let manager = AssetManager::new(path.clone());

    manager
        .save_asset(
            "test_asset",
            "Test Tool".into(),
            AssetContent::Insight(InsightData {
                observation: "Does something useful".into(),
                reasoning: "Because it's needed".into(),
                evidence_ids: vec![],
                confidence: 0.95,
            }),
            1.0,
        )
        .await
        .unwrap();

    manager.save().await.unwrap();
    assert!(path.exists());

    // Load into fresh manager
    let manager2 = AssetManager::new(path.clone());
    manager2.load().await.unwrap();

    assert_eq!(manager2.asset_count().await, 1);

    let asset = manager2.get_asset("test_asset").await.unwrap();
    assert_eq!(asset.name, "Test Tool");
    assert_eq!(asset.id, "test_asset");
}

#[tokio::test]
async fn test_asset_manager_query() {
    let dir = TempDir::new().unwrap();
    let manager = AssetManager::new(dir.path().join("query.json"));

    // Add various assets
    manager
        .save_asset(
            "a1",
            "File Reader".into(),
            AssetContent::Insight(InsightData {
                observation: "Reads files from disk".into(),
                reasoning: "".into(),
                evidence_ids: vec![],
                confidence: 1.0,
            }),
            1.0,
        )
        .await
        .unwrap();

    manager
        .save_asset(
            "a2",
            "Git Operations".into(),
            AssetContent::Insight(InsightData {
                observation: "Performs git commands".into(),
                reasoning: "".into(),
                evidence_ids: vec![],
                confidence: 1.0,
            }),
            1.0,
        )
        .await
        .unwrap();

    manager
        .save_asset(
            "a3",
            "Database Query".into(),
            AssetContent::Insight(InsightData {
                observation: "Reads from database".into(),
                reasoning: "".into(),
                evidence_ids: vec![],
                confidence: 1.0,
            }),
            1.0,
        )
        .await
        .unwrap();

    // Search for "read" - should match a1 (name) and a3 (description)
    let results = manager.query_tools_for_goal("read").await;
    assert!(results.len() >= 2);
    // a1 should rank higher (name match)
    assert_eq!(results[0].id, "a1");

    // Case insensitive search
    let results = manager.query_tools_for_goal("FILE").await;
    assert!(!results.is_empty());
    assert_eq!(results[0].id, "a1");

    // No match
    let results = manager.query_tools_for_goal("unrelated_xyz").await;
    assert!(results.is_empty());
}

#[tokio::test]
async fn test_asset_manager_sediment_tool() {
    let dir = TempDir::new().unwrap();
    let manager = AssetManager::new(dir.path().join("tool.json"));

    let spec = sisyphus_types::tools::ToolSpec {
        tool_id: "test_tool".into(),
        required_capability: "file reading".into(),
        expected_input: serde_json::json!({"type": "object"}),
        expected_output: serde_json::json!({"type": "object"}),
        success_criteria: "file read successfully".into(),
        test_command: None,
        workdir: None,
    };

    let wasm_path = dir.path().join("tool.wasm");
    let id = manager.sediment_tool(spec, wasm_path.clone()).await.unwrap();

    assert!(id.starts_with("asset_tool_"));

    // Verify asset was created
    let asset = manager.get_asset(&id).await.unwrap();
    assert!(matches!(asset.kind, AssetKind::Tool));
    assert_eq!(
        asset.metadata.get("wasm_path"),
        Some(&wasm_path.to_string_lossy().to_string())
    );
}

#[tokio::test]
async fn test_asset_manager_load_nonexistent() {
    let manager = AssetManager::new(std::path::PathBuf::from(
        "/tmp/nonexistent_test_file_12345.json",
    ));
    assert!(manager.load().await.is_ok());
    assert_eq!(manager.asset_count().await, 0);
}

#[tokio::test]
async fn test_asset_manager_load_corrupted() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("bad.json");
    std::fs::write(&path, "{not valid json!!!").unwrap();

    let manager = AssetManager::new(path);
    let err = manager.load().await.unwrap_err();
    match err {
        sisyphus_knowledge::KnowledgeError::StorageError(_) => {},
        _ => panic!("expected StorageError"),
    }
}

#[tokio::test]
async fn test_asset_manager_concurrent_access() {
    let dir = TempDir::new().unwrap();
    let manager = std::sync::Arc::new(AssetManager::new(dir.path().join("concurrent.json")));

    // Spawn multiple tasks saving different assets
    let mut handles = vec![];
    for i in 0..10 {
        let mgr = manager.clone();
        let handle = tokio::spawn(async move {
            let id = format!("asset_{}", i);
            mgr.save_asset(
                &id,
                format!("Asset {}", i),
                AssetContent::Insight(InsightData {
                    observation: format!("Observation {}", i),
                    reasoning: "test".into(),
                    evidence_ids: vec![],
                    confidence: 0.9,
                }),
                1.0,
            )
            .await
            .unwrap();
        });
        handles.push(handle);
    }

    for h in handles {
        h.await.unwrap();
    }

    assert_eq!(manager.asset_count().await, 10);

    // Verify all assets can be retrieved
    for i in 0..10 {
        let id = format!("asset_{}", i);
        let asset = manager.get_asset(&id).await.unwrap();
        assert_eq!(asset.id, id);
    }
}

#[tokio::test]
async fn test_asset_manager_empty_query() {
    let dir = TempDir::new().unwrap();
    let manager = AssetManager::new(dir.path().join("empty.json"));

    let results = manager.query_tools_for_goal("anything").await;
    assert!(results.is_empty());
}

#[tokio::test]
async fn test_asset_manager_multiple_relations() {
    let mut graph = KnowledgeGraph::new();

    graph.add_asset(make_test_asset("a1", "A", "First", AssetKind::Insight));
    graph.add_asset(make_test_asset("a2", "B", "Second", AssetKind::Insight));
    graph.add_asset(make_test_asset("a3", "C", "Third", AssetKind::Insight));
    graph.add_asset(make_test_asset("a4", "D", "Fourth", AssetKind::Insight));

    // Multiple relations from a1
    graph
        .add_relation(AssetRelation {
            from_id: "a1".into(),
            to_id: "a2".into(),
            relation_type: RelationType::Solves,
            weight: 1.0,
        })
        .unwrap();

    graph
        .add_relation(AssetRelation {
            from_id: "a1".into(),
            to_id: "a3".into(),
            relation_type: RelationType::Requires,
            weight: 0.5,
        })
        .unwrap();

    graph
        .add_relation(AssetRelation {
            from_id: "a1".into(),
            to_id: "a4".into(),
            relation_type: RelationType::Solves,
            weight: 0.8,
        })
        .unwrap();

    // Query by relation type
    let solves = graph.get_related_assets("a1", RelationType::Solves);
    assert_eq!(solves.len(), 2);

    let requires = graph.get_related_assets("a1", RelationType::Requires);
    assert_eq!(requires.len(), 1);
}
