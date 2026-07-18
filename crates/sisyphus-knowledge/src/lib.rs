//! Sisyphus Knowledge Management system for cognitive asset sedimentation.
//!
//! Provides [`KnowledgeGraph`] for in-memory graph-based asset storage and
//! [`AssetManager`] for async CRUD, persistence, and intelligent retrieval.

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use serde::{Deserialize, Serialize};
use sisyphus_types::asset::{AssetKind, AssetRelation, CognitiveAsset, RelationType};
use sisyphus_types::tools::ToolSpec;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

// ── Errors ───────────────────────────────────────────────────────────────

#[derive(Error, Debug)]
pub enum KnowledgeError {
    #[error("Asset not found: {0}")]
    AssetNotFound(String),
    #[error("Storage error: {0}")]
    StorageError(String),
    #[error("Internal error: {0}")]
    Internal(String),
}

// ── Persistence snapshot ─────────────────────────────────────────────────

/// Serializable snapshot of the entire knowledge graph for disk persistence.
#[derive(Serialize, Deserialize)]
struct KnowledgeSnapshot {
    assets: Vec<CognitiveAsset>,
    relations: Vec<AssetRelation>,
}

// ── KnowledgeGraph ───────────────────────────────────────────────────────

pub struct KnowledgeGraph {
    graph: DiGraph<String, RelationType>,
    node_map: HashMap<String, NodeIndex>,
    assets: HashMap<String, CognitiveAsset>,
}

impl Default for KnowledgeGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl KnowledgeGraph {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            node_map: HashMap::new(),
            assets: HashMap::new(),
        }
    }

    pub fn add_asset(&mut self, asset: CognitiveAsset) {
        let id = asset.id.clone();
        let node_idx = self.graph.add_node(id.clone());
        self.node_map.insert(id.clone(), node_idx);
        self.assets.insert(id, asset);
    }

    pub fn add_relation(&mut self, rel: AssetRelation) -> Result<(), KnowledgeError> {
        let from_idx = self
            .node_map
            .get(&rel.from_id)
            .ok_or_else(|| KnowledgeError::AssetNotFound(rel.from_id.clone()))?;
        let to_idx = self
            .node_map
            .get(&rel.to_id)
            .ok_or_else(|| KnowledgeError::AssetNotFound(rel.to_id.clone()))?;

        self.graph.add_edge(*from_idx, *to_idx, rel.relation_type);
        Ok(())
    }

    pub fn get_asset(&self, id: &str) -> Option<&CognitiveAsset> {
        self.assets.get(id)
    }

    pub fn find_assets_by_kind(&self, kind: AssetKind) -> Vec<&CognitiveAsset> {
        self.assets
            .values()
            .filter(|a| std::mem::discriminant(&a.kind) == std::mem::discriminant(&kind))
            .collect()
    }

    pub fn get_related_assets(&self, id: &str, rel_type: RelationType) -> Vec<&CognitiveAsset> {
        let Some(node_idx) = self.node_map.get(id) else {
            return vec![];
        };

        self.graph
            .edges(*node_idx)
            .filter(|e| std::mem::discriminant(e.weight()) == std::mem::discriminant(&rel_type))
            .filter_map(|e| {
                let target_id = &self.graph[e.target()];
                self.assets.get(target_id)
            })
            .collect()
    }

    /// Serialize the graph to a JSON-serializable snapshot.
    fn to_snapshot(&self) -> KnowledgeSnapshot {
        let assets: Vec<CognitiveAsset> = self.assets.values().cloned().collect();

        // Reconstruct relations from the petgraph
        let mut relations = Vec::new();
        for edge in self.graph.edge_indices() {
            if let Some((from_idx, to_idx)) = self.graph.edge_endpoints(edge) {
                let from_id = self.graph[from_idx].clone();
                let to_id = self.graph[to_idx].clone();
                let rel_type = self.graph[edge].clone();
                relations.push(AssetRelation {
                    from_id,
                    to_id,
                    relation_type: rel_type,
                    weight: 1.0, // default weight
                });
            }
        }

        KnowledgeSnapshot { assets, relations }
    }

    /// Reconstruct from a snapshot.
    fn from_snapshot(snapshot: KnowledgeSnapshot) -> Self {
        let mut graph = KnowledgeGraph::new();
        for asset in snapshot.assets {
            graph.add_asset(asset);
        }
        for rel in snapshot.relations {
            let _ = graph.add_relation(rel); // ignore missing refs in snapshot
        }
        graph
    }

    pub fn asset_count(&self) -> usize {
        self.assets.len()
    }

    pub fn relation_count(&self) -> usize {
        self.graph.edge_count()
    }
}

// ── AssetManager ─────────────────────────────────────────────────────────

pub struct AssetManager {
    graph: Arc<RwLock<KnowledgeGraph>>,
    storage_path: std::path::PathBuf,
}

impl AssetManager {
    pub fn new(storage_path: std::path::PathBuf) -> Self {
        Self {
            graph: Arc::new(RwLock::new(KnowledgeGraph::new())),
            storage_path,
        }
    }

    /// Persist the knowledge graph to disk as JSON.
    pub async fn save(&self) -> Result<(), KnowledgeError> {
        let graph = self.graph.read().await;
        let snapshot = graph.to_snapshot();
        drop(graph);

        let json = serde_json::to_string_pretty(&snapshot)
            .map_err(|e| KnowledgeError::StorageError(e.to_string()))?;

        // Ensure parent directory exists
        if let Some(parent) = self.storage_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| KnowledgeError::StorageError(e.to_string()))?;
        }

        std::fs::write(&self.storage_path, &json)
            .map_err(|e| KnowledgeError::StorageError(e.to_string()))?;

        Ok(())
    }

    /// Load the knowledge graph from disk.
    pub async fn load(&self) -> Result<(), KnowledgeError> {
        if !self.storage_path.exists() {
            return Ok(()); // no file yet, start fresh
        }

        let json = std::fs::read_to_string(&self.storage_path)
            .map_err(|e| KnowledgeError::StorageError(e.to_string()))?;

        let snapshot: KnowledgeSnapshot =
            serde_json::from_str(&json).map_err(|e| KnowledgeError::StorageError(e.to_string()))?;

        let mut graph = self.graph.write().await;
        *graph = KnowledgeGraph::from_snapshot(snapshot);

        Ok(())
    }

    pub async fn save_asset(
        &self,
        id: &str,
        name: String,
        content: sisyphus_types::asset::AssetContent,
        _confidence: f32,
    ) -> Result<(), KnowledgeError> {
        let (kind, description) = match &content {
            sisyphus_types::asset::AssetContent::Tool(_) => {
                (AssetKind::Tool, "Sedimented tool".into())
            }
            sisyphus_types::asset::AssetContent::SOP(_) => {
                (AssetKind::SOP, "Sedimented SOP".into())
            }
            sisyphus_types::asset::AssetContent::Insight(i) => {
                (AssetKind::Insight, i.observation.clone())
            }
            sisyphus_types::asset::AssetContent::Constraint(c) => {
                (AssetKind::Constraint, c.description.clone())
            }
        };
        let asset = CognitiveAsset {
            id: id.to_string(),
            kind,
            name,
            description,
            content,
            metadata: std::collections::HashMap::new(),
            version: 1,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };

        let mut graph = self.graph.write().await;
        graph.add_asset(asset);
        Ok(())
    }

    pub async fn sediment_tool(
        &self,
        spec: ToolSpec,
        wasm_path: std::path::PathBuf,
    ) -> Result<String, KnowledgeError> {
        let id = format!("asset_tool_{}", uuid::Uuid::new_v4());
        let name = spec.tool_id.clone();
        let content = sisyphus_types::asset::AssetContent::Tool(spec);

        self.save_asset(&id, name, content, 1.0).await?;

        // Store wasm_path in metadata on next save
        let mut graph = self.graph.write().await;
        if let Some(asset) = graph.assets.get_mut(&id) {
            asset
                .metadata
                .insert("wasm_path".into(), wasm_path.to_string_lossy().to_string());
        }

        Ok(id)
    }

    pub async fn get_asset(&self, id: &str) -> Result<CognitiveAsset, KnowledgeError> {
        let graph = self.graph.read().await;
        graph
            .get_asset(id)
            .cloned()
            .ok_or_else(|| KnowledgeError::AssetNotFound(id.to_string()))
    }

    /// Search for assets matching a goal string.
    ///
    /// Uses case-insensitive substring matching across name, description,
    /// and content fields.  Returns results sorted by relevance (matches
    /// in name rank higher than matches in description, etc.).
    pub async fn query_tools_for_goal(&self, goal: &str) -> Vec<CognitiveAsset> {
        let graph = self.graph.read().await;
        let goal_lower = goal.to_lowercase();

        let mut scored: Vec<(i32, CognitiveAsset)> = graph
            .assets
            .values()
            .filter_map(|a| {
                let name_lower = a.name.to_lowercase();
                let desc_lower = a.description.to_lowercase();

                let name_match = name_lower.contains(&goal_lower);
                let desc_match = desc_lower.contains(&goal_lower);

                if !name_match && !desc_match {
                    return None;
                }

                // Score: name match = 10, description match = 5
                let score = if name_match { 10 } else { 0 } + if desc_match { 5 } else { 0 };

                Some((score, a.clone()))
            })
            .collect();

        scored.sort_by_key(|b| std::cmp::Reverse(b.0));
        scored.into_iter().map(|(_, a)| a).collect()
    }

    /// Use LLM to find the best matching asset for a goal.
    pub async fn find_best_asset(
        &self,
        goal: &str,
        llm_client: Arc<dyn sisyphus_tools_llm::LlmClient>,
    ) -> Result<Option<CognitiveAsset>, KnowledgeError> {
        let graph = self.graph.read().await;
        let candidates: Vec<&CognitiveAsset> = graph.assets.values().collect();

        if candidates.is_empty() {
            return Ok(None);
        }

        let mut candidate_descriptions = Vec::new();
        for (i, asset) in candidates.iter().enumerate() {
            candidate_descriptions.push(format!("{}: {} - {}", i, asset.name, asset.description));
        }

        let prompt = format!(
            "You are a Knowledge Retrieval Agent. Given the goal: '{}', \n\
            which of the following cognitive assets is the best match for a hot-start?\n\n\
            Candidates:\n{}\n\n\
            Return only the index of the best candidate, or 'NONE' if no asset is relevant.",
            goal,
            candidate_descriptions.join("\n")
        );

        let response = llm_client
            .chat("You are a Knowledge Retrieval Agent.", &prompt, false)
            .await
            .map_err(|e| KnowledgeError::Internal(e.to_string()))?;

        if let Ok(idx) = response.trim().parse::<usize>() {
            if let Some(asset) = candidates.get(idx) {
                return Ok(Some((*asset).clone()));
            }
        }

        Ok(None)
    }

    pub async fn asset_count(&self) -> usize {
        let graph = self.graph.read().await;
        graph.asset_count()
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sisyphus_types::asset::{AssetContent, InsightData};

    fn make_test_asset(id: &str, name: &str, description: &str) -> CognitiveAsset {
        CognitiveAsset {
            id: id.to_string(),
            kind: AssetKind::Insight,
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
    fn test_knowledge_graph_add_and_get() {
        let mut graph = KnowledgeGraph::new();
        let asset = make_test_asset("a1", "Test Asset", "A test");
        graph.add_asset(asset);
        assert!(graph.get_asset("a1").is_some());
        assert_eq!(graph.asset_count(), 1);
    }

    #[test]
    fn test_knowledge_graph_find_by_kind() {
        let mut graph = KnowledgeGraph::new();
        graph.add_asset(make_test_asset("a1", "Insight 1", "desc"));
        graph.add_asset(make_test_asset("a2", "Insight 2", "desc"));
        let insights = graph.find_assets_by_kind(AssetKind::Insight);
        assert_eq!(insights.len(), 2);
    }

    #[test]
    fn test_knowledge_graph_relation() {
        let mut graph = KnowledgeGraph::new();
        graph.add_asset(make_test_asset("a1", "A", "desc"));
        graph.add_asset(make_test_asset("a2", "B", "desc"));
        let rel = AssetRelation {
            from_id: "a1".into(),
            to_id: "a2".into(),
            relation_type: RelationType::Solves,
            weight: 1.0,
        };
        assert!(graph.add_relation(rel).is_ok());
        assert_eq!(graph.relation_count(), 1);
    }

    #[test]
    fn test_knowledge_graph_relation_missing_asset() {
        let mut graph = KnowledgeGraph::new();
        graph.add_asset(make_test_asset("a1", "A", "desc"));
        let rel = AssetRelation {
            from_id: "a1".into(),
            to_id: "missing".into(),
            relation_type: RelationType::Solves,
            weight: 1.0,
        };
        assert!(graph.add_relation(rel).is_err());
    }

    #[test]
    fn test_snapshot_roundtrip() {
        let mut graph = KnowledgeGraph::new();
        graph.add_asset(make_test_asset("a1", "Asset 1", "Description one"));
        graph.add_asset(make_test_asset("a2", "Asset 2", "Description two"));
        let rel = AssetRelation {
            from_id: "a1".into(),
            to_id: "a2".into(),
            relation_type: RelationType::Requires,
            weight: 0.8,
        };
        graph.add_relation(rel).unwrap();

        let snapshot = graph.to_snapshot();
        let restored = KnowledgeGraph::from_snapshot(snapshot);
        assert_eq!(restored.asset_count(), 2);
        assert_eq!(restored.relation_count(), 1);
        assert!(restored.get_asset("a1").is_some());
        assert!(restored.get_asset("a2").is_some());
    }

    #[tokio::test]
    async fn test_asset_manager_save_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("knowledge.json");

        // Save
        let manager = AssetManager::new(path.clone());
        manager
            .save_asset(
                "a1",
                "Test".into(),
                AssetContent::Insight(InsightData {
                    observation: "obs".into(),
                    reasoning: "reason".into(),
                    evidence_ids: vec![],
                    confidence: 0.9,
                }),
                1.0,
            )
            .await
            .unwrap();
        manager.save().await.unwrap();
        assert!(path.exists());

        // Load into a fresh manager
        let manager2 = AssetManager::new(path);
        manager2.load().await.unwrap();
        assert_eq!(manager2.asset_count().await, 1);
        let asset = manager2.get_asset("a1").await.unwrap();
        assert_eq!(asset.name, "Test");
    }

    #[tokio::test]
    async fn test_query_tools_for_goal() {
        let dir = tempfile::tempdir().unwrap();
        let manager = AssetManager::new(dir.path().join("q.json"));

        // Add assets with different names/descriptions
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

        // Search for "read" — should match a1 (name) and a3 (description)
        let results = manager.query_tools_for_goal("read").await;
        assert!(results.len() >= 2);
        // a1 should rank higher (name match)
        assert_eq!(results[0].id, "a1");
    }

    #[tokio::test]
    async fn test_query_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        let manager = AssetManager::new(dir.path().join("qi.json"));

        manager
            .save_asset(
                "a1",
                "MyTool".into(),
                AssetContent::Insight(InsightData {
                    observation: "A tool".into(),
                    reasoning: "".into(),
                    evidence_ids: vec![],
                    confidence: 1.0,
                }),
                1.0,
            )
            .await
            .unwrap();

        let results = manager.query_tools_for_goal("mytool").await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "a1");
    }

    #[tokio::test]
    async fn test_load_nonexistent_file() {
        let manager =
            AssetManager::new(std::path::PathBuf::from("/tmp/nonexistent_test_file.json"));
        assert!(manager.load().await.is_ok());
        assert_eq!(manager.asset_count().await, 0);
    }

    #[test]
    fn test_get_asset_nonexistent() {
        let graph = KnowledgeGraph::new();
        assert!(graph.get_asset("missing").is_none());
    }

    #[test]
    fn test_find_assets_by_kind_empty() {
        let graph = KnowledgeGraph::new();
        assert!(graph.find_assets_by_kind(AssetKind::Insight).is_empty());
    }

    #[test]
    fn test_find_assets_by_kind_wrong_kind() {
        let mut graph = KnowledgeGraph::new();
        graph.add_asset(make_test_asset("a1", "Insight only", "desc"));
        let tools = graph.find_assets_by_kind(AssetKind::Tool);
        assert!(tools.is_empty());
    }

    #[test]
    fn test_get_related_assets_no_edges() {
        let mut graph = KnowledgeGraph::new();
        graph.add_asset(make_test_asset("a1", "A", "desc"));
        let related = graph.get_related_assets("a1", RelationType::Solves);
        assert!(related.is_empty());
    }

    #[test]
    fn test_get_related_assets_wrong_relation_type() {
        let mut graph = KnowledgeGraph::new();
        graph.add_asset(make_test_asset("a1", "A", "desc"));
        graph.add_asset(make_test_asset("a2", "B", "desc"));
        graph
            .add_relation(AssetRelation {
                from_id: "a1".into(),
                to_id: "a2".into(),
                relation_type: RelationType::Solves,
                weight: 1.0,
            })
            .unwrap();
        let related = graph.get_related_assets("a1", RelationType::Requires);
        assert!(related.is_empty());
    }

    #[test]
    fn test_get_related_assets_nonexistent_id() {
        let graph = KnowledgeGraph::new();
        let related = graph.get_related_assets("ghost", RelationType::Solves);
        assert!(related.is_empty());
    }

    #[test]
    fn test_add_relation_both_missing() {
        let mut graph = KnowledgeGraph::new();
        let rel = AssetRelation {
            from_id: "missing_from".into(),
            to_id: "missing_to".into(),
            relation_type: RelationType::Solves,
            weight: 1.0,
        };
        let err = graph.add_relation(rel).unwrap_err();
        match err {
            KnowledgeError::AssetNotFound(id) => assert_eq!(id, "missing_from"),
            _ => panic!("expected AssetNotFound"),
        }
    }

    #[test]
    fn test_snapshot_empty_roundtrip() {
        let graph = KnowledgeGraph::new();
        let snapshot = graph.to_snapshot();
        let restored = KnowledgeGraph::from_snapshot(snapshot);
        assert_eq!(restored.asset_count(), 0);
        assert_eq!(restored.relation_count(), 0);
    }

    #[tokio::test]
    async fn test_query_no_match() {
        let dir = tempfile::tempdir().unwrap();
        let manager = AssetManager::new(dir.path().join("q.json"));
        manager
            .save_asset(
                "a1",
                "Git Ops".into(),
                AssetContent::Insight(InsightData {
                    observation: "does git".into(),
                    reasoning: "".into(),
                    evidence_ids: vec![],
                    confidence: 1.0,
                }),
                1.0,
            )
            .await
            .unwrap();
        let results = manager.query_tools_for_goal("unrelated_xyz").await;
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_query_empty_manager() {
        let dir = tempfile::tempdir().unwrap();
        let manager = AssetManager::new(dir.path().join("empty.json"));
        let results = manager.query_tools_for_goal("anything").await;
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_load_corrupted_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "{not valid json!!!").unwrap();
        let manager = AssetManager::new(path);
        let err = manager.load().await.unwrap_err();
        match err {
            KnowledgeError::StorageError(_) => {}
            _ => panic!("expected StorageError"),
        }
    }
}
