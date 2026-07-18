//! Management of human-in-the-loop interventions for Sisyphus.

use std::sync::Arc;
use tokio::sync::{oneshot, RwLock};
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum InterventionError {
    #[error("Intervention timed out")]
    Timeout,
    #[error("Intervention cancelled")]
    Cancelled,
    #[error("Internal error: {0}")]
    Internal(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterventionRequest {
    pub node_name: String,
    pub prompt: String,
    pub context_summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserFeedback {
    pub correction: String,
    pub should_pivot: bool,
    pub pivot_reason: Option<String>,
    pub confidence: f32,
}

pub struct InterventionManager {
    pending: Arc<RwLock<HashMap<String, oneshot::Sender<UserFeedback>>>>,
}

impl InterventionManager {
    pub fn new() -> Self {
        Self {
            pending: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Requests feedback for a specific node and waits for the user's response.
    pub async fn request_feedback(
        &self, 
        node_name: String, 
        _prompt: String, 
        _ctx_summary: String,
        timeout: std::time::Duration
    ) -> Result<UserFeedback, InterventionError> {
        let (tx, rx) = oneshot::channel();
        
        {
            let mut pending = self.pending.write().await;
            pending.insert(node_name.clone(), tx);
        }

        tracing::info!(node = %node_name, "Execution paused: awaiting human intervention");

        let result = tokio::select! {
            res = rx => res.map_err(|_| InterventionError::Cancelled),
            _ = tokio::time::sleep(timeout) => Err(InterventionError::Timeout),
        };

        // Cleanup
        let mut pending = self.pending.write().await;
        pending.remove(&node_name);

        result
    }

    /// Pushes user feedback into the system, resuming the paused execution.
    pub async fn submit_feedback(&self, node_name: &str, feedback: UserFeedback) -> Result<(), InterventionError> {
        let mut pending = self.pending.write().await;
        if let Some(tx) = pending.remove(node_name) {
            tx.send(feedback).map_err(|e| InterventionError::Internal(format!("{:?}", e)))?;
            Ok(())
        } else {
            Err(InterventionError::Internal(format!("No pending intervention for node {}", node_name)))
        }
    }
}
