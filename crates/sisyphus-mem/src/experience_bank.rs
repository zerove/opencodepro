use crate::store::{ApplyResult, MfsStore};
use anyhow::Result;
use sisyphus_types::TypesError;
use sisyphus_types::experience::{
    ExperienceBank, ExperienceQuery, ExperienceTuple, bump_local_clock,
};
use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use sisyphus_tools_llm::EmbeddingClient;

/// 词元集合的 Jaccard 相似度：|A∩B| / |A∪B|。
///
/// 伪嵌入降级路径（P2-7）的词法打分基元：小写化后按非
/// 字母数字切分。空集合对空集合定义为 0.0（无信息）。
pub fn jaccard_similarity(a: &str, b: &str) -> f64 {
    let tokenize = |s: &str| -> HashSet<String> {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|t| !t.is_empty())
            .map(String::from)
            .collect()
    };
    let sa = tokenize(a);
    let sb = tokenize(b);
    if sa.is_empty() && sb.is_empty() {
        return 0.0;
    }
    let inter = sa.intersection(&sb).count();
    let union = sa.len() + sb.len() - inter;
    if union == 0 {
        return 0.0;
    }
    inter as f64 / union as f64
}

/// 嵌入检索不可用时的回退原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LexicalFallbackReason {
    /// 嵌入调用失败（网络/配额等瞬时错误）。
    EmbedFailed,
}

impl std::fmt::Display for LexicalFallbackReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmbedFailed => write!(f, "embedding call failed"),
        }
    }
}

/// Vector Experience Bank.
pub struct VectorExperienceBank {
    store: Arc<MfsStore>,
    vector_store: Arc<crate::vector_memory::VectorMemoryStore>,
    embedding_client: Arc<dyn EmbeddingClient>,
}

impl VectorExperienceBank {
    /// Open (or create) a persistent experience bank at the given `db_url`.
    ///
    /// All examples, queries, and outcomes are stored in a single SQLite
    /// database file; passing `sqlite::memory:` gives an ephemeral store
    /// for tests.
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

    /// 应用来自其他节点的经验（TODO 9.1 跨节点同步）。
    ///
    /// 与 [`ExperienceBank::record`] 的区别是这里**不做本地时钟 bump**：
    /// 远程经验携带的是发送节点的 version/vector_clock，合并判定完全交给
    /// [`MfsStore::apply_remote_experience`] 的 `merge_crdt`（LWW-Element-Set +
    /// vector clock）。若改走 `record()`，本地会把远端版本当作自己的新写入，
    /// 并发冲突退化为「最后到达者获胜」，同时丢失 happened-before 信息。
    ///
    /// 合并获胜时补建向量索引：CRDT 落库只写关系表，向量检索依赖本地的
    /// embedding，缺失会让远程经验无法被 `query_similar` 召回。
    pub async fn apply_remote(&self, exp: &ExperienceTuple) -> Result<ApplyResult, TypesError> {
        let outcome =
            self.store.apply_remote_experience(exp).await.map_err(|e| {
                TypesError::ExperienceError(format!("apply remote experience: {e}"))
            })?;

        if matches!(outcome, ApplyResult::Applied) {
            let key = format!("exp:{}", exp.timestamp);
            let text_embedding = self
                .embedding_client
                .embed(&exp.context_text)
                .await
                .map_err(|e| TypesError::ExperienceError(format!("embedding failed: {e}")))?;
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
        }

        Ok(outcome)
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
        // 0. CRDT (TODO 8.3.2): bump local version + vector_clock
        // 默认 node_id 用 hostname；后续可通过 Config.cluster.node_id 配置覆盖。
        // 保持向后兼容：调用方未设置 version/vector_clock 时自动 bump。
        let node_id = std::env::var("SISYPHUS_CLUSTER_NODE_ID").unwrap_or_else(|_| {
            std::env::var("HOSTNAME").unwrap_or_else(|_| "local-node".to_string())
        });
        let (new_vc, new_ver) = bump_local_clock(&exp.vector_clock, exp.version, &node_id);
        exp.vector_clock = new_vc;
        exp.version = new_ver;

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

        // 1. Text-based Retrieval（嵌入调用失败时词法回退，P2-7）
        let text_results = match self.embedding_client.embed(&query.query_text).await {
            Ok(text_query_vec) => {
                let text_top_k = query.limit * 5;
                self.vector_store
                    .search_similar(&text_query_vec, text_top_k)
                    .await
                    .map_err(|e| TypesError::ExperienceError(format!("text search failed: {e}")))?
            },
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "Embedding failed; falling back to lexical (Jaccard) retrieval.                      Results are token-overlap based, not semantic."
                );
                return self.lexical_fallback(&query, LexicalFallbackReason::EmbedFailed).await;
            },
        };

        for sv in text_results {
            all_candidates.push(sv);
        }

        // 2. Visual-based Retrieval
        if let Some(ref vis_query_vec) = query.visual_embedding {
            let vis_top_k = query.limit * 5;
            let vis_results =
                self.vector_store.search_similar(vis_query_vec, vis_top_k).await.map_err(|e| {
                    TypesError::ExperienceError(format!("visual search failed: {e}"))
                })?;

            for sv in vis_results {
                all_candidates.push(sv);
            }
        }

        // 3. De-duplicate and Filter
        let mut experiences = Vec::new();
        let mut seen_keys = std::collections::HashSet::new();

        // Sort candidates by score (this is a simplification, in a real system we'd normalize scores)
        all_candidates
            .sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

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

impl VectorExperienceBank {
    /// 词法回退检索：对最近经验按查询文本的 Jaccard 重合度打分。
    ///
    /// 伪嵌入场景下语义向量本就无意义（哈希正弦伪向量），
    /// 词法重合度是更诚实的相似度信号。扫描上限防止全表膨胀。
    const LEXICAL_SCAN_LIMIT: usize = 500;

    async fn lexical_fallback(
        &self,
        query: &ExperienceQuery,
        reason: LexicalFallbackReason,
    ) -> Result<Vec<ExperienceTuple>, TypesError> {
        tracing::info!(reason = %reason, scan_limit = Self::LEXICAL_SCAN_LIMIT, "Lexical fallback retrieval engaged");

        let candidates =
            self.store.list_recent_experiences(Self::LEXICAL_SCAN_LIMIT).await.map_err(|e| {
                TypesError::ExperienceError(format!("lexical fallback scan failed: {e}"))
            })?;

        let mut scored: Vec<(f64, ExperienceTuple)> = candidates
            .into_iter()
            .map(|exp| {
                let s = jaccard_similarity(&query.query_text, &exp.context_text);
                (s, exp)
            })
            .filter(|(score, _)| *score > 0.0)
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let mut results = Vec::new();
        for (_score, exp) in scored {
            // 项目与动作过滤与主路径语义一致
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
            // 排序已完成；ExperienceTuple 无分数字段，保持接口不变
            results.push(exp);
            if results.len() >= query.limit {
                break;
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jaccard_identical() {
        assert!(
            (jaccard_similarity("deploy the service", "deploy the service") - 1.0).abs() < 1e-9
        );
    }

    #[test]
    fn test_jaccard_disjoint() {
        assert_eq!(jaccard_similarity("alpha beta", "gamma delta"), 0.0);
    }

    #[test]
    fn test_jaccard_partial_overlap() {
        // {deploy, service} ∩ {deploy, retry} = {deploy}; union = 3
        let s = jaccard_similarity("Deploy Service", "deploy retry");
        assert!((s - 1.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn test_jaccard_empty_inputs() {
        assert_eq!(jaccard_similarity("", ""), 0.0);
        assert_eq!(jaccard_similarity("", "words here"), 0.0);
    }

    #[test]
    fn test_jaccard_case_and_punct_insensitive() {
        assert!((jaccard_similarity("Hello, World!", "hello world") - 1.0).abs() < 1e-9);
    }
}
