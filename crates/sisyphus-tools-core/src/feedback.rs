//! Feedback collection and querying for agent response correction.
//!
//! ## Sub-modules
//! - [`types`]: Feedback and FeedbackStore.

pub mod types;

pub use types::{Feedback, FeedbackStore};

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn unique_temp_dir() -> PathBuf {
        let n = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("sisyphus_feedback_test_{}", n))
    }

    fn sample_feedback(
        agent_id: &str,
        request: &str,
        output: &str,
        correction: &str,
        tags: Vec<&str>,
    ) -> Feedback {
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

        store
            .save_feedback(feedback)
            .await
            .expect("save should succeed");

        let loaded = store
            .load_all_for_agent("agent_a")
            .await
            .expect("load should succeed");
        assert_eq!(loaded.len(), 1, "should load exactly one feedback");
        assert_eq!(loaded[0].original_request, "List files in /tmp");
        assert!(loaded[0].tags.contains(&"security".to_string()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_load_nonexistent_agent_returns_empty() {
        let dir = unique_temp_dir();
        let store = FeedbackStore::new(&dir);

        let loaded = store
            .load_all_for_agent("ghost")
            .await
            .expect("load should succeed");
        assert!(loaded.is_empty(), "nonexistent agent should return empty");
    }

    #[tokio::test]
    async fn test_get_recent_feedback_returns_latest() {
        let dir = unique_temp_dir();
        let store = FeedbackStore::new(&dir);

        for i in 0..5 {
            let mut fb = sample_feedback(
                "agent_b",
                &format!("request {}", i),
                "output",
                "correction",
                vec![],
            );
            fb.timestamp = i as i64 * 100;
            store.save_feedback(fb).await.expect("save should succeed");
        }

        let recent = store
            .get_recent_feedback("agent_b", 2)
            .await
            .expect("get_recent should succeed");
        assert_eq!(recent.len(), 2, "should return only 2 entries");
        assert_eq!(
            recent[0].original_request, "request 4",
            "most recent should be first"
        );
        assert_eq!(
            recent[1].original_request, "request 3",
            "second most recent should be second"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_query_feedback_by_topic() {
        let dir = unique_temp_dir();
        let store = FeedbackStore::new(&dir);

        store
            .save_feedback(sample_feedback(
                "agent_c",
                "How to parse JSON",
                "use serde",
                "use serde with derive",
                vec!["rust"],
            ))
            .await
            .unwrap();
        store
            .save_feedback(sample_feedback(
                "agent_c",
                "How to read a file",
                "use fs::read",
                "use fs::read_to_string",
                vec!["io"],
            ))
            .await
            .unwrap();
        store
            .save_feedback(sample_feedback(
                "agent_d",
                "JSON serialization",
                "use serde_json",
                "use serde_json::to_string",
                vec!["rust", "json"],
            ))
            .await
            .unwrap();

        let results = store
            .query_feedback("json", None)
            .await
            .expect("query should succeed");
        assert_eq!(results.len(), 2, "should find 2 entries matching 'json'");

        let results = store
            .query_feedback("json", Some(vec!["rust".to_string()]))
            .await
            .expect("query with tags should succeed");
        assert_eq!(results.len(), 2, "should find 2 entries matching 'json' with tag 'rust' (both entries 1 and 3 have 'rust' tag)");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_query_feedback_across_agents() {
        let dir = unique_temp_dir();
        let store = FeedbackStore::new(&dir);

        store
            .save_feedback(sample_feedback(
                "agent_e",
                "Fix memory leak",
                "free memory",
                "use Arc",
                vec!["performance"],
            ))
            .await
            .unwrap();
        store
            .save_feedback(sample_feedback(
                "agent_f",
                "Fix memory issue",
                "check heap",
                "use valgrind",
                vec!["debug"],
            ))
            .await
            .unwrap();
        store
            .save_feedback(sample_feedback(
                "agent_g",
                "Add new feature",
                "implement module",
                "use module system",
                vec!["feature"],
            ))
            .await
            .unwrap();

        let results = store
            .query_feedback("memory", None)
            .await
            .expect("cross-agent query should succeed");
        assert_eq!(
            results.len(),
            2,
            "should find 2 entries across agents matching 'memory'"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_query_empty_store() {
        let dir = unique_temp_dir();
        let store = FeedbackStore::new(&dir);

        let results = store
            .query_feedback("anything", None)
            .await
            .expect("query on empty store should succeed");
        assert!(
            results.is_empty(),
            "empty store should return empty results"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_feedback_new_sets_reasonable_timestamp() {
        let feedback = Feedback::new(
            "agent_h".to_string(),
            "test".to_string(),
            "out".to_string(),
            "corr".to_string(),
            vec![],
        );
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        assert!(feedback.timestamp > 0, "timestamp should be set");
        assert!(
            (feedback.timestamp - now).abs() < 5,
            "timestamp should be close to current time"
        );
    }

    #[tokio::test]
    async fn test_save_creates_directory_automatically() {
        let dir = unique_temp_dir();
        assert!(!dir.exists(), "test dir should not exist yet");
        let store = FeedbackStore::new(&dir);

        let feedback = sample_feedback("agent_i", "request", "output", "correction", vec!["test"]);
        store
            .save_feedback(feedback)
            .await
            .expect("save should auto-create directory");

        assert!(dir.exists(), "directory should have been created");
        assert!(dir.join("agent_i.json").exists(), "agent file should exist");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_save_appends_multiple_entries() {
        let dir = unique_temp_dir();
        let store = FeedbackStore::new(&dir);

        for i in 0..3 {
            let fb = sample_feedback("agent_j", &format!("req_{}", i), "out", "corr", vec![]);
            store.save_feedback(fb).await.expect("save should succeed");
        }

        let all = store
            .load_all_for_agent("agent_j")
            .await
            .expect("load should succeed");
        assert_eq!(all.len(), 3, "should have accumulated 3 entries");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_get_recent_returns_all_when_limit_exceeds_count() {
        let dir = unique_temp_dir();
        let store = FeedbackStore::new(&dir);

        for i in 0..3 {
            let mut fb = sample_feedback("agent_k", &format!("req_{}", i), "out", "corr", vec![]);
            fb.timestamp = i as i64;
            store.save_feedback(fb).await.expect("save should succeed");
        }

        let recent = store
            .get_recent_feedback("agent_k", 99)
            .await
            .expect("get_recent should succeed");
        assert_eq!(
            recent.len(),
            3,
            "should return all entries when limit > count"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_get_recent_for_nonexistent_agent_returns_empty() {
        let dir = unique_temp_dir();
        let store = FeedbackStore::new(&dir);

        let recent = store
            .get_recent_feedback("ghost_agent", 5)
            .await
            .expect("get_recent should succeed");
        assert!(recent.is_empty(), "nonexistent agent should return empty");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_query_no_matches() {
        let dir = unique_temp_dir();
        let store = FeedbackStore::new(&dir);

        store
            .save_feedback(sample_feedback(
                "agent_l",
                "alpha",
                "beta",
                "gamma",
                vec!["x"],
            ))
            .await
            .unwrap();
        store
            .save_feedback(sample_feedback(
                "agent_l",
                "delta",
                "epsilon",
                "zeta",
                vec!["y"],
            ))
            .await
            .unwrap();

        let results = store
            .query_feedback("ZZZZ_ZZZZ", None)
            .await
            .expect("query should succeed");
        assert!(results.is_empty(), "no matches should return empty");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_query_with_tags_no_match() {
        let dir = unique_temp_dir();
        let store = FeedbackStore::new(&dir);

        store
            .save_feedback(sample_feedback(
                "agent_m",
                "alpha",
                "beta",
                "gamma",
                vec!["rust"],
            ))
            .await
            .unwrap();

        let results = store
            .query_feedback("alpha", Some(vec!["python".to_string()]))
            .await
            .expect("query should succeed");
        assert!(results.is_empty(), "tag mismatch should return empty");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_query_when_base_path_not_exists() {
        let dir = unique_temp_dir();
        assert!(!dir.exists(), "dir should not exist");
        let store = FeedbackStore::new(&dir);

        let results = store
            .query_feedback("anything", None)
            .await
            .expect("query on nonexistent path should succeed");
        assert!(results.is_empty(), "should return empty without error");
        assert!(!dir.exists(), "query should not create the directory");
    }

    #[tokio::test]
    async fn test_feedback_with_empty_tags() {
        let fb = Feedback::new(
            "agent_n".to_string(),
            "req".to_string(),
            "out".to_string(),
            "corr".to_string(),
            vec![],
        );
        assert!(
            fb.tags.is_empty(),
            "empty tags list should be stored as empty"
        );
    }

    #[tokio::test]
    async fn test_save_and_query_multiword_query() {
        let dir = unique_temp_dir();
        let store = FeedbackStore::new(&dir);

        store
            .save_feedback(sample_feedback(
                "agent_o",
                "How to parse JSON in Rust",
                "use serde_json",
                "use serde_json with derive",
                vec!["rust", "json"],
            ))
            .await
            .unwrap();
        store
            .save_feedback(sample_feedback(
                "agent_o",
                "How to read a file",
                "use fs::read_to_string",
                "use fs::read",
                vec!["io"],
            ))
            .await
            .unwrap();

        let results = store
            .query_feedback("JSON", None)
            .await
            .expect("query should succeed");
        assert_eq!(results.len(), 1, "should find 1 entry matching 'JSON'");

        let results = store
            .query_feedback("json", None)
            .await
            .expect("query should succeed");
        assert_eq!(results.len(), 1, "should be case-insensitive");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
