//! 并行冲突检测（T3.4 / M1.1 编码感知增强）。
//!
//! 同一批并行的任务里，两个任务写同一资源（write-write）会造成**静默覆盖**；
//! 一个任务读、另一个写同一资源（read-write）可能读到过期值。
//!
//! 本模块提供：
//! - [`detect_conflicts`]：列出冲突（供事件流与日志告警）；
//! - [`partition_by_write_conflicts`]：把写冲突的任务拆到不同执行波次，
//!   调度器按波次顺序执行，从而避免并发写同一文件。
//! - [`write_keys_of`]：合并显式声明与（编码场景）启发式推断的写键，
//!   降低 planner 未声明 `write_keys` 时的同文件静默覆盖风险。

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

/// 任务声明/推断的写键。
///
/// 来源（按优先顺序合并、去重）：
/// - 显式 [`TaskNode::write_keys`] 字段（调度器写上下文时使用）；
/// - 兼容旧约定 `args["_write_keys"]`（与 `_delegation` 同风格的历史写法）；
/// - **编码感知启发式推断**：当 `tool_id`/操作语义表明可能写入文件时，
///   从常见路径参数（`path`/`file`/`files`/`target` 等）提取候选键。
///
/// 推断是保守增量的：宁可多串行化一次，也不静默并发覆盖同一文件。
pub fn write_keys_of(task: &TaskNode) -> Vec<String> {
    let mut keys: Vec<String> = task.write_keys.clone();

    // 兼容旧约定 args["_write_keys"]
    if let Some(arr) = task.args.get("_write_keys").and_then(|v| v.as_array()) {
        for value in arr {
            if let Some(key) = value.as_str() {
                push_unique(&mut keys, key);
            }
        }
    }

    // M1.1：编码场景写键启发式推断（与显式键合并）
    for key in infer_write_keys(task) {
        push_unique(&mut keys, &key);
    }

    keys
}

fn push_unique(keys: &mut Vec<String>, key: &str) {
    let key = key.trim();
    if key.is_empty() {
        return;
    }
    if !keys.iter().any(|existing| existing == key) {
        keys.push(key.to_string());
    }
}

/// 从任务参数中启发式推断可能被写入的资源键（主要用于编码/文件操作场景）。
///
/// 仅在存在较强写意图信号时启用（避免把纯读取路径误判为写冲突）。
fn infer_write_keys(task: &TaskNode) -> Vec<String> {
    if !looks_like_write_operation(task) {
        return Vec::new();
    }

    let mut out = Vec::new();

    // 单值路径键（写目标常见命名）
    for key in [
        "path",
        "file",
        "file_path",
        "filepath",
        "target",
        "target_path",
        "output",
        "output_path",
        "dest",
        "destination",
        "new_path",
        "to",
        "save_path",
        "write_path",
    ] {
        if let Some(v) = task.args.get(key) {
            collect_string_values(v, &mut out);
        }
    }

    // 多值路径键（批量写/编辑）
    for key in ["paths", "files", "targets", "outputs", "edits"] {
        if let Some(v) = task.args.get(key) {
            collect_path_like_from_value(v, &mut out);
        }
    }

    // apply_patch / patch 类参数中可能嵌入文件路径（尽力提取常见字段）
    for key in ["patch", "patches", "changes", "diff", "operations"] {
        if let Some(v) = task.args.get(key) {
            collect_path_like_from_value(v, &mut out);
        }
    }

    out
}

fn looks_like_write_operation(task: &TaskNode) -> bool {
    let tool = task.tool_id.to_ascii_lowercase();
    const WRITE_HINTS: &[&str] = &[
        "write", "edit", "patch", "apply", "create", "delete", "remove", "rename", "move", "save",
        "update", "mkdir", "touch", "append", "replace",
    ];
    if WRITE_HINTS.iter().any(|h| tool.contains(h)) {
        return true;
    }

    // 兼容显式操作字段（部分工具/规划器使用）
    for key in ["action", "operation", "op", "mode", "command"] {
        if let Some(v) = task.args.get(key).and_then(|v| v.as_str()) {
            let v = v.to_ascii_lowercase();
            if WRITE_HINTS.iter().any(|h| v.contains(h)) {
                return true;
            }
        }
    }

    false
}

/// 从任务参数中启发式推断可能被读取的资源键（编码场景读写冲突辅助）。
fn infer_read_keys(task: &TaskNode) -> Vec<String> {
    if !looks_like_read_operation(task) {
        return Vec::new();
    }

    let mut out = Vec::new();
    for key in [
        "path",
        "file",
        "file_path",
        "filepath",
        "target",
        "source",
        "src",
    ] {
        if let Some(v) = task.args.get(key) {
            collect_string_values(v, &mut out);
        }
    }
    for key in ["paths", "files", "sources", "targets", "inputs"] {
        if let Some(v) = task.args.get(key) {
            collect_path_like_from_value(v, &mut out);
        }
    }
    out
}

fn looks_like_read_operation(task: &TaskNode) -> bool {
    let tool = task.tool_id.to_ascii_lowercase();
    const READ_HINTS: &[&str] = &[
        "read", "cat", "view", "open", "search", "grep", "glob", "find", "list", "ls", "stat",
        "inspect", "analy", "diff", "show", "get", "fetch", "query",
    ];
    if READ_HINTS.iter().any(|h| tool.contains(h)) {
        return true;
    }
    for key in ["action", "operation", "op", "mode", "command"] {
        if let Some(v) = task.args.get(key).and_then(|v| v.as_str()) {
            let v = v.to_ascii_lowercase();
            if READ_HINTS.iter().any(|h| v.contains(h)) {
                return true;
            }
        }
    }
    false
}

fn collect_string_values(v: &serde_json::Value, out: &mut Vec<String>) {
    match v {
        serde_json::Value::String(s) => push_unique(out, s),
        serde_json::Value::Array(arr) => {
            for item in arr {
                collect_string_values(item, out);
            }
        },
        _ => {},
    }
}

/// 从结构化参数中尽力提取路径样字段（用于 patches/edits 等嵌套对象）。
fn collect_path_like_from_value(v: &serde_json::Value, out: &mut Vec<String>) {
    match v {
        serde_json::Value::String(s) => {
            // 仅在看起来像路径时收集，避免把任意文本当作资源键
            if looks_like_path(s) {
                push_unique(out, s);
            }
        },
        serde_json::Value::Array(arr) => {
            for item in arr {
                collect_path_like_from_value(item, out);
            }
        },
        serde_json::Value::Object(map) => {
            for key in [
                "path",
                "file",
                "file_path",
                "filepath",
                "target",
                "target_path",
                "output",
                "dest",
                "destination",
                "new_path",
                "to",
                "from",
                "old_path",
            ] {
                if let Some(inner) = map.get(key) {
                    collect_string_values(inner, out);
                }
            }
            // 递归扫描嵌套结构中的路径字段
            for inner in map.values() {
                if inner.is_object() || inner.is_array() {
                    collect_path_like_from_value(inner, out);
                }
            }
        },
        _ => {},
    }
}

fn looks_like_path(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() || s.len() > 1024 {
        return false;
    }
    // 常见路径特征：包含分隔符、相对/绝对前缀、或带扩展名的文件名
    s.contains('/')
        || s.contains('\\')
        || s.starts_with("./")
        || s.starts_with("../")
        || s.starts_with("~/")
        || (s.contains('.') && !s.contains(' ') && s.len() <= 256)
}

/// 任务声明/推断的读键（显式 `read_keys` + 编码场景启发式推断）。
///
/// 推断用于提升读写冲突检测覆盖率；保守起见仅在读意图明显时启用。
pub fn read_keys_of(task: &TaskNode) -> Vec<String> {
    let mut keys: Vec<String> = task.read_keys.clone();
    if let Some(arr) = task.args.get("_read_keys").and_then(|v| v.as_array()) {
        for value in arr {
            if let Some(key) = value.as_str() {
                push_unique(&mut keys, key);
            }
        }
    }
    for key in infer_read_keys(task) {
        push_unique(&mut keys, &key);
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
            let a_reads = read_keys_of(a);
            let b_reads = read_keys_of(b);
            for key in &a_writes {
                if b_writes.contains(key) {
                    conflicts.push(Conflict {
                        kind: ConflictKind::WriteWrite,
                        key: key.clone(),
                        task_ids: vec![a.id.clone(), b.id.clone()],
                    });
                }
            }
            for key in &a_reads {
                if b_writes.contains(key) {
                    conflicts.push(Conflict {
                        kind: ConflictKind::ReadWrite,
                        key: key.clone(),
                        task_ids: vec![a.id.clone(), b.id.clone()],
                    });
                }
            }
            for key in &b_reads {
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

    fn coding_task(id: &str, tool_id: &str, args: serde_json::Value) -> TaskNode {
        TaskNode {
            id: id.to_string(),
            agent_id: "coder".to_string(),
            tool_id: tool_id.to_string(),
            args,
            depends_on: vec![],
            read_keys: vec![],
            write_keys: vec![],
        }
    }

    #[test]
    fn infers_write_key_from_path_for_write_tool() {
        let t = coding_task(
            "t1",
            "builtin/write",
            json!({"path": "src/lib.rs", "content": "x"}),
        );
        let keys = write_keys_of(&t);
        assert!(keys.iter().any(|k| k == "src/lib.rs"), "keys={keys:?}");
    }

    #[test]
    fn infers_write_keys_from_files_array_for_edit_tool() {
        let t = coding_task(
            "t1",
            "builtin/edit",
            json!({"files": ["src/a.rs", "src/b.rs"]}),
        );
        let keys = write_keys_of(&t);
        assert!(keys.iter().any(|k| k == "src/a.rs"), "keys={keys:?}");
        assert!(keys.iter().any(|k| k == "src/b.rs"), "keys={keys:?}");
    }

    #[test]
    fn does_not_infer_write_key_for_read_like_tool() {
        let t = coding_task("t1", "builtin/read", json!({"path": "src/lib.rs"}));
        let keys = write_keys_of(&t);
        assert!(
            keys.is_empty(),
            "read-like tool should not infer write keys: {keys:?}"
        );
    }

    #[test]
    fn detects_conflict_from_inferred_same_file_writes() {
        let a = coding_task("t1", "file/write", json!({"path": "src/main.rs"}));
        let b = coding_task("t2", "file/edit", json!({"file_path": "src/main.rs"}));
        let conflicts = detect_conflicts(&[&a, &b]);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].kind, ConflictKind::WriteWrite);
        assert_eq!(conflicts[0].key, "src/main.rs");
    }

    #[test]
    fn inferred_keys_merge_with_explicit_without_duplicates() {
        let mut t = coding_task("t1", "builtin/write", json!({"path": "src/lib.rs"}));
        t.write_keys = vec!["src/lib.rs".to_string(), "out.txt".to_string()];
        let keys = write_keys_of(&t);
        assert_eq!(
            keys.iter().filter(|k| k.as_str() == "src/lib.rs").count(),
            1,
            "keys={keys:?}"
        );
        assert!(keys.iter().any(|k| k == "out.txt"), "keys={keys:?}");
    }

    #[test]
    fn infers_nested_patch_path_for_apply_patch_tool() {
        let t = coding_task(
            "t1",
            "apply_patch",
            json!({"patch": {"path": "src/lib.rs", "diff": "@@"}}),
        );
        let keys = write_keys_of(&t);
        assert!(keys.iter().any(|k| k == "src/lib.rs"), "keys={keys:?}");
    }

    #[test]
    fn infers_read_key_from_path_for_read_tool() {
        let t = coding_task("t1", "builtin/read", json!({"path": "src/lib.rs"}));
        let keys = read_keys_of(&t);
        assert!(keys.iter().any(|k| k == "src/lib.rs"), "keys={keys:?}");
    }

    #[test]
    fn detects_read_write_conflict_from_inferred_read_key() {
        let reader = coding_task("t1", "builtin/read", json!({"path": "src/lib.rs"}));
        let writer = coding_task("t2", "builtin/write", json!({"path": "src/lib.rs"}));
        let conflicts = detect_conflicts(&[&reader, &writer]);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].kind, ConflictKind::ReadWrite);
        assert_eq!(conflicts[0].key, "src/lib.rs");
    }

    #[test]
    fn inferred_read_keys_merge_with_explicit_without_duplicates() {
        let mut t = coding_task("t1", "builtin/read", json!({"path": "src/lib.rs"}));
        t.read_keys = vec!["src/lib.rs".to_string(), "README.md".to_string()];
        let keys = read_keys_of(&t);
        assert_eq!(
            keys.iter().filter(|k| k.as_str() == "src/lib.rs").count(),
            1,
            "keys={keys:?}"
        );
        assert!(keys.iter().any(|k| k == "README.md"), "keys={keys:?}");
    }
}
