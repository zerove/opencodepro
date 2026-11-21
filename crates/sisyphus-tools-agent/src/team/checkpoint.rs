//! 团队执行检查点（M0.3/S5 最小可测中断恢复切片）。
//!
//! 提供可序列化的执行状态快照，用于中断后的持久化与恢复前置。
//! 当前实现覆盖：计划/已完成结果/冲突统计/工作区影响审计/上下文快照的保存与加载；
//! 调度器可基于检查点恢复继续执行剩余任务（最小恢复路径）。

use super::{
    ConflictStats, TaskResult, TeamContext, TeamPlan, WorkspaceChange, WorkspaceImpactRecord,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

/// 可序列化的上下文快照（用于恢复）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TeamContextSnapshot {
    /// 共享键值状态快照。
    #[serde(default)]
    pub shared: HashMap<String, Value>,
    /// 工作区变更事件。
    #[serde(default)]
    pub workspace_changes: Vec<WorkspaceChange>,
    /// 影响处理策略（恢复后可继续沿用）。
    #[serde(default)]
    pub impact_policy: super::context::WorkspaceImpactPolicy,
    /// 已标记受影响任务 id（去重顺序）。
    #[serde(default)]
    pub impacted_task_ids: Vec<String>,
    /// 影响评估审计记录。
    #[serde(default)]
    pub impact_records: Vec<WorkspaceImpactRecord>,
}

/// 团队执行检查点（可持久化）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamCheckpoint {
    /// 检查点格式版本。
    pub version: u32,
    /// 关联目标/计划标识。
    pub goal_id: String,
    /// 执行计划。
    pub plan: TeamPlan,
    /// 已完成任务结果（含成功/失败/被策略终止等终态）。
    #[serde(default)]
    pub completed_results: Vec<TaskResult>,
    /// 聚合冲突统计（截至检查点）。
    #[serde(default)]
    pub conflict_stats: ConflictStats,
    /// 上下文快照。
    #[serde(default)]
    pub context: TeamContextSnapshot,
    /// 已消费的工作区变更数量（用于恢复后继续增量评估）。
    #[serde(default)]
    pub processed_change_count: usize,
    /// 创建时间（毫秒，UNIX epoch）。
    #[serde(default)]
    pub created_at_ms: u128,
}

/// 调度恢复所需的最小状态。
#[derive(Debug, Clone, Default)]
pub struct TeamResumeState {
    pub completed_results: Vec<TaskResult>,
    pub conflict_stats: ConflictStats,
    pub processed_change_count: usize,
}

impl TeamResumeState {
    /// 返回排除“因工作区影响而失败”的任务结果后的恢复状态，便于定向重跑（M1.2/M2.1）。
    pub fn without_workspace_impact_failures(mut self) -> Self {
        self.completed_results.retain(|r| !r.is_workspace_impact_failure());
        self
    }

    /// 返回排除所有失败任务结果后的恢复状态（便于重试失败任务）。
    pub fn without_failed_tasks(mut self) -> Self {
        self.completed_results.retain(|r| r.success);
        self
    }
}

impl TeamCheckpoint {
    pub const VERSION: u32 = 1;

    /// 从当前执行状态构建检查点。
    pub fn from_state(
        goal_id: impl Into<String>,
        plan: TeamPlan,
        completed_results: Vec<TaskResult>,
        conflict_stats: ConflictStats,
        context: &TeamContext,
        processed_change_count: usize,
    ) -> Self {
        Self {
            version: Self::VERSION,
            goal_id: goal_id.into(),
            plan,
            completed_results,
            conflict_stats,
            context: TeamContextSnapshot {
                shared: context
                    .snapshot()
                    .as_object()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .collect(),
                workspace_changes: context.workspace_changes(),
                impact_policy: context.workspace_impact_policy(),
                impacted_task_ids: context.impacted_task_ids(),
                impact_records: context.workspace_impact_records(),
            },
            processed_change_count,
            created_at_ms: now_ms(),
        }
    }

    /// 从执行报告与上下文构建检查点（便于 S5 持久化/恢复触发）。
    pub fn from_report(plan: TeamPlan, context: &TeamContext, report: &super::TeamReport) -> Self {
        Self::from_state(
            report.goal_id.clone(),
            plan,
            report.task_results.clone(),
            report.conflict_stats.clone(),
            context,
            report.processed_change_count,
        )
    }

    /// 计算剩余（未完成）任务 id。
    pub fn remaining_task_ids(&self) -> Vec<String> {
        let done: std::collections::HashSet<&str> =
            self.completed_results.iter().map(|r| r.task_id.as_str()).collect();
        self.plan
            .tasks
            .iter()
            .filter(|t| !done.contains(t.id.as_str()))
            .map(|t| t.id.clone())
            .collect()
    }

    /// 恢复为可注入调度器的运行上下文。
    pub fn restore_context(&self) -> TeamContext {
        let ctx = TeamContext::new();
        for (k, v) in &self.context.shared {
            ctx.write(k.clone(), v.clone(), "checkpoint_restore");
        }
        ctx.set_workspace_impact_policy(self.context.impact_policy);
        for c in &self.context.workspace_changes {
            ctx.record_workspace_change(c.clone());
        }
        if !self.context.impact_records.is_empty() {
            // 重建影响标记/审计记录（保持可审计性；策略设回后记录仅作恢复存储）
            ctx.set_workspace_impact_policy(super::context::WorkspaceImpactPolicy::RecordAffected);
            for rec in &self.context.impact_records {
                ctx.record_workspace_impact(
                    rec.batch_index,
                    rec.changed_paths.clone(),
                    rec.affected_task_ids.clone(),
                );
            }
            ctx.set_workspace_impact_policy(self.context.impact_policy);
        } else if !self.context.impacted_task_ids.is_empty() {
            ctx.set_workspace_impact_policy(super::context::WorkspaceImpactPolicy::RecordAffected);
            ctx.record_workspace_impact(0, vec![], self.context.impacted_task_ids.clone());
            ctx.set_workspace_impact_policy(self.context.impact_policy);
        }
        ctx
    }

    /// 转换为调度恢复状态。
    pub fn to_resume_state(&self) -> TeamResumeState {
        TeamResumeState {
            completed_results: self.completed_results.clone(),
            conflict_stats: self.conflict_stats.clone(),
            processed_change_count: self.processed_change_count,
        }
    }

    /// 保存到 JSON 文件。
    pub fn save_to_file(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, bytes)
    }

    /// 从 JSON 文件加载。
    pub fn load_from_file(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let bytes = std::fs::read(path)?;
        serde_json::from_slice(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::team::{TaskNode, TaskResult};
    use serde_json::json;

    fn sample_plan() -> TeamPlan {
        TeamPlan {
            goal_id: "g1".into(),
            tasks: vec![
                TaskNode {
                    id: "t1".into(),
                    agent_id: "metis".into(),
                    tool_id: "metis/analyse".into(),
                    args: json!({}),
                    depends_on: vec![],
                    read_keys: vec![],
                    write_keys: vec!["a".into()],
                },
                TaskNode {
                    id: "t2".into(),
                    agent_id: "metis".into(),
                    tool_id: "metis/analyse".into(),
                    args: json!({}),
                    depends_on: vec!["t1".into()],
                    read_keys: vec!["a".into()],
                    write_keys: vec![],
                },
            ],
        }
    }

    #[test]
    fn checkpoint_round_trip_preserves_state() {
        let ctx = TeamContext::new();
        ctx.write("k".into(), json!(1), "t1");
        ctx.record_workspace_change(WorkspaceChange::new(
            "src/a.rs",
            super::super::WorkspaceChangeKind::Modified,
        ));
        ctx.set_workspace_impact_policy(
            super::super::context::WorkspaceImpactPolicy::RecordAffected,
        );
        ctx.record_workspace_impact(0, vec!["src/a.rs".into()], vec!["t2".into()]);

        let completed = vec![TaskResult {
            task_id: "t1".into(),
            agent_id: "metis".into(),
            success: true,
            output: Some(json!({"ok": true})),
            error: None,
            duration_ms: 10,
        }];
        let stats = ConflictStats {
            detected: 1,
            ..Default::default()
        };

        let ckpt = TeamCheckpoint::from_state(
            "g1",
            sample_plan(),
            completed.clone(),
            stats.clone(),
            &ctx,
            1,
        );
        assert_eq!(ckpt.version, TeamCheckpoint::VERSION);
        assert_eq!(ckpt.remaining_task_ids(), vec!["t2".to_string()]);

        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("ckpt.json");
        ckpt.save_to_file(&path).expect("save");
        let loaded = TeamCheckpoint::load_from_file(&path).expect("load");
        assert_eq!(loaded.goal_id, "g1");
        assert_eq!(loaded.completed_results.len(), 1);
        assert_eq!(loaded.remaining_task_ids(), vec!["t2".to_string()]);
        assert_eq!(loaded.conflict_stats.detected, 1);
        assert_eq!(loaded.context.impacted_task_ids, vec!["t2".to_string()]);
        assert_eq!(loaded.context.workspace_changes.len(), 1);

        let restored = loaded.restore_context();
        assert_eq!(restored.read("k"), Some(json!(1)));
        assert_eq!(restored.impacted_task_ids(), vec!["t2".to_string()]);
        assert_eq!(restored.workspace_changes().len(), 1);
    }

    #[test]
    fn resume_state_filters_workspace_impact_failures() {
        let resume = TeamResumeState {
            completed_results: vec![
                TaskResult {
                    task_id: "t1".into(),
                    agent_id: "a".into(),
                    success: true,
                    output: None,
                    error: None,
                    duration_ms: 1,
                },
                TaskResult {
                    task_id: "t2".into(),
                    agent_id: "a".into(),
                    success: false,
                    output: None,
                    error: Some(
                        "workspace impact: pending task affected by changes [\"x\"] in batch 0"
                            .into(),
                    ),
                    duration_ms: 1,
                },
                TaskResult {
                    task_id: "t3".into(),
                    agent_id: "a".into(),
                    success: false,
                    output: None,
                    error: Some("other failure".into()),
                    duration_ms: 1,
                },
            ],
            conflict_stats: ConflictStats::default(),
            processed_change_count: 0,
        };
        let filtered = resume.without_workspace_impact_failures();
        let ids: Vec<_> = filtered.completed_results.iter().map(|r| r.task_id.as_str()).collect();
        assert_eq!(ids, vec!["t1", "t3"]);
    }
}
