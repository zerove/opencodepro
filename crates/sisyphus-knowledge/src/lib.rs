//! Sisyphus Knowledge Management system for cognitive asset sedimentation.

use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use serde::{Serialize, Deserialize};
use thiserror::Error;
use sisyphus_types::asset::{CognitiveAsset, AssetRelation, RelationType, AssetKind};
use sisyphus_types::graph::SOPGraphConfig;
use sisyphus_types::tools::ToolSpec;

#[derive(Error, Debug)]
pub enum KnowledgeError {
    #[error("Asset not found: {0}")]
    AssetNotFound(String),
    #[error("Storage error: {0}")]
    StorageError(String),
    #[error("Internal error: {0}")]
    Internal(String),
}

pub struct KnowledgeGraph {
    graph: DiGraph<String, RelationType>,
    node_map: HashMap<String, NodeIndex>,
    assets: HashMap<String, CognitiveAsset>,
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
        let from_idx = self.node_map.get(&rel.from_id)
            .ok_or_else(|| KnowledgeError::AssetNotFound(rel.from_id.clone()))?;
        let to_idx = self.node_map.get(&rel.to_id)
            .ok_or_else(|| KnowledgeError::AssetNotFound(rel.to_id.clone()))?;
        
        self.graph.add_edge(*from_idx, *to_idx, rel.relation_type);
        Ok(())
    }

    pub fn get_asset(&self, id: &str) -> Option<&CognitiveAsset> {
        self.assets.get(id)
    }

    pub fn find_assets_by_kind(&self, kind: AssetKind) -> Vec<&CognitiveAsset> {
        self.assets.values()
            .filter(|a| std::mem::discriminant(&a.kind) == std::mem::discriminant(&kind))
            .collect()
    }

    pub fn get_related_assets(&self, id: &str, rel_type: RelationType) -> Vec<&CognitiveAsset> {
        let Some(node_idx) = self.node_map.get(id) else { return vec![] };
        
        self.graph.edges(*node_idx)
            .filter(|e| std::mem::discriminant(e.weight()) == std::mem::discriminant(&rel_type))
            .filter_map(|e| {
                let target_id = &self.graph[e.target()];
                self.assets.get(target_id)
            })
            .collect()
    }
}

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

    pub async fn save_asset(
        &self,
        id: &str,
        name: String,
        content: sisyphus_types::asset::AssetContent,
        confidence: f32,
    ) -> Result<(), KnowledgeError> {
        let asset = sisyphus_types::asset::CognitiveAsset {
            id: id.to_string(),
            kind: match &content {
                sisyphus_types::asset::AssetContent::Tool(_) => sisyphus_types::asset::AssetKind::Tool,
                sisyphus_types::asset::AssetContent::SOP(_) => sisyphus_types::asset::AssetKind::SOP,
                sisyphus_types::asset::AssetContent::Insight(_) => sisyphus_types::asset::AssetKind::Insight,
                sisyphus_types::asset::AssetContent::Constraint(_) => sisyphus_types::asset::AssetKind::Constraint,
            },
            name,
            description: "Sedimented asset".into(),
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

    pub async fn sediment_tool(&self, spec: ToolSpec, wasm_path: std::path::PathBuf) -> Result<String, KnowledgeError> {
        let id = format!("asset_tool_{}", uuid::Uuid::new_v4());
        let name = spec.tool_id.clone();
        let content = sisyphus_types::asset::AssetContent::Tool(spec);
        
        self.save_asset(&id, name, content, 1.0).await?;
        
        // Note: we are not storing the wasm_path in the current save_asset implementation.
        // In a real system, we would add it to metadata.
        
        Ok(id)
    }

    pub async fn get_asset(&self, id: &str) -> Result<CognitiveAsset, KnowledgeError> {
        let graph = self.graph.read().await;
        graph.get_asset(id)
            .cloned()
            .ok_or_else(|| KnowledgeError::AssetNotFound(id.to_string()))
    }

    pub async fn query_tools_for_goal(&self, goal: &str) -> Vec<CognitiveAsset> {
        let graph = self.graph.read().await;
        // For now, simple keyword match. Later will use embeddings.
        graph.assets.values()
            .filter(|a| a.description.contains(goal))
            .cloned()
            .collect()
    }

    pub async fn find_best_asset(
        &self, 
        goal: &str, 
        llm_client: Arc<dyn sisyphus_tools_llm::LlmClient>
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
            goal, candidate_descriptions.join("\n")
        );

        let response = llm_client.chat("You are a Knowledge Retrieval Agent.", &prompt, false).await
            .map_err(|e| KnowledgeError::Internal(e.to_string()))?;

        if let Ok(idx) = response.trim().parse::<usize>() {
            if let Some(asset) = candidates.get(idx) {
                return Ok(Some((*asset).clone()));
            }
        }

        Ok(None)
    }
}
