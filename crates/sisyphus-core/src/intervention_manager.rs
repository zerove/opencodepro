//! Sisyphus 的人在回路（human-in-the-loop）干预管理。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::{oneshot, RwLock};

#[derive(Error, Debug)]
/// Intervention Error.
pub enum InterventionError {
    #[error("Intervention timed out")]
    Timeout,
    #[error("Intervention cancelled")]
    Cancelled,
    #[error("Internal error: {0}")]
    Internal(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Intervention Request.
pub struct InterventionRequest {
    pub node_name: String,
    pub prompt: String,
    pub context_summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// User Feedback.
pub struct UserFeedback {
    pub correction: String,
    pub should_pivot: bool,
    pub pivot_reason: Option<String>,
    pub confidence: f32,
}

/// Intervention Manager.
pub struct InterventionManager {
    pending: Arc<RwLock<HashMap<String, oneshot::Sender<UserFeedback>>>>,
}

impl Default for InterventionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl InterventionManager {
    /// New.
    pub fn new() -> Self {
        Self {
            pending: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 请求某个节点的反馈并等待用户响应。
    pub async fn request_feedback(
        &self,
        node_name: String,
        _prompt: String,
        _ctx_summary: String,
        timeout: std::time::Duration,
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

        // 清理
        let mut pending = self.pending.write().await;
        pending.remove(&node_name);

        result
    }

    /// 将用户反馈推入系统，恢复暂停的执行。
    pub async fn submit_feedback(
        &self,
        node_name: &str,
        feedback: UserFeedback,
    ) -> Result<(), InterventionError> {
        let mut pending = self.pending.write().await;
        if let Some(tx) = pending.remove(node_name) {
            tx.send(feedback)
                .map_err(|e| InterventionError::Internal(format!("{:?}", e)))?;
            Ok(())
        } else {
            Err(InterventionError::Internal(format!(
                "No pending intervention for node {}",
                node_name
            )))
        }
    }
}
