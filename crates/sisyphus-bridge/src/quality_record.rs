//! Bridge 路径的任务级质量记录（TaskRecord）落盘。
//!
//! 为 bridge 的 goal / SOP 执行路径写入 `TaskRecord`，供
//! `sisyphus metrics quality` 聚合。落盘为 best-effort，不影响执行响应。
//!
//! 覆盖入口：HTTP `execute_goal` / composite / goal streaming；
//! gRPC `execute_sop` / `stream_execution`（goal 与 SOP 分支）/ `call_jcode`。
//!
//! 引擎版本归因复用 `sisyphus_metrics::engine_revision`（与 CLI 的
//! `dev --record` / `verify --record` 同一实现），避免两处探测口径分叉。
//!
//! 口径见 `docs/plans/bridge-task-record-metrics.md` 与
//! `docs/plans/single-node-quality-metrics.md`。

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sisyphus_metrics::engine_revision::{engine_revision, EngineRevision};
use sisyphus_metrics::quality::{TaskRecord, TaskStatus};

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

/// 构建任务 ID：`<prefix>-<sanitized hint>-<ms>`。
///
/// 拆成纯函数便于单测：ID 必须单次执行唯一（timestamp 保证），且文件名
/// 只用 ASCII 安全字符（hint 可能来自用户 goal / sop_id）。
fn build_task_id(prefix: &str, id_hint: &str, ts_ms: u128) -> String {
    format!("{prefix}-{}-{ts_ms}", sanitize_for_filename(id_hint))
}

/// 构建一条 bridge 执行记录（纯函数，便于单测字段映射）。
#[allow(clippy::too_many_arguments)]
fn build_record(
    task_prefix: &str,
    scenario: &str,
    id_hint: &str,
    passed: bool,
    duration: Duration,
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    revision: Option<&EngineRevision>,
    ts_ms: u128,
    recorded_at: String,
) -> TaskRecord {
    TaskRecord {
        task_id: build_task_id(task_prefix, id_hint, ts_ms),
        scenario: scenario.to_string(),
        status: if passed {
            TaskStatus::Pass
        } else {
            TaskStatus::Fail
        },
        // bridge 执行没有 dev 式客观闸门循环：首轮/最终与结果一致，
        // 不虚构「修复前失败」样本污染修复率分母。
        initial_verify_passed: passed,
        final_verify_passed: passed,
        fix_attempts: 0,
        regressed: false,
        duration_ms: duration.as_millis().min(u128::from(u64::MAX)) as u64,
        prompt_tokens,
        completion_tokens,
        engine_commit: revision.map(|r| r.commit.clone()),
        engine_dirty: revision.map(|r| r.dirty),
        recorded_at,
    }
}

/// 为一次执行构建并 best-effort 落盘 TaskRecord。
///
/// `task_prefix` 决定文件名/`task_id` 前缀，`scenario` 决定聚合分组；
/// 两者分开是因为同一类入口（如 goal）会来自多个传输入口。
#[allow(clippy::too_many_arguments)]
pub(crate) fn record_execution(
    task_prefix: &str,
    scenario: &str,
    id_hint: &str,
    passed: bool,
    duration: Duration,
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
) {
    let Some(dir) = records_dir() else {
        return;
    };

    let revision = engine_revision();
    let record = build_record(
        task_prefix,
        scenario,
        id_hint,
        passed,
        duration,
        prompt_tokens,
        completion_tokens,
        revision.as_ref(),
        now_ms(),
        chrono::Utc::now().to_rfc3339(),
    );

    if let Err(e) = write_record(&dir, &record.task_id, &record) {
        tracing::warn!(
            error = %e,
            task_id = %record.task_id,
            dir = %dir.display(),
            "failed to write bridge quality task record (best-effort)"
        );
    }
}

/// HTTP unary `POST /api/execute/goal`。
pub(crate) fn record_goal_execution(
    sop_id: &str,
    passed: bool,
    duration: Duration,
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
) {
    record_execution(
        "bridge-goal",
        "bridge-goal-execute",
        sop_id,
        passed,
        duration,
        prompt_tokens,
        completion_tokens,
    );
}

/// HTTP `POST /api/execute/composite`：以请求级 composite id 归属。
pub(crate) fn record_composite_execution(
    composite_id: &str,
    passed: bool,
    duration: Duration,
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
) {
    record_execution(
        "bridge-composite",
        "bridge-composite-execute",
        composite_id,
        passed,
        duration,
        prompt_tokens,
        completion_tokens,
    );
}

/// goal 流式执行（HTTP SSE / gRPC server-streaming）。
pub(crate) fn record_goal_streaming(
    id_hint: &str,
    passed: bool,
    duration: Duration,
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
) {
    record_execution(
        "bridge-goal-stream",
        "bridge-goal-streaming",
        id_hint,
        passed,
        duration,
        prompt_tokens,
        completion_tokens,
    );
}

/// 直接 SOP/工具执行（HTTP/gRPC `execute_sop`、gRPC stream 的非 goal 分支）。
pub(crate) fn record_sop_execution(
    sop_id: &str,
    passed: bool,
    duration: Duration,
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
) {
    record_execution(
        "bridge-sop",
        "bridge-sop-execute",
        sop_id,
        passed,
        duration,
        prompt_tokens,
        completion_tokens,
    );
}

/// gRPC `call_jcode` 的 goal 执行。
pub(crate) fn record_jcode_execution(
    task_id: &str,
    passed: bool,
    duration: Duration,
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
) {
    record_execution(
        "bridge-jcode",
        "bridge-jcode-execute",
        task_id,
        passed,
        duration,
        prompt_tokens,
        completion_tokens,
    );
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

/// 计算任务级 token 用量增量（best-effort）。
///
/// `before`/`after` 为 LLM 客户端累计快照；仅在调用方已确认「观测窗口内无并发
/// 归因冲突」时才有意义（见 `MySisyphusBridge::begin_llm_task`）。
/// - `after == None` → provider 未上报 usage，返回 `(None, None)`（未知 ≠ 0）
/// - `before == None` → 视作累计基线为 0（客户端在无记录时返回 `None`）
/// - 正常情况返回饱和差值
pub(crate) fn token_usage_delta(
    before: Option<sisyphus_types::LlmTokenUsage>,
    after: Option<sisyphus_types::LlmTokenUsage>,
) -> (Option<u64>, Option<u64>) {
    let Some(after) = after else {
        return (None, None);
    };
    let before = before.unwrap_or_default();
    (
        Some(after.prompt_tokens.saturating_sub(before.prompt_tokens)),
        Some(after.completion_tokens.saturating_sub(before.completion_tokens)),
    )
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

    // `normalize_sha` 的边界（非 sha 输出、`-dirty` 后缀、过短）由
    // `sisyphus-metrics` 的 engine_revision 单测覆盖，bridge 不再重复实现。

    #[test]
    fn build_task_id_includes_prefix_and_sanitized_hint() {
        assert_eq!(
            build_task_id("bridge-goal", "sop/main:v1", 1700000000000),
            "bridge-goal-sop_main_v1-1700000000000"
        );
    }

    #[test]
    fn build_record_maps_pass_fail_without_faking_repair_semantics() {
        let rev = EngineRevision {
            commit: "abcdef123456".into(),
            dirty: true,
        };
        let ok = build_record(
            "bridge-composite",
            "bridge-composite-execute",
            "composite-x",
            true,
            Duration::from_millis(12),
            None,
            None,
            Some(&rev),
            42,
            "2026-11-13T00:00:00Z".into(),
        );
        assert_eq!(ok.task_id, "bridge-composite-composite-x-42");
        assert_eq!(ok.scenario, "bridge-composite-execute");
        assert_eq!(ok.status, TaskStatus::Pass);
        assert!(ok.initial_verify_passed);
        assert!(ok.final_verify_passed);
        assert_eq!(ok.fix_attempts, 0);
        assert!(!ok.regressed);
        assert_eq!(ok.duration_ms, 12);
        assert_eq!(ok.engine_commit.as_deref(), Some("abcdef123456"));
        assert_eq!(ok.engine_dirty, Some(true));

        let failed = build_record(
            "bridge-sop",
            "bridge-sop-execute",
            "sop-1",
            false,
            Duration::from_millis(1),
            None,
            None,
            None,
            43,
            "2026-11-13T00:00:01Z".into(),
        );
        assert_eq!(failed.status, TaskStatus::Fail);
        assert!(!failed.initial_verify_passed);
        assert!(!failed.final_verify_passed);
        assert_eq!(failed.fix_attempts, 0);
        assert!(!failed.regressed);
        assert_eq!(failed.engine_commit, None);
        assert_eq!(failed.engine_dirty, None);
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

    #[test]
    fn token_usage_delta_returns_none_when_provider_omits_usage() {
        assert_eq!(token_usage_delta(None, None), (None, None));
        assert_eq!(
            token_usage_delta(
                Some(sisyphus_types::LlmTokenUsage {
                    prompt_tokens: 10,
                    completion_tokens: 5,
                    calls: 1,
                }),
                None
            ),
            (None, None)
        );
    }

    #[test]
    fn token_usage_delta_computes_saturating_difference() {
        let before = Some(sisyphus_types::LlmTokenUsage {
            prompt_tokens: 100,
            completion_tokens: 20,
            calls: 2,
        });
        let after = Some(sisyphus_types::LlmTokenUsage {
            prompt_tokens: 160,
            completion_tokens: 35,
            calls: 3,
        });
        assert_eq!(token_usage_delta(before, after), (Some(60), Some(15)));
        // 基线未知（首次观测前客户端未上报）时按 0 计
        assert_eq!(token_usage_delta(None, after), (Some(160), Some(35)));
        // 防御性饱和：after 小于 before 不应下溢
        let weird_after = Some(sisyphus_types::LlmTokenUsage {
            prompt_tokens: 50,
            completion_tokens: 10,
            calls: 3,
        });
        assert_eq!(token_usage_delta(before, weird_after), (Some(0), Some(0)));
    }
}
