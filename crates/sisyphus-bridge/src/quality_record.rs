//! Bridge 路径的任务级质量记录（TaskRecord）落盘。
//!
//! 最小实现：为 HTTP `execute_goal` 成功/失败结果写入 `TaskRecord`，
//! 供 `sisyphus metrics quality` 聚合。落盘为 best-effort，不影响执行响应。
//!
//! 口径见 `docs/plans/bridge-task-record-metrics.md` 与
//! `docs/plans/single-node-quality-metrics.md`。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sisyphus_metrics::quality::{TaskRecord, TaskStatus};

/// 编译时记录的 bridge 源码目录（用于定位引擎 git 版本）。
const BRIDGE_SOURCE_DIR: &str = env!("CARGO_MANIFEST_DIR");

#[derive(Debug, Clone, PartialEq, Eq)]
struct EngineRevision {
    commit: String,
    dirty: bool,
}

fn engine_revision() -> Option<EngineRevision> {
    let dir = Path::new(BRIDGE_SOURCE_DIR);
    if !dir.is_dir() {
        return None;
    }
    let commit = git_short_sha(dir)?;
    Some(EngineRevision {
        commit,
        dirty: git_is_dirty(dir),
    })
}

fn git_short_sha(dir: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let sha = String::from_utf8(output.stdout).ok()?;
    normalize_sha(&sha)
}

fn git_is_dirty(dir: &Path) -> bool {
    let Ok(output) =
        Command::new("git").arg("-C").arg(dir).args(["status", "--porcelain"]).output()
    else {
        return true;
    };
    if !output.status.success() {
        return true;
    }
    match String::from_utf8(output.stdout) {
        Ok(status) => !status.trim().is_empty(),
        Err(_) => true,
    }
}

fn normalize_sha(raw: &str) -> Option<String> {
    let sha = raw.trim();
    let ok = (7..=40).contains(&sha.len()) && sha.chars().all(|c| c.is_ascii_hexdigit());
    ok.then(|| sha.to_ascii_lowercase())
}

/// 记录目录：显式环境变量优先；未设置则不写（opt-in，避免意外写入）。
pub(crate) fn records_dir() -> Option<PathBuf> {
    std::env::var("SISYPHUS_BRIDGE_QUALITY_RECORDS_DIR")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
}

fn sanitize_for_filename(s: &str) -> String {
    let mut out = String::with_capacity(s.len().min(48));
    for ch in s.chars() {
        if out.len() >= 48 {
            break;
        }
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "goal".to_string()
    } else {
        out
    }
}

/// 为一次 goal 执行构建并 best-effort 落盘 TaskRecord。
pub(crate) fn record_goal_execution(
    sop_id: &str,
    passed: bool,
    duration: Duration,
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
) {
    let Some(dir) = records_dir() else {
        return;
    };

    let revision = engine_revision();
    let ts = now_ms();
    let sop = sanitize_for_filename(sop_id);
    let task_id = format!("bridge-goal-{sop}-{ts}");
    let record = TaskRecord {
        task_id: task_id.clone(),
        scenario: "bridge-goal-execute".to_string(),
        status: if passed {
            TaskStatus::Pass
        } else {
            TaskStatus::Fail
        },
        initial_verify_passed: passed,
        final_verify_passed: passed,
        fix_attempts: 0,
        regressed: false,
        duration_ms: duration.as_millis().min(u128::from(u64::MAX)) as u64,
        prompt_tokens,
        completion_tokens,
        engine_commit: revision.as_ref().map(|r| r.commit.clone()),
        engine_dirty: revision.as_ref().map(|r| r.dirty),
        recorded_at: chrono::Utc::now().to_rfc3339(),
    };

    if let Err(e) = write_record(&dir, &task_id, &record) {
        tracing::warn!(
            error = %e,
            task_id = %task_id,
            dir = %dir.display(),
            "failed to write bridge quality task record (best-effort)"
        );
    }
}

fn write_record(dir: &Path, task_id: &str, record: &TaskRecord) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{task_id}.json"));
    let tmp = dir.join(format!("{task_id}.json.tmp"));
    let json = serde_json::to_string_pretty(record)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&tmp, json)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_for_filename_replaces_and_limits() {
        assert_eq!(sanitize_for_filename("sop/main:v1"), "sop_main_v1");
        assert_eq!(sanitize_for_filename(""), "goal");
        let long = "a".repeat(100);
        assert_eq!(sanitize_for_filename(&long).len(), 48);
    }

    #[test]
    fn normalize_sha_accepts_valid() {
        assert_eq!(
            normalize_sha("5869d3dbae51\n").as_deref(),
            Some("5869d3dbae51")
        );
        assert_eq!(normalize_sha("bad"), None);
    }

    #[test]
    fn write_record_writes_json_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let record = TaskRecord {
            task_id: "bridge-goal-test-1".into(),
            scenario: "bridge-goal-execute".into(),
            status: TaskStatus::Pass,
            initial_verify_passed: true,
            final_verify_passed: true,
            fix_attempts: 0,
            regressed: false,
            duration_ms: 42,
            prompt_tokens: None,
            completion_tokens: None,
            engine_commit: Some("abcdef123456".into()),
            engine_dirty: Some(false),
            recorded_at: "2026-11-13T00:00:00Z".into(),
        };
        write_record(dir.path(), "bridge-goal-test-1", &record).unwrap();
        let path = dir.path().join("bridge-goal-test-1.json");
        let content = std::fs::read_to_string(path).unwrap();
        let parsed: TaskRecord = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed.task_id, "bridge-goal-test-1");
        assert_eq!(parsed.status, TaskStatus::Pass);
        assert_eq!(parsed.fix_attempts, 0);
        assert!(!parsed.regressed);
    }
}
