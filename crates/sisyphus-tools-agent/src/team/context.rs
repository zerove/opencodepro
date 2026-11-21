//! Team context — shared mutable state across tasks in a team plan.
//!
//! Provides a thread-safe key-value store that tasks can read from and write to,
//! enabling inter-task communication and state accumulation across iterations.
//!
//! M1.2（最小闭环）：提供工作区变更事件记录与“变更 → 受影响任务”评估原语，
//! 供后续调度/重评估流程接线（当前不改变既有执行语义）。

use serde_json::Value;
use std::collections::{HashMap, HashSet};
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
    /// 工作区变更事件（按记录顺序追加，M1.2 最小闭环）。
    workspace_changes: Arc<RwLock<Vec<WorkspaceChange>>>,
    /// 工作区影响处理策略。
    impact_policy: Arc<RwLock<WorkspaceImpactPolicy>>,
    /// 被标记为受影响的任务 id（去重，保持首次出现顺序）。
    impacted_task_ids: Arc<RwLock<Vec<String>>>,
    /// 影响评估审计记录。
    impact_records: Arc<RwLock<Vec<WorkspaceImpactRecord>>>,
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

/// 工作区文件/资源变更类型（M1.2 协调原语）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceChangeKind {
    Created,
    Modified,
    Deleted,
    Renamed,
    Other(String),
}

/// 工作区变更事件（M1.2：用于“变更通知 → 受影响任务重评估”最小闭环）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceChange {
    /// 变更资源键（通常为文件路径；与任务读写键使用同一命名空间时可直接匹配）。
    pub path: String,
    /// 变更类型。
    pub kind: WorkspaceChangeKind,
    /// 可选来源标识（任务 id / agent / watcher 等）。
    #[serde(default)]
    pub source: Option<String>,
    /// 记录时间（毫秒，UNIX epoch）。
    #[serde(default)]
    pub timestamp_ms: u128,
}

impl WorkspaceChange {
    pub fn new(path: impl Into<String>, kind: WorkspaceChangeKind) -> Self {
        Self {
            path: path.into(),
            kind,
            source: None,
            timestamp_ms: now_ms(),
        }
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

/// 工作区变更影响的处理策略（M1.2 深化：可配置标记/观测/定向处理）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceImpactPolicy {
    /// 仅观测：发出事件/日志，不持久化受影响任务标记。
    #[default]
    Observe,
    /// 记录受影响任务：在观测基础上，将受影响的待执行任务 id 记录到上下文，供后续重试/重规划接线使用。
    RecordAffected,
    /// 记录并使受影响的待执行任务失败（定向处理）：写入审计记录并将受影响 pending 任务标记为失败，避免在已失效前提下继续执行。
    FailAffectedPending,
}

/// 一次工作区影响评估记录（可审计）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceImpactRecord {
    pub batch_index: usize,
    pub changed_paths: Vec<String>,
    pub affected_task_ids: Vec<String>,
    pub timestamp_ms: u128,
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
            workspace_changes: Arc::new(RwLock::new(Vec::new())),
            impact_policy: Arc::new(RwLock::new(WorkspaceImpactPolicy::default())),
            impacted_task_ids: Arc::new(RwLock::new(Vec::new())),
            impact_records: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// 设置工作区影响处理策略（M1.2 深化）。
    pub fn set_workspace_impact_policy(&self, policy: WorkspaceImpactPolicy) {
        let mut p = self.impact_policy.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        *p = policy;
    }

    /// 获取当前工作区影响处理策略。
    pub fn workspace_impact_policy(&self) -> WorkspaceImpactPolicy {
        *self.impact_policy.read().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// 记录受影响任务标记与审计记录（策略为 `RecordAffected` 时由调度层调用）。
    pub fn record_workspace_impact(
        &self,
        batch_index: usize,
        changed_paths: Vec<String>,
        affected_task_ids: Vec<String>,
    ) {
        if affected_task_ids.is_empty() {
            return;
        }
        {
            let mut ids =
                self.impacted_task_ids.write().unwrap_or_else(std::sync::PoisonError::into_inner);
            for id in &affected_task_ids {
                if !ids.iter().any(|x| x == id) {
                    ids.push(id.clone());
                }
            }
        }
        {
            let mut records =
                self.impact_records.write().unwrap_or_else(std::sync::PoisonError::into_inner);
            records.push(WorkspaceImpactRecord {
                batch_index,
                changed_paths,
                affected_task_ids,
                timestamp_ms: now_ms(),
            });
        }
    }

    /// 获取被标记为受影响的任务 id（去重，首次出现顺序）。
    pub fn impacted_task_ids(&self) -> Vec<String> {
        self.impacted_task_ids.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    /// 获取影响评估审计记录。
    pub fn workspace_impact_records(&self) -> Vec<WorkspaceImpactRecord> {
        self.impact_records.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    /// 清空受影响任务标记与影响审计记录（不清空原始 workspace_changes）。
    pub fn clear_workspace_impacts(&self) {
        self.impacted_task_ids.write().unwrap_or_else(std::sync::PoisonError::into_inner).clear();
        self.impact_records.write().unwrap_or_else(std::sync::PoisonError::into_inner).clear();
    }

    /// Read a value from the shared context.
    pub fn read(&self, key: &str) -> Option<Value> {
        let map = self.shared.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        map.get(key).cloned()
    }

    /// Write a value to the shared context.
    ///
    /// Records the writer in key metadata for lineage tracking.
    pub fn write(&self, key: String, value: Value, writer_task_id: &str) {
        let mut map = self.shared.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        map.insert(key.clone(), value);

        let mut meta = self.key_metadata.write().unwrap_or_else(std::sync::PoisonError::into_inner);
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
            let mut meta =
                self.key_metadata.write().unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(entry) = meta.get_mut(key) {
                entry.readers.push(reader_task_id.to_string());
            }
        }
        value
    }

    /// Check if a key exists in the context.
    pub fn has(&self, key: &str) -> bool {
        let map = self.shared.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        map.contains_key(key)
    }

    /// Remove a key from the context.
    pub fn remove(&self, key: &str) -> Option<Value> {
        let mut map = self.shared.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        map.remove(key)
    }

    /// Get all keys currently in the context.
    pub fn keys(&self) -> Vec<String> {
        let map = self.shared.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        map.keys().cloned().collect()
    }

    /// Get the entire context as a JSON object (for debugging/synthesis).
    pub fn snapshot(&self) -> Value {
        let map = self.shared.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        Value::Object(map.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
    }

    /// Record a task result in the execution log.
    pub fn record_task_result(&self, result: crate::team::TaskResult) {
        let mut log = self.execution_log.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        log.push(result);
    }

    /// Get the execution log (all completed task results).
    pub fn execution_log(&self) -> Vec<crate::team::TaskResult> {
        let log = self.execution_log.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        log.clone()
    }

    /// Get key metadata for debugging/analysis.
    pub fn key_metadata(&self) -> HashMap<String, KeyMetadata> {
        let meta = self.key_metadata.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        meta.clone()
    }

    /// Mark a key as final (no more writes expected).
    pub fn mark_final(&self, key: &str) {
        let mut meta = self.key_metadata.write().unwrap_or_else(std::sync::PoisonError::into_inner);
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
        // 统一走冲突检测模块（T3.4）：写键来源含显式 `write_keys` 字段
        // 与兼容旧约定 `args["_write_keys"]`。
        crate::team::conflict::detect_conflicts(tasks)
            .into_iter()
            .filter(|c| c.kind == crate::team::conflict::ConflictKind::WriteWrite)
            .map(|c| (c.key, c.task_ids))
            .collect()
    }

    /// 记录一条工作区变更事件（M1.2 最小闭环：变更通知）。
    pub fn record_workspace_change(&self, change: WorkspaceChange) {
        if change.path.trim().is_empty() {
            return;
        }
        let mut changes =
            self.workspace_changes.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        changes.push(change);
    }

    /// 获取已记录的工作区变更事件快照。
    pub fn workspace_changes(&self) -> Vec<WorkspaceChange> {
        let changes =
            self.workspace_changes.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        changes.clone()
    }

    /// 清空已记录的工作区变更事件（在完成一轮重评估后可调用）。
    pub fn clear_workspace_changes(&self) {
        let mut changes =
            self.workspace_changes.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        changes.clear();
    }

    /// 根据工作区变更评估受影响任务（M1.2 最小闭环：变更 → 受影响任务）。
    ///
    /// 判定规则（保守）：若任务的读键或写键（含 conflict 模块的启发式推断）
    /// 与任一变更 `path` 命中，则视为受影响，需重评估/复核。
    pub fn affected_tasks_by_changes(
        &self,
        tasks: &[&crate::team::TaskNode],
        changes: &[WorkspaceChange],
    ) -> Vec<String> {
        if changes.is_empty() || tasks.is_empty() {
            return Vec::new();
        }
        let changed: HashSet<&str> = changes.iter().map(|c| c.path.as_str()).collect();
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for task in tasks {
            let mut impacted = false;
            for key in crate::team::conflict::read_keys_of(task)
                .into_iter()
                .chain(crate::team::conflict::write_keys_of(task))
            {
                if changed.contains(key.as_str()) {
                    impacted = true;
                    break;
                }
            }
            if impacted && seen.insert(task.id.clone()) {
                out.push(task.id.clone());
            }
        }
        out
    }

    /// 基于当前已记录变更评估受影响任务（便捷方法）。
    pub fn tasks_affected_by_recorded_changes(
        &self,
        tasks: &[&crate::team::TaskNode],
    ) -> Vec<String> {
        let changes = self.workspace_changes();
        self.affected_tasks_by_changes(tasks, &changes)
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
        assert!(meta.get("key1").unwrap().readers.contains(&"task2".to_string()));
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

    #[test]
    fn test_workspace_change_record_and_clear() {
        let ctx = TeamContext::new();
        ctx.record_workspace_change(
            WorkspaceChange::new("src/lib.rs", WorkspaceChangeKind::Modified).with_source("task1"),
        );
        ctx.record_workspace_change(WorkspaceChange::new(
            "src/main.rs",
            WorkspaceChangeKind::Created,
        ));
        let changes = ctx.workspace_changes();
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].path, "src/lib.rs");
        assert_eq!(changes[0].source.as_deref(), Some("task1"));
        assert_eq!(changes[1].kind, WorkspaceChangeKind::Created);
        ctx.clear_workspace_changes();
        assert!(ctx.workspace_changes().is_empty());
    }

    #[test]
    fn test_affected_tasks_by_changes_uses_inferred_read_write_keys() {
        let ctx = TeamContext::new();
        let reader = crate::team::TaskNode {
            id: "reader".into(),
            agent_id: "coder".into(),
            tool_id: "builtin/read".into(),
            args: json!({"path": "src/lib.rs"}),
            depends_on: vec![],
            read_keys: vec![],
            write_keys: vec![],
        };
        let writer = crate::team::TaskNode {
            id: "writer".into(),
            agent_id: "coder".into(),
            tool_id: "builtin/write".into(),
            args: json!({"path": "src/lib.rs", "content": "x"}),
            depends_on: vec![],
            read_keys: vec![],
            write_keys: vec![],
        };
        let unrelated = crate::team::TaskNode {
            id: "other".into(),
            agent_id: "coder".into(),
            tool_id: "builtin/write".into(),
            args: json!({"path": "README.md", "content": "y"}),
            depends_on: vec![],
            read_keys: vec![],
            write_keys: vec![],
        };
        let changes = vec![WorkspaceChange::new(
            "src/lib.rs",
            WorkspaceChangeKind::Modified,
        )];
        let affected = ctx.affected_tasks_by_changes(&[&reader, &writer, &unrelated], &changes);
        assert_eq!(affected.len(), 2);
        assert!(affected.contains(&"reader".to_string()));
        assert!(affected.contains(&"writer".to_string()));
        assert!(!affected.contains(&"other".to_string()));
    }

    #[test]
    fn test_tasks_affected_by_recorded_changes_convenience() {
        let ctx = TeamContext::new();
        let task = crate::team::TaskNode {
            id: "t1".into(),
            agent_id: "coder".into(),
            tool_id: "builtin/edit".into(),
            args: json!({"file_path": "src/app.rs"}),
            depends_on: vec![],
            read_keys: vec![],
            write_keys: vec![],
        };
        ctx.record_workspace_change(WorkspaceChange::new(
            "src/app.rs",
            WorkspaceChangeKind::Modified,
        ));
        let affected = ctx.tasks_affected_by_recorded_changes(&[&task]);
        assert_eq!(affected, vec!["t1".to_string()]);
    }

    #[test]
    fn test_workspace_impact_policy_and_recording() {
        let ctx = TeamContext::new();
        assert_eq!(
            ctx.workspace_impact_policy(),
            WorkspaceImpactPolicy::Observe
        );
        ctx.set_workspace_impact_policy(WorkspaceImpactPolicy::RecordAffected);
        assert_eq!(
            ctx.workspace_impact_policy(),
            WorkspaceImpactPolicy::RecordAffected
        );

        ctx.record_workspace_impact(
            0,
            vec!["src/lib.rs".to_string()],
            vec!["t3".to_string(), "t4".to_string()],
        );
        // duplicate ids should not be recorded twice
        ctx.record_workspace_impact(1, vec!["src/lib.rs".to_string()], vec!["t3".to_string()]);

        assert_eq!(
            ctx.impacted_task_ids(),
            vec!["t3".to_string(), "t4".to_string()]
        );
        let records = ctx.workspace_impact_records();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].batch_index, 0);
        assert_eq!(records[0].affected_task_ids, vec!["t3", "t4"]);
        assert_eq!(records[1].batch_index, 1);

        ctx.clear_workspace_impacts();
        assert!(ctx.impacted_task_ids().is_empty());
        assert!(ctx.workspace_impact_records().is_empty());
    }
}
