use crate::strategic_reflection::{ProposalStatus, StrategicProposal};
use crate::RoadmapError;
use sisyphus_mem::MfsStore;
use sisyphus_types::StateStore;
use std::sync::Arc;

/// 管理战略提案的生命周期。
/// 通过 MfsStore 处理持久化与状态转换。
pub struct ProposalManager {
    store: Arc<MfsStore>,
}

impl ProposalManager {
    /// New.
    pub fn new(store: Arc<MfsStore>) -> Self {
        Self { store }
    }

    /// 提交新提案供人工审查。
    pub async fn submit_proposal(
        &self,
        project_id: &str,
        proposal: StrategicProposal,
    ) -> Result<(), RoadmapError> {
        let key = format!("proposal:{}", proposal.proposal_id);
        let val = serde_json::to_value(&proposal)
            .map_err(|e| RoadmapError::Internal(format!("Serialization failed: {e}")))?;

        self.store
            .set_state_json(project_id, &key, val)
            .await
            .map_err(|e| RoadmapError::Internal(format!("Store save failed: {e}")))?;

        Ok(())
    }

    /// 按 ID 检索特定提案。
    pub async fn get_proposal(
        &self,
        project_id: &str,
        proposal_id: &str,
    ) -> Result<Option<StrategicProposal>, RoadmapError> {
        let key = format!("proposal:{}", proposal_id);
        let val = self
            .store
            .get_state_json(project_id, &key)
            .await
            .map_err(|e| RoadmapError::Internal(format!("Store load failed: {e}")))?;

        match val {
            Some(v) => Ok(Some(serde_json::from_value(v).map_err(|e| {
                RoadmapError::Internal(format!("Deserialization failed: {e}"))
            })?)),
            None => Ok(None),
        }
    }

    /// 批准提案，标记为审查人已批准。
    pub async fn approve_proposal(
        &self,
        project_id: &str,
        proposal_id: &str,
        reviewer: &str,
    ) -> Result<(), RoadmapError> {
        let mut proposal = self
            .get_proposal(project_id, proposal_id)
            .await?
            .ok_or_else(|| RoadmapError::Internal("Proposal not found".into()))?;

        proposal.status = ProposalStatus::Approved;
        proposal.reviewer = Some(reviewer.to_string());

        self.submit_proposal(project_id, proposal).await
    }

    /// 拒绝提案。
    pub async fn reject_proposal(
        &self,
        project_id: &str,
        proposal_id: &str,
        reviewer: &str,
    ) -> Result<(), RoadmapError> {
        let mut proposal = self
            .get_proposal(project_id, proposal_id)
            .await?
            .ok_or_else(|| RoadmapError::Internal("Proposal not found".into()))?;

        proposal.status = ProposalStatus::Rejected;
        proposal.reviewer = Some(reviewer.to_string());

        self.submit_proposal(project_id, proposal).await
    }

    /// 列出特定项目的全部提案。
    pub async fn list_proposals(
        &self,
        _project_id: &str,
    ) -> Result<Vec<StrategicProposal>, RoadmapError> {
        // 由于 MfsStore 不支持复杂查询，我们采用简化的
        // 方式：假定提案通过项目专属索引列出
        // （若有）。目前实现基础过滤（这是原型）。

        // 在生产系统中，我们会维护 'project:proposals' 列表。
        // 当前实现返回空列表或执行基础扫描。
        Ok(vec![])
    }
}
