//! 并行冲突检测（T3.4）。
//!
//! 同一批并行的任务里，两个任务写同一资源（write-write）会造成**静默覆盖**；
//! 一个任务读、另一个写同一资源（read-write）可能读到过期值。
//!
//! 本模块提供：
//! - [`detect_conflicts`]：列出冲突（供事件流与日志告警）；
//! - [`partition_by_write_conflicts`]：把写冲突的任务拆到不同执行波次，
//!   调度器按波次顺序执行，从而避免并发写同一文件。

use super::TaskNode;

/// 冲突类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKind {
    /// 两个任务写同一资源（可能静默覆盖）。
    WriteWrite,
    /// 一个任务读、另一个写同一资源（可能读到过期值）。
    ReadWrite,
}

impl ConflictKind {
    /// 稳定字符串标识（用于事件负载与日志）。
    pub fn as_str(&self) -> &'static str {
        match self {
            ConflictKind::WriteWrite => "write_write",
            ConflictKind::ReadWrite => "read_write",
        }
    }
}

/// 一个冲突。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// 冲突类型。
    pub kind: ConflictKind,
    /// 冲突的资源键（文件路径等）。
    pub key: String,
    /// 参与冲突的任务 id。
    pub task_ids: Vec<String>,
}

/// 任务声明的写键。
///
/// 来源有二：
/// - 显式 [`TaskNode::write_keys`] 字段（调度器写上下文时使用）；
/// - 兼容旧约定 `args["_write_keys"]`（与 `_delegation` 同风格的历史写法）。
pub fn write_keys_of(task: &TaskNode) -> Vec<String> {
    let mut keys: Vec<String> = task.write_keys.clone();
    if let Some(arr) = task.args.get("_write_keys").and_then(|v| v.as_array()) {
        for value in arr {
            if let Some(key) = value.as_str() {
                if !keys.iter().any(|existing| existing == key) {
                    keys.push(key.to_string());
                }
            }
        }
    }
    keys
}

/// 检测同批任务间的写写冲突与读写冲突。
pub fn detect_conflicts(tasks: &[&TaskNode]) -> Vec<Conflict> {
    let mut conflicts: Vec<Conflict> = Vec::new();

    for (i, a) in tasks.iter().enumerate() {
        for b in tasks.iter().skip(i + 1) {
            let a_writes = write_keys_of(a);
            let b_writes = write_keys_of(b);
            for key in &a_writes {
                if b_writes.contains(key) {
                    conflicts.push(Conflict {
                        kind: ConflictKind::WriteWrite,
                        key: key.clone(),
                        task_ids: vec![a.id.clone(), b.id.clone()],
                    });
                }
            }
            for key in &a.read_keys {
                if b_writes.contains(key) {
                    conflicts.push(Conflict {
                        kind: ConflictKind::ReadWrite,
                        key: key.clone(),
                        task_ids: vec![a.id.clone(), b.id.clone()],
                    });
                }
            }
            for key in &b.read_keys {
                if a_writes.contains(key) {
                    conflicts.push(Conflict {
                        kind: ConflictKind::ReadWrite,
                        key: key.clone(),
                        task_ids: vec![b.id.clone(), a.id.clone()],
                    });
                }
            }
        }
    }

    conflicts
}

/// 按写冲突把任务拆成执行波次。
///
/// 同一波次内的任务写集合互不相交，可安全并行；写冲突的任务被分到不同波次，
/// 由调度器顺序执行（串行化）。返回的波次顺序稳定（按入参顺序贪心分组）。
pub fn partition_by_write_conflicts<'a>(tasks: &[&'a TaskNode]) -> Vec<Vec<&'a TaskNode>> {
    let mut waves: Vec<Vec<&'a TaskNode>> = Vec::new();
    let mut wave_keys: Vec<Vec<String>> = Vec::new();

    for task in tasks {
        let mut placed = false;
        let task_writes = write_keys_of(task);
        for (idx, keys) in wave_keys.iter_mut().enumerate() {
            if !task_writes.iter().any(|key| keys.contains(key)) {
                keys.extend(task_writes.iter().cloned());
                waves[idx].push(*task);
                placed = true;
                break;
            }
        }
        if !placed {
            wave_keys.push(task_writes.clone());
            waves.push(vec![*task]);
        }
    }

    waves
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn task(id: &str, reads: &[&str], writes: &[&str]) -> TaskNode {
        TaskNode {
            id: id.to_string(),
            agent_id: "metis".to_string(),
            tool_id: "metis/analyse".to_string(),
            args: json!({}),
            depends_on: vec![],
            read_keys: reads.iter().map(|s| s.to_string()).collect(),
            write_keys: writes.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn detects_write_write_conflict() {
        let a = task("t1", &[], &["src/lib.rs"]);
        let b = task("t2", &[], &["src/lib.rs"]);
        let conflicts = detect_conflicts(&[&a, &b]);

        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].kind, ConflictKind::WriteWrite);
        assert_eq!(conflicts[0].key, "src/lib.rs");
        assert_eq!(
            conflicts[0].task_ids,
            vec!["t1".to_string(), "t2".to_string()]
        );
    }

    #[test]
    fn detects_read_write_conflict() {
        let reader = task("t1", &["src/lib.rs"], &[]);
        let writer = task("t2", &[], &["src/lib.rs"]);
        let conflicts = detect_conflicts(&[&reader, &writer]);

        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].kind, ConflictKind::ReadWrite);
    }

    #[test]
    fn no_conflict_for_disjoint_resources() {
        let a = task("t1", &["a.rs"], &["a.out"]);
        let b = task("t2", &["b.rs"], &["b.out"]);
        assert!(detect_conflicts(&[&a, &b]).is_empty());
    }

    #[test]
    fn write_conflicting_tasks_are_separated_into_waves() {
        let a = task("t1", &[], &["src/lib.rs"]);
        let b = task("t2", &[], &["src/lib.rs"]);
        let c = task("t3", &[], &["src/other.rs"]);

        let waves = partition_by_write_conflicts(&[&a, &b, &c]);

        assert_eq!(waves.len(), 2, "写同一文件的两个任务必须分到不同波次");
        assert_eq!(waves[0].len(), 2, "无冲突任务可同波并行");
        assert_eq!(waves[1].len(), 1);
        let first_wave_ids: Vec<&str> = waves[0].iter().map(|t| t.id.as_str()).collect();
        assert!(first_wave_ids.contains(&"t1") && first_wave_ids.contains(&"t3"));
        assert_eq!(waves[1][0].id, "t2");
    }

    #[test]
    fn tasks_without_write_keys_stay_in_one_wave() {
        let a = task("t1", &[], &[]);
        let b = task("t2", &[], &[]);
        let waves = partition_by_write_conflicts(&[&a, &b]);
        assert_eq!(waves.len(), 1);
        assert_eq!(waves[0].len(), 2);
    }
}
