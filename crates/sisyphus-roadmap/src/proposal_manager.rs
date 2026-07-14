use std::sync::Arc;
use sisyphus_mem::MfsStore;
use sisyphus_types::StateStore;
use crate::strategic_reflection::{StrategicProposal, ProposalStatus};
use crate::RoadmapError;

/// Manages the lifecycle of strategic proposals.
/// Handles persistence via MfsStore and status transitions.
pub struct ProposalManager {
    store: Arc<MfsStore>,
}

impl ProposalManager {
    pub fn new(store: Arc<MfsStore>) -> Self {
        Self { store }
    }

    /// Submit a new proposal for human review.
    pub async fn submit_proposal(&self, project_id: &str, proposal: StrategicProposal) -> Result<(), RoadmapError> {
        let key = format!("proposal:{}", proposal.proposal_id);
        let val = serde_json::to_value(&proposal)
            .map_err(|e| RoadmapError::Internal(format!("Serialization failed: {e}")))?;
        
        self.store.set_state_json(project_id, &key, val)
            .await
            .map_err(|e| RoadmapError::Internal(format!("Store save failed: {e}")))?;
        
        Ok(())
    }

    /// Retrieve a specific proposal by ID.
    pub async fn get_proposal(&self, project_id: &str, proposal_id: &str) -> Result<Option<StrategicProposal>, RoadmapError> {
        let key = format!("proposal:{}", proposal_id);
        let val = self.store.get_state_json(project_id, &key)
            .await
            .map_err(|e| RoadmapError::Internal(format!("Store load failed: {e}")))?;
        
        match val {
            Some(v) => Ok(Some(serde_json::from_value(v).map_err(|e| RoadmapError::Internal(format!("Deserialization failed: {e}")))?)),
            None => Ok(None),
        }
    }

    /// Approve a proposal and mark it as approved by the reviewer.
    pub async fn approve_proposal(&self, project_id: &str, proposal_id: &str, reviewer: &str) -> Result<(), RoadmapError> {
        let mut proposal = self.get_proposal(project_id, proposal_id).await?
            .ok_or_else(|| RoadmapError::Internal("Proposal not found".into()))?;
        
        proposal.status = ProposalStatus::Approved;
        proposal.reviewer = Some(reviewer.to_string());
        
        self.submit_proposal(project_id, proposal).await
    }

    /// Reject a proposal.
    pub async fn reject_proposal(&self, project_id: &str, proposal_id: &str, reviewer: &str) -> Result<(), RoadmapError> {
        let mut proposal = self.get_proposal(project_id, proposal_id).await?
            .ok_or_else(|| RoadmapError::Internal("Proposal not found".into()))?;
        
        proposal.status = ProposalStatus::Rejected;
        proposal.reviewer = Some(reviewer.to_string());
        
        self.submit_proposal(project_id, proposal).await
    }

    /// List all proposals for a specific project.
        pub async fn list_proposals(&self, _project_id: &str) -> Result<Vec<StrategicProposal>, RoadmapError> {
        // Since MfsStore doesn't support complex queries, we use a simplified 
        // approach: we assume proposals are listed via a project-specific index 
        // if we had one. For now, we implement a basic filter (this is a prototype).
        
        // In a real production system, we would maintain a 'project:proposals' list.
        // For this implementation, we'll return an empty list or implement basic scanning.
        Ok(vec![]) 
    }
}
