use crate::store::MfsStore;
use anyhow::Result;
use sisyphus_types::TypesError;
use sisyphus_types::experience::{ExperienceBank, ExperienceQuery, ExperienceTuple};
use std::sync::Arc;

use async_trait::async_trait;
use sisyphus_tools_llm::EmbeddingClient;

pub struct VectorExperienceBank {
    store: Arc<MfsStore>,
    vector_store: Arc<crate::vector_memory::VectorMemoryStore>,
    embedding_client: Arc<dyn EmbeddingClient>,
}

impl VectorExperienceBank {
    pub async fn new(
        store: Arc<MfsStore>,
        vector_store: Arc<crate::vector_memory::VectorMemoryStore>,
        embedding_client: Arc<dyn EmbeddingClient>,
    ) -> Result<Self> {
        Ok(Self {
            store,
            vector_store,
            embedding_client,
        })
    }

    #[expect(dead_code)]
    const INDEX_KEY: &'static str = "exp:index";

    /// Make an experience globally shared: remove its project ownership so that
    /// it becomes visible to queries from any project (multi-workspace knowledge
    /// transfer). The vector index metadata is updated in lockstep.
    pub async fn share_experience(&self, exp_key: &str) -> Result<(), TypesError> {
        self.set_project_ownership(exp_key, None).await
    }

    /// Restore project ownership for a previously shared experience.
    pub async fn unshare_experience(
        &self,
        exp_key: &str,
        project_id: &str,
    ) -> Result<(), TypesError> {
        self.set_project_ownership(exp_key, Some(project_id)).await
    }

    /// List all globally shared experiences.
    pub async fn list_shared(
        &self,
    ) -> Result<Vec<sisyphus_types::experience::ExperienceTuple>, TypesError> {
        self.store
            .list_shared_experiences()
            .await
            .map_err(|e| TypesError::ExperienceError(format!("list shared failed: {e}")))
    }

    async fn set_project_ownership(
        &self,
        exp_key: &str,
        project_id: Option<&str>,
    ) -> Result<(), TypesError> {
        // 1. Update the canonical record in MfsStore.
        let updated = self
            .store
            .update_experience_project(exp_key, project_id)
            .await
            .map_err(|e| TypesError::ExperienceError(format!("update project failed: {e}")))?;
        if !updated {
            return Err(TypesError::ExperienceError(format!(
                "experience {exp_key} not found"
            )));
        }

        // 2. Keep the vector index metadata in sync (upsert with new project_id).
        let vectors = self
            .vector_store
            .search_by_metadata(serde_json::json!({ "exp_key": exp_key }), 16)
            .await
            .map_err(|e| TypesError::ExperienceError(format!("vector lookup failed: {e}")))?;

        for sv in vectors {
            let mut meta = sv.record.metadata.clone();
            match project_id {
                Some(pid) => meta["project_id"] = serde_json::json!(pid),
                None => meta["project_id"] = serde_json::Value::Null,
            }
            self.vector_store
                .store_embedding(&sv.record.key, sv.record.embedding, meta)
                .await
                .map_err(|e| {
                    TypesError::ExperienceError(format!("vector metadata sync failed: {e}"))
                })?;
        }

        Ok(())
    }
}

#[async_trait]
impl ExperienceBank for VectorExperienceBank {
    async fn record(&self, mut exp: ExperienceTuple) -> Result<(), TypesError> {
        // 1. Generate text embedding for the context
        let text_embedding = self
            .embedding_client
            .embed(&exp.context_text)
            .await
            .map_err(|e| TypesError::ExperienceError(format!("embedding failed: {e}")))?;
        exp.embedding = Some(text_embedding.clone());

        // 2. Store the full experience object in MfsStore for easy retrieval
        let key = format!("exp:{}", exp.timestamp);
        self.store
            .record_experience(&exp)
            .await
            .map_err(|e| TypesError::ExperienceError(format!("store record failed: {e}")))?;

        // 3. Store the text embedding in the optimized VectorMemoryStore
        let metadata = serde_json::json!({
            "exp_key": key,
            "success": exp.outcome_success,
            "timestamp": exp.timestamp,
            "project_id": exp.project_id,
            "modality": "text",
        });
        self.vector_store
            .store_embedding(&key, text_embedding, metadata)
            .await
            .map_err(|e| TypesError::ExperienceError(format!("vector store failed: {e}")))?;

        // 4. If visual embedding is provided, store it in the vector store as well
        if let Some(ref vis_emb) = exp.visual_embedding {
            let vis_key = format!("vis_exp:{}", exp.timestamp);
            let vis_metadata = serde_json::json!({
                "exp_key": key,
                "success": exp.outcome_success,
                "timestamp": exp.timestamp,
                "project_id": exp.project_id,
                "modality": "visual",
            });
            self.vector_store
                .store_embedding(&vis_key, vis_emb.clone(), vis_metadata)
                .await
                .map_err(|e| {
                    TypesError::ExperienceError(format!("visual vector store failed: {e}"))
                })?;
        }

        Ok(())
    }

    async fn query_similar(
        &self,
        query: ExperienceQuery,
    ) -> Result<Vec<ExperienceTuple>, TypesError> {
        let mut all_candidates = Vec::new();

        // 1. Text-based Retrieval
        let text_query_vec = self
            .embedding_client
            .embed(&query.query_text)
            .await
            .map_err(|e| TypesError::ExperienceError(format!("query embedding failed: {e}")))?;
        let text_top_k = query.limit * 5;
        let text_results = self
            .vector_store
            .search_similar(&text_query_vec, text_top_k)
            .await
            .map_err(|e| TypesError::ExperienceError(format!("text search failed: {e}")))?;

        for sv in text_results {
            all_candidates.push(sv);
        }

        // 2. Visual-based Retrieval
        if let Some(ref vis_query_vec) = query.visual_embedding {
            let vis_top_k = query.limit * 5;
            let vis_results = self
                .vector_store
                .search_similar(vis_query_vec, vis_top_k)
                .await
                .map_err(|e| TypesError::ExperienceError(format!("visual search failed: {e}")))?;

            for sv in vis_results {
                all_candidates.push(sv);
            }
        }

        // 3. De-duplicate and Filter
        let mut experiences = Vec::new();
        let mut seen_keys = std::collections::HashSet::new();

        // Sort candidates by score (this is a simplification, in a real system we'd normalize scores)
        all_candidates.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        for sv in all_candidates {
            let metadata = sv.record.metadata;
            let exp_key = metadata
                .get("exp_key")
                .and_then(|v| v.as_str())
                .ok_or_else(|| TypesError::ExperienceError("missing exp_key in metadata".into()))?;

            if seen_keys.contains(exp_key) {
                continue;
            }

            if let Some(exp) =
                self.store.get_experience(exp_key).await.map_err(|e| {
                    TypesError::ExperienceError(format!("get experience failed: {e}"))
                })?
            {
                // Project-based filtering:
                #[allow(clippy::collapsible_if)]
                if let Some(ref pid) = query.project_id {
                    if exp.project_id.as_ref() != Some(pid) && exp.project_id.is_some() {
                        continue;
                    }
                }

                #[allow(clippy::collapsible_if)]
                if let Some(ref action_id) = query.action_id {
                    if exp.action_id != *action_id {
                        continue;
                    }
                }

                experiences.push(exp);
                seen_keys.insert(exp_key.to_string());
            }
        }

        Ok(experiences.into_iter().take(query.limit).collect())
    }
}
