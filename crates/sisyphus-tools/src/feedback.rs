use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;
use std::time::{SystemTime, UNIX_EPOCH};
use crate::ToolResult;

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

pub struct FeedbackStore {
    base_path: PathBuf,
}

impl FeedbackStore {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            base_path: path.as_ref().to_path_buf(),
        }
    }

    pub fn default() -> Self {
        Self::new(".omo/feedback")
    }

    async fn get_file_path(&self, agent_id: &str) -> PathBuf {
        self.base_path.join(format!("{}.json", agent_id))
    }

    pub async fn save_feedback(&self, feedback: Feedback) -> ToolResult<()> {
        if !self.base_path.exists() {
            fs::create_dir_all(&self.base_path).await?;
        }

        let path = self.get_file_path(&feedback.agent_id).await;
        
        let mut feedbacks = self.load_all_for_agent(&feedback.agent_id).await?;
        feedbacks.push(feedback);
        
        let json = serde_json::to_string_pretty(&feedbacks)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            
        fs::write(path, json).await?;
        Ok(())
    }

    async fn load_all_for_agent(&self, agent_id: &str) -> ToolResult<Vec<Feedback>> {
        let path = self.get_file_path(agent_id).await;
        if !path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(path).await?;
        let feedbacks: Vec<Feedback> = serde_json::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            
        Ok(feedbacks)
    }

    pub async fn get_recent_feedback(&self, agent_id: &str, limit: usize) -> ToolResult<Vec<Feedback>> {
        let mut feedbacks = self.load_all_for_agent(agent_id).await?;
        
        // Sort by timestamp descending
        feedbacks.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        
        let result = feedbacks.into_iter().take(limit).collect();
        Ok(result)
    }

    pub async fn query_feedback(&self, query: &str, tags: Option<Vec<String>>) -> ToolResult<Vec<Feedback>> {
        if !self.base_path.exists() {
            return Ok(Vec::new());
        }

        let mut all_feedbacks = Vec::new();
        let mut entries = fs::read_dir(&self.base_path).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                let content = fs::read_to_string(&path).await?;
                let feedbacks: Vec<Feedback> = serde_json::from_str(&content)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                all_feedbacks.extend(feedbacks);
            }
        }

        let query_lower = query.to_lowercase();
        let filtered = all_feedbacks.into_iter().filter(|f| {
            let matches_query = f.original_request.to_lowercase().contains(&query_lower)
                || f.original_output.to_lowercase().contains(&query_lower)
                || f.corrected_output.to_lowercase().contains(&query_lower);
            
            let matches_tags = if let Some(ref required_tags) = tags {
                required_tags.iter().all(|t| f.tags.contains(t))
            } else {
                true
            };

            matches_query && matches_tags
        }).collect();

        Ok(filtered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // Use a counter for unique temp dirs to avoid test interference
    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn unique_temp_dir() -> PathBuf {
        let n = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("sisyphus_feedback_test_{}", n))
    }

    fn sample_feedback(agent_id: &str, request: &str, output: &str, correction: &str, tags: Vec<&str>) -> Feedback {
        Feedback {
            agent_id: agent_id.to_string(),
            original_request: request.to_string(),
            original_output: output.to_string(),
            corrected_output: correction.to_string(),
            timestamp: 1000,
            tags: tags.into_iter().map(|s| s.to_string()).collect(),
        }
    }

    #[tokio::test]
    async fn test_save_and_load_for_agent() {
        let dir = unique_temp_dir();
        let store = FeedbackStore::new(&dir);

        let feedback = sample_feedback(
            "agent_a",
            "List files in /tmp",
            "file1, file2",
            "Use ls -la /tmp for details",
            vec!["security"],
        );

        store.save_feedback(feedback).await.expect("save should succeed");

        let loaded = store.load_all_for_agent("agent_a").await.expect("load should succeed");
        assert_eq!(loaded.len(), 1, "should load exactly one feedback");
        assert_eq!(loaded[0].original_request, "List files in /tmp");
        assert!(loaded[0].tags.contains(&"security".to_string()));

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_load_nonexistent_agent_returns_empty() {
        let dir = unique_temp_dir();
        let store = FeedbackStore::new(&dir);

        let loaded = store.load_all_for_agent("ghost").await.expect("load should succeed");
        assert!(loaded.is_empty(), "nonexistent agent should return empty");
    }

    #[tokio::test]
    async fn test_get_recent_feedback_returns_latest() {
        let dir = unique_temp_dir();
        let store = FeedbackStore::new(&dir);

        // Save entries with increasing timestamps
        for i in 0..5 {
            let mut fb = sample_feedback("agent_b", &format!("request {}", i), "output", "correction", vec![]);
            fb.timestamp = i as i64 * 100;
            store.save_feedback(fb).await.expect("save should succeed");
        }

        let recent = store.get_recent_feedback("agent_b", 2).await.expect("get_recent should succeed");
        assert_eq!(recent.len(), 2, "should return only 2 entries");
        assert_eq!(recent[0].original_request, "request 4", "most recent should be first");
        assert_eq!(recent[1].original_request, "request 3", "second most recent should be second");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_query_feedback_by_topic() {
        let dir = unique_temp_dir();
        let store = FeedbackStore::new(&dir);

        store.save_feedback(sample_feedback("agent_c", "How to parse JSON", "use serde", "use serde with derive", vec!["rust"])).await.unwrap();
        store.save_feedback(sample_feedback("agent_c", "How to read a file", "use fs::read", "use fs::read_to_string", vec!["io"])).await.unwrap();
        store.save_feedback(sample_feedback("agent_d", "JSON serialization", "use serde_json", "use serde_json::to_string", vec!["rust", "json"])).await.unwrap();

        // Query for "json" — should match 2 entries (first and third)
        let results = store.query_feedback("json", None).await.expect("query should succeed");
        assert_eq!(results.len(), 2, "should find 2 entries matching 'json'");

        // Query for "json" with tag ["rust"] — should match only third
        let results = store.query_feedback("json", Some(vec!["rust".to_string()])).await.expect("query with tags should succeed");
        assert_eq!(results.len(), 2, "should find 2 entries matching 'json' with tag 'rust' (both entries 1 and 3 have 'rust' tag)");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_query_feedback_across_agents() {
        let dir = unique_temp_dir();
        let store = FeedbackStore::new(&dir);

        store.save_feedback(sample_feedback("agent_e", "Fix memory leak", "free memory", "use Arc", vec!["performance"])).await.unwrap();
        store.save_feedback(sample_feedback("agent_f", "Fix memory issue", "check heap", "use valgrind", vec!["debug"])).await.unwrap();
        store.save_feedback(sample_feedback("agent_g", "Add new feature", "implement module", "use module system", vec!["feature"])).await.unwrap();

        // Query for "memory" across all agents
        let results = store.query_feedback("memory", None).await.expect("cross-agent query should succeed");
        assert_eq!(results.len(), 2, "should find 2 entries across agents matching 'memory'");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_query_empty_store() {
        let dir = unique_temp_dir();
        let store = FeedbackStore::new(&dir);

        // Querying an empty store should return empty, not error
        let results = store.query_feedback("anything", None).await.expect("query on empty store should succeed");
        assert!(results.is_empty(), "empty store should return empty results");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
