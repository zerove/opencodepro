//! Shared types for the Feedback module.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs;

use crate::ToolResult;

/// Feedback entry for an agent's response correction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feedback {
    pub agent_id: String,
    pub original_request: String,
    pub original_output: String,
    pub corrected_output: String,
    pub timestamp: i64,
    pub tags: Vec<String>,
}

impl Feedback {
    /// New.
    pub fn new(
        agent_id: String,
        original_request: String,
        original_output: String,
        corrected_output: String,
        tags: Vec<String>,
    ) -> Self {
        Self {
            agent_id,
            original_request,
            original_output,
            corrected_output,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64,
            tags,
        }
    }
}

/// Persistent store for feedback entries, one JSON file per agent.
pub struct FeedbackStore {
    base_path: PathBuf,
}

impl FeedbackStore {
    /// Open (or create) a feedback store at `path`.
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            base_path: path.as_ref().to_path_buf(),
        }
    }

    /// Default dir.
    pub fn default_dir() -> Self {
        Self::new(".omo/feedback")
    }

    fn get_file_path(&self, agent_id: &str) -> PathBuf {
        self.base_path.join(format!("{agent_id}.json"))
    }

    /// Save feedback.
    pub async fn save_feedback(&self, feedback: Feedback) -> ToolResult<()> {
        if !self.base_path.exists() {
            fs::create_dir_all(&self.base_path).await?;
        }

        let path = self.get_file_path(&feedback.agent_id);

        let mut feedbacks = self.load_all_for_agent(&feedback.agent_id).await?;
        feedbacks.push(feedback);

        let json = serde_json::to_string_pretty(&feedbacks).map_err(std::io::Error::other)?;

        fs::write(path, json).await?;
        Ok(())
    }

    /// Load all for agent.
    pub async fn load_all_for_agent(&self, agent_id: &str) -> ToolResult<Vec<Feedback>> {
        let path = self.get_file_path(agent_id);
        if !path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(path).await?;
        let feedbacks: Vec<Feedback> =
            serde_json::from_str(&content).map_err(std::io::Error::other)?;

        Ok(feedbacks)
    }

    /// Get recent feedback.
    pub async fn get_recent_feedback(
        &self,
        agent_id: &str,
        limit: usize,
    ) -> ToolResult<Vec<Feedback>> {
        let mut feedbacks = self.load_all_for_agent(agent_id).await?;

        // Sort by timestamp descending
        feedbacks.sort_by_key(|a| std::cmp::Reverse(a.timestamp));

        let result = feedbacks.into_iter().take(limit).collect();
        Ok(result)
    }

    /// Query feedback.
    pub async fn query_feedback(
        &self,
        query: &str,
        tags: Option<Vec<String>>,
    ) -> ToolResult<Vec<Feedback>> {
        if !self.base_path.exists() {
            return Ok(Vec::new());
        }

        let mut all_feedbacks = Vec::new();
        let mut entries = fs::read_dir(&self.base_path).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                let content = fs::read_to_string(&path).await?;
                let feedbacks: Vec<Feedback> =
                    serde_json::from_str(&content).map_err(std::io::Error::other)?;
                all_feedbacks.extend(feedbacks);
            }
        }

        let query_lower = query.to_lowercase();
        let filtered = all_feedbacks
            .into_iter()
            .filter(|f| {
                let matches_query = f.original_request.to_lowercase().contains(&query_lower)
                    || f.original_output.to_lowercase().contains(&query_lower)
                    || f.corrected_output.to_lowercase().contains(&query_lower);

                let matches_tags = if let Some(ref required_tags) = tags {
                    required_tags.iter().all(|t| f.tags.contains(t))
                } else {
                    true
                };

                matches_query && matches_tags
            })
            .collect();

        Ok(filtered)
    }
}
