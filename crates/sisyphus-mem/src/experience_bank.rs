use crate::store::{ApplyResult, MfsStore};
use anyhow::Result;
use sisyphus_types::TypesError;
use sisyphus_types::experience::{
    ExperienceBank, ExperienceQuery, ExperienceTuple, bump_local_clock, experience_store_id,
};
use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use sisyphus_tools_llm::EmbeddingClient;

/// 本节点 ID。
///
/// 构造时解析一次（要用于序号恢复），写入时复用，避免两处读环境变量得到
/// 不同结果——一旦不一致，恢复出的序号会被算到错误的 origin 上，等于没恢复。
fn local_node_id() -> String {
    std::env::var("SISYPHUS_CLUSTER_NODE_ID")
        .unwrap_or_else(|_| std::env::var("HOSTNAME").unwrap_or_else(|_| "local-node".to_string()))
}

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
    /// 本地写入的单调序号。
    ///
    /// 必需：调用方（如 `execute_and_record`）每次都传入 `vector_clock` 为空、
    /// `version` 为 0 的新 tuple，`bump_local_clock` 只能从这个空输入自增，
    /// 于是**每一条本地经验都会得到同一个 version 与同一个 vc 计数**。
    /// 后果有二：同秒写入的行主键相同而互相覆盖；`merge_crdt` 的版本裁决
    /// 对本地写入完全失效。序号存在实例上，因此单调递增。
    ///
    /// 跨重启的单调性由 [`VectorExperienceBank::new`] 从存储层恢复：
    /// 不恢复而从 0 重开，重启后的新写入会拿到比历史更小的 `counter`，
    /// `version` 一并回退，远端节点按 CRDT 版本裁决会把这些**新**写入当成
    /// 旧版本丢弃——比主键碰撞更隐蔽，因为它丢的是刚发生的经验。
    local_seq: std::sync::atomic::AtomicU64,
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
        // 把本地序号抬到历史最大值之上，保证重启后写入的 version 不回退。
        // 查不到（空库/旧格式主键）时保持 0，与首次启动行为一致。
        let origin = local_node_id();
        let recovered = store.max_local_counter(&origin).await.unwrap_or_else(|e| {
            // 恢复失败不应阻止启动：退化行为与修复前一致（从 0 开始），
            // 但必须出声，否则会静默地退回到有缺陷的模式。
            tracing::warn!(
                error = %e,
                "failed to recover local experience sequence; falling back to 0",
            );
            0
        });
        if recovered > 0 {
            tracing::info!(
                recovered_seq = recovered,
                "resumed local experience sequence from store",
            );
        }

        Ok(Self {
            store,
            vector_store,
            embedding_client,
            local_seq: std::sync::atomic::AtomicU64::new(recovered),
        })
    }

    #[expect(dead_code)]
    const INDEX_KEY: &'static str = "exp:index";

    /// 底层存储句柄（ADR-010 反熵需要直接读主键与批量取载荷）。
    ///
    /// 反熵要回答两个问题：「我有什么」（摘要，主键即可）与「把这些行的
    /// 内容发出去」（需要完整载荷）。这两个都不是 `ExperienceBank` trait 的
    /// 能力，因此这里直接暴露 store；刻意不扩展 trait——它会被其他实现者
    /// 实现，而反熵确实只关心这个落盘实现。
    pub fn store(&self) -> &Arc<MfsStore> {
        &self.store
    }

    /// 导出本地经验库的紧凑摘要（ADR-010 反熵）。
    ///
    /// 返回 (node_id → 最大写入计数)。dial 方把它随 `SyncRequest` 发给对端，
    /// 对端据此算出自己该补发哪些行。
    pub async fn digest(&self) -> Result<sisyphus_types::experience::ExperienceDigest, TypesError> {
        self.store
            .experience_digest()
            .await
            .map_err(|e| TypesError::ExperienceError(format!("experience digest failed: {e}")))
    }

    /// 列出全部经验主键（ADR-010 反熵差量筛选）。
    pub async fn keys(&self) -> Result<Vec<String>, TypesError> {
        self.store
            .experience_keys()
            .await
            .map_err(|e| TypesError::ExperienceError(format!("experience keys failed: {e}")))
    }

    /// 按主键批量读取经验的完整载荷（ADR-010 反熵差量回送）。
    ///
    /// 返回顺序与 `ids` 一致，因此调用方可直接按切片分页。
    pub async fn load_by_keys(
        &self,
        ids: &[String],
    ) -> Result<Vec<(String, ExperienceTuple)>, TypesError> {
        self.store
            .load_experiences_by_keys(ids)
            .await
            .map_err(|e| TypesError::ExperienceError(format!("load experiences failed: {e}")))
    }

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
    /// `remote_key` 是发送方算出的主键（随 ExperienceSync 一同传输）。采用它
    /// 而非本地重算，才能保证同一行在对端与本地是同一个 id。
    pub async fn apply_remote(
        &self,
        exp: &ExperienceTuple,
        remote_key: &str,
    ) -> Result<ApplyResult, TypesError> {
        let key = remote_key.to_string();
        let outcome =
            self.store.apply_remote_experience(exp, &key).await.map_err(|e| {
                TypesError::ExperienceError(format!("apply remote experience: {e}"))
            })?;

        if matches!(outcome, ApplyResult::Applied) {
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

impl VectorExperienceBank {
    /// 写入一条本地经验，返回其存储主键。
    ///
    /// 主键必须由这里产生并回传给调用方：跨节点广播需要携带**与本地落库完全
    /// 相同**的主键，否则对端按自己的规则重算会把同一行存成两条。
    pub async fn record_with_id(&self, mut exp: ExperienceTuple) -> Result<String, TypesError> {
        // 0. CRDT (TODO 8.3.2): bump local version + vector_clock
        // 保持向后兼容：调用方未设置 version/vector_clock 时自动 bump。
        let node_id = local_node_id();
        // 本地序号保证单调：仅靠传入时钟自增无法做到（见 local_seq 字段说明）。
        let seq = self.local_seq.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        let (new_vc, new_ver) = bump_local_clock(&exp.vector_clock, exp.version, &node_id);
        exp.vector_clock = new_vc;
        exp.version = new_ver;
        // 本行的来源与写入计数。以本地序号为准，避免 vc 停在 1。
        let origin = node_id.clone();
        let counter = seq;
        let entry = exp.vector_clock.entry(origin.clone()).or_insert(0);
        if *entry < counter {
            *entry = counter;
        }
        if exp.version < counter {
            exp.version = counter;
        }

        // 1. Generate text embedding for the context
        let text_embedding = self
            .embedding_client
            .embed(&exp.context_text)
            .await
            .map_err(|e| TypesError::ExperienceError(format!("embedding failed: {e}")))?;
        exp.embedding = Some(text_embedding.clone());

        // 2. Store the full experience object in MfsStore for easy retrieval
        //    主键纳入来源节点与该节点的写入计数：仅用秒级 timestamp 会在
        //    同一秒内的两次写入之间撞主键，导致第二条静默丢失。
        let key = experience_store_id(exp.timestamp, &origin, counter);
        self.store
            .record_experience(&exp, &key)
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
            let vis_key = format!("vis_{key}");
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

        Ok(key)
    }
}

#[async_trait]
impl ExperienceBank for VectorExperienceBank {
    async fn record(&self, exp: ExperienceTuple) -> Result<(), TypesError> {
        self.record_with_id(exp).await.map(|_| ())
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
        let mut seen_keys = std::collections::HashSet::new();

        // Sort candidates by score (this is a simplification, in a real system we'd normalize scores)
        all_candidates
            .sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        // One round trip for every candidate instead of one per candidate.
        let mut wanted: Vec<String> = Vec::new();
        for sv in &all_candidates {
            if let Some(key) = sv.record.metadata.get("exp_key").and_then(|v| v.as_str())
                && seen_keys.insert(key.to_string())
            {
                wanted.push(key.to_string());
            }
        }

        let loaded = self.load_by_keys(&wanted).await?;
        let by_key: std::collections::HashMap<String, ExperienceTuple> =
            loaded.into_iter().collect();

        let mut experiences = Vec::new();
        for sv in all_candidates {
            let Some(exp_key) = sv.record.metadata.get("exp_key").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(exp) = by_key.get(exp_key) else {
                continue;
            };

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

            experiences.push(exp.clone());
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
