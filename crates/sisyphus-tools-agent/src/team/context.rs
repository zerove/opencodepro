//! Team context — shared mutable state across tasks in a team plan.
//!
//! Provides a thread-safe key-value store that tasks can read from and write to,
//! enabling inter-task communication and state accumulation across iterations.

use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Shared context accessible to all tasks in a team execution.
///
/// Tasks can declare `read_keys` and `write_keys` in their [`TaskNode`] to
/// advertise their data dependencies. The scheduler uses this for:
/// - Conflict detection (concurrent writes to same key)
/// - Optimization (task reordering based on data flow)
/// - Debugging (data lineage tracking)
#[derive(Debug, Clone)]
pub struct TeamContext {
    /// Key-value store for arbitrary JSON data.
    shared: Arc<RwLock<HashMap<String, Value>>>,
    /// Execution log of all completed tasks.
    execution_log: Arc<RwLock<Vec<crate::team::TaskResult>>>,
    /// Metadata about key access patterns.
    key_metadata: Arc<RwLock<HashMap<String, KeyMetadata>>>,
}

/// Metadata for a single key in the shared context.
#[derive(Debug, Clone)]
pub struct KeyMetadata {
    /// Task IDs that have written to this key.
    pub writers: Vec<String>,
    /// Task IDs that have read from this key.
    pub readers: Vec<String>,
    /// Last write timestamp (millis since epoch).
    pub last_written_ms: Option<u128>,
    /// Whether the key is considered "final" (no more writes expected).
    pub is_final: bool,
}

impl Default for TeamContext {
    fn default() -> Self {
        Self::new()
    }
}

impl TeamContext {
    /// Create a new empty team context.
    pub fn new() -> Self {
        Self {
            shared: Arc::new(RwLock::new(HashMap::new())),
            execution_log: Arc::new(RwLock::new(Vec::new())),
            key_metadata: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Read a value from the shared context.
    pub fn read(&self, key: &str) -> Option<Value> {
        let map = self
            .shared
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        map.get(key).cloned()
    }

    /// Write a value to the shared context.
    ///
    /// Records the writer in key metadata for lineage tracking.
    pub fn write(&self, key: String, value: Value, writer_task_id: &str) {
        let mut map = self
            .shared
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        map.insert(key.clone(), value);

        let mut meta = self
            .key_metadata
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = meta.entry(key).or_insert(KeyMetadata {
            writers: Vec::new(),
            readers: Vec::new(),
            last_written_ms: None,
            is_final: false,
        });
        entry.writers.push(writer_task_id.to_string());
        entry.last_written_ms = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
        );
    }

    /// Read a value and record the reader in metadata.
    pub fn read_tracked(&self, key: &str, reader_task_id: &str) -> Option<Value> {
        let value = self.read(key);
        if value.is_some() {
            let mut meta = self
                .key_metadata
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(entry) = meta.get_mut(key) {
                entry.readers.push(reader_task_id.to_string());
            }
        }
        value
    }

    /// Check if a key exists in the context.
    pub fn has(&self, key: &str) -> bool {
        let map = self
            .shared
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        map.contains_key(key)
    }

    /// Remove a key from the context.
    pub fn remove(&self, key: &str) -> Option<Value> {
        let mut map = self
            .shared
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        map.remove(key)
    }

    /// Get all keys currently in the context.
    pub fn keys(&self) -> Vec<String> {
        let map = self
            .shared
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        map.keys().cloned().collect()
    }

    /// Get the entire context as a JSON object (for debugging/synthesis).
    pub fn snapshot(&self) -> Value {
        let map = self
            .shared
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Value::Object(map.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
    }

    /// Record a task result in the execution log.
    pub fn record_task_result(&self, result: crate::team::TaskResult) {
        let mut log = self
            .execution_log
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        log.push(result);
    }

    /// Get the execution log (all completed task results).
    pub fn execution_log(&self) -> Vec<crate::team::TaskResult> {
        let log = self
            .execution_log
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        log.clone()
    }

    /// Get key metadata for debugging/analysis.
    pub fn key_metadata(&self) -> HashMap<String, KeyMetadata> {
        let meta = self
            .key_metadata
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        meta.clone()
    }

    /// Mark a key as final (no more writes expected).
    pub fn mark_final(&self, key: &str) {
        let mut meta = self
            .key_metadata
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(entry) = meta.get_mut(key) {
            entry.is_final = true;
        }
    }

    /// Check for write conflicts among a set of tasks.
    ///
    /// Returns a list of (key, [task_ids]) tuples where multiple tasks
    /// declare writes to the same key.
    pub fn detect_write_conflicts(
        &self,
        tasks: &[&crate::team::TaskNode],
    ) -> Vec<(String, Vec<String>)> {
        let mut key_writers: HashMap<String, Vec<String>> = HashMap::new();
        for task in tasks {
            // Tasks declare write keys via a convention in args
            // For now, we check the tool_id for known patterns
            // In the future, TaskNode will have explicit write_keys field
            if let Some(write_keys) = Self::extract_write_keys(task) {
                for key in write_keys {
                    key_writers.entry(key).or_default().push(task.id.clone());
                }
            }
        }
        key_writers
            .into_iter()
            .filter(|(_, writers)| writers.len() > 1)
            .collect()
    }

    /// Extract declared write keys from a task's args.
    /// Convention: args["_write_keys"] = ["key1", "key2", ...]
    fn extract_write_keys(task: &crate::team::TaskNode) -> Option<Vec<String>> {
        task.args
            .get("_write_keys")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_basic_read_write() {
        let ctx = TeamContext::new();
        ctx.write("key1".into(), json!("value1"), "task1");
        assert_eq!(ctx.read("key1"), Some(json!("value1")));
        assert_eq!(ctx.read("nonexistent"), None);
    }

    #[test]
    fn test_write_overwrite() {
        let ctx = TeamContext::new();
        ctx.write("key1".into(), json!("v1"), "task1");
        ctx.write("key1".into(), json!("v2"), "task2");
        assert_eq!(ctx.read("key1"), Some(json!("v2")));
    }

    #[test]
    fn test_tracked_read() {
        let ctx = TeamContext::new();
        ctx.write("key1".into(), json!("value1"), "task1");
        let _ = ctx.read_tracked("key1", "task2");
        let meta = ctx.key_metadata();
        assert!(meta
            .get("key1")
            .unwrap()
            .readers
            .contains(&"task2".to_string()));
    }

    #[test]
    fn test_snapshot() {
        let ctx = TeamContext::new();
        ctx.write("a".into(), json!(1), "task1");
        ctx.write("b".into(), json!(2), "task2");
        let snap = ctx.snapshot();
        assert_eq!(snap["a"], json!(1));
        assert_eq!(snap["b"], json!(2));
    }

    #[test]
    fn test_execution_log() {
        let ctx = TeamContext::new();
        let result = crate::team::TaskResult {
            task_id: "task1".into(),
            agent_id: "metis".into(),
            success: true,
            output: Some(json!({"result": "ok"})),
            error: None,
            duration_ms: 100,
        };
        ctx.record_task_result(result.clone());
        let log = ctx.execution_log();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].task_id, "task1");
    }

    #[test]
    fn test_mark_final() {
        let ctx = TeamContext::new();
        ctx.write("key1".into(), json!("value"), "task1");
        ctx.mark_final("key1");
        let meta = ctx.key_metadata();
        assert!(meta.get("key1").unwrap().is_final);
    }

    #[test]
    fn test_detect_write_conflicts() {
        let ctx = TeamContext::new();
        let task1 = crate::team::TaskNode {
            id: "task1".into(),
            agent_id: "metis".into(),
            tool_id: "metis/analyse".into(),
            args: json!({"_write_keys": ["spec", "plan"]}),
            depends_on: vec![],
            read_keys: vec![],
            write_keys: vec![],
        };
        let task2 = crate::team::TaskNode {
            id: "task2".into(),
            agent_id: "momus".into(),
            tool_id: "momus/review".into(),
            args: json!({"_write_keys": ["spec", "review"]}),
            depends_on: vec![],
            read_keys: vec![],
            write_keys: vec![],
        };
        let task3 = crate::team::TaskNode {
            id: "task3".into(),
            agent_id: "scribe".into(),
            tool_id: "scribe/generate".into(),
            args: json!({"_write_keys": ["doc"]}),
            depends_on: vec![],
            read_keys: vec![],
            write_keys: vec![],
        };
        let conflicts = ctx.detect_write_conflicts(&[&task1, &task2, &task3]);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].0, "spec");
        assert_eq!(conflicts[0].1.len(), 2);
    }
}
