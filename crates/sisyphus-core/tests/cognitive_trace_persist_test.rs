//! 认知追踪持久化集成测试：记录决策事件 → 异步 writer 落盘 → SQL 历史检索。
//!
//! 覆盖 `record_cognitive_trace` → `cognitive_trace_history` 的完整闭环，
//! 验证跨调用持久化、新在前排序与 SOP 过滤。

use async_trait::async_trait;
use sisyphus_core::execution_manager::CognitiveTraceEntry;
use sisyphus_core::ExecutionManager;
use sisyphus_types::{
    ExecutionEvent, TestExecutor, TestResult, ToolExecutor, ToolMetadata, TypesError,
};
use std::sync::Arc;

// ── 最小 mock ──────────────────────────────────────────

struct NoopToolExecutor;

#[async_trait]
impl ToolExecutor for NoopToolExecutor {
    async fn execute(
        &self,
        _tool_id: &str,
        _args: Vec<u8>,
        _fuel_limit: u64,
    ) -> Result<Vec<u8>, TypesError> {
        Err(TypesError::ToolExecutionError {
            tool_id: "mock".into(),
            details: "noop".into(),
        })
    }
    async fn list_tools(&self) -> Result<Vec<ToolMetadata>, TypesError> {
        Ok(vec![])
    }
}

struct NoopTestExecutor;

#[async_trait]
impl TestExecutor for NoopTestExecutor {
    async fn run_test(
        &self,
        _command: &str,
        _workdir: Option<&str>,
    ) -> Result<TestResult, TypesError> {
        Ok(TestResult {
            success: true,
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
            duration_ms: 0,
            error_summary: None,
        })
    }
}

async fn test_execution_manager() -> ExecutionManager {
    test_execution_manager_with("sqlite::memory:").await
}

async fn test_execution_manager_with(db: &str) -> ExecutionManager {
    let state_store = Arc::new(
        sisyphus_mem::MfsStore::new(db, &sisyphus_types::CacheConfig::default())
            .await
            .expect("store"),
    );
    let pool = sqlx::SqlitePool::connect(db).await.expect("pool");
    let sop_store =
        Arc::new(sisyphus_mem::SopSqlStore::new(pool.clone()).await.expect("sop store"));
    ExecutionManager::new(
        state_store,
        sop_store,
        Arc::new(NoopToolExecutor),
        Arc::new(NoopTestExecutor),
        Arc::new(sisyphus_core::intervention_manager::InterventionManager::new()),
        Arc::new(
            sisyphus_core::internalization_manager::InternalizationManager::new(
                Arc::new(sisyphus_tools_llm::metis::DummyLlmClient),
                Arc::new(sisyphus_knowledge::AssetManager::new(
                    ".omo/test_knowledge".into(),
                )),
            ),
        ),
        None,
    )
}

/// 每个进程唯一的临时 SQLite 文件路径。
fn temp_db_path(tag: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "sisyphus_cog_trace_{}_{}.db",
        std::process::id(),
        tag
    ));
    path.to_string_lossy().to_string()
}

fn step_event(step: &str) -> ExecutionEvent {
    ExecutionEvent::CognitiveStep {
        step: step.to_string(),
        message: format!("{step} message"),
        metrics: Some(sisyphus_types::CognitiveMetrics {
            iteration: 1,
            deviation: 0.2,
            confidence: 0.8,
            progress: 0.5,
            token_cost: 10,
            stability: 0.9,
            info_gain: 0.1,
        }),
    }
}

/// 轮询等待 writer 落盘（异步），最多 ~1s。
async fn wait_history(em: &ExecutionManager, expected: usize) -> Vec<CognitiveTraceEntry> {
    for _ in 0..50 {
        let rows = em.cognitive_trace_history("default", None, 100).await.expect("history");
        if rows.len() >= expected {
            return rows;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    em.cognitive_trace_history("default", None, 100).await.expect("history")
}

#[tokio::test]
async fn test_persist_roundtrip_via_history() {
    let em = test_execution_manager().await;

    // 只保留 CognitiveStep / MicroReflection；NodeStarted 不入库。
    em.record_cognitive_trace("sop-a", step_event("Predicting"));
    em.record_cognitive_trace("sop-a", step_event("Reflecting"));
    em.record_cognitive_trace(
        "sop-a",
        ExecutionEvent::NodeStarted {
            node_name: "n1".into(),
        },
    );

    let rows = wait_history(&em, 2).await;
    assert_eq!(rows.len(), 2, "only cognitive events should persist");
    // 新在前
    assert!(matches!(
        rows[0].event,
        ExecutionEvent::CognitiveStep { ref step, .. } if step == "Reflecting"
    ));
    assert!(matches!(
        rows[1].event,
        ExecutionEvent::CognitiveStep { ref step, .. } if step == "Predicting"
    ));
    assert!(rows.iter().all(|r| r.sop_id == "sop-a"));
    // 热缓冲也应包含相同事件
    let hot = em.recent_cognitive_trace(10);
    assert_eq!(hot.len(), 2);
}

#[tokio::test]
async fn test_history_sop_filter_and_limit() {
    let em = test_execution_manager().await;
    em.record_cognitive_trace("sop-a", step_event("A1"));
    em.record_cognitive_trace("sop-a", step_event("A2"));
    em.record_cognitive_trace("sop-b", step_event("B1"));

    let all = wait_history(&em, 3).await;
    assert_eq!(all.len(), 3);

    let sop_a =
        em.cognitive_trace_history("default", Some("sop-a"), 10).await.expect("filtered history");
    assert_eq!(sop_a.len(), 2);
    assert!(sop_a.iter().all(|r| r.sop_id == "sop-a"));

    let limited =
        em.cognitive_trace_history("default", Some("sop-a"), 1).await.expect("limited history");
    assert_eq!(limited.len(), 1);
    assert!(matches!(
        limited[0].event,
        ExecutionEvent::CognitiveStep { ref step, .. } if step == "A2"
    ));
}

#[tokio::test]
async fn test_flush_cognitive_traces_survives_recreation() {
    // bridge 关闭（SIGINT/SIGTERM）时的真实语义：先 flush 再丢弃实例。
    // 修复前不 flush 直接 drop，实测 30 轮中有 5 轮丢失认知追踪。
    //
    // 2026-10-05 曾记录一个**尚未闭环**的偶发现象，勿当成本用例的已知行为：
    // 全量 `cargo test -p sisyphus-core` 期间曾出现一次 `1/10 rounds lost`。
    // 事后定向复跑约 130 轮（单测 x3、6 并发、8 并发、全量 core、CI test-light）
    // 均未复现。插桩测量显示 flush 阶段增量恒为约 0.1ms（峰值 0.19s）、
    // `record_cognitive_trace` 写入峰值约 1s，都远低于 5s 超时，
    // 因此「超时导致回执被丢弃」这一假说已被证伪；FIFO 屏障逻辑本身也正确。
    //
    // 随后定位并修复了一个**确定存在**的相邻缺陷：写入失败此前只由
    // `tracing::warn` 记录，`flush_cognitive_traces` 一律返回 `()`，
    // 即写入彻底失败也对外谎报「已持久化」（见同文件下两个 flush 回归用例）。
    // 本用例偶发丢失的**确切根因仍未定位**，如再现请记录当次完整日志。
    let mut lost = 0;
    const ROUNDS: usize = 10;
    for i in 0..ROUNDS {
        let db = temp_db_path(&format!("flush_{i}"));
        let _ = std::fs::remove_file(&db);

        let em1 = test_execution_manager_with(&db).await;
        em1.record_cognitive_trace("sop-a", step_event("Flushed"));
        em1.flush_cognitive_traces().await.expect("flush");
        drop(em1);

        let em2 = test_execution_manager_with(&db).await;
        let rows = em2.cognitive_trace_history("default", None, 100).await.expect("history");
        if rows.len() != 1 {
            lost += 1;
        }
        drop(em2);
        let _ = std::fs::remove_file(&db);
    }
    assert_eq!(
        lost, 0,
        "{lost}/{ROUNDS} rounds lost cognitive trace after flush"
    );
}

#[tokio::test]
async fn test_persist_survives_buffer_recreation() {
    // 模拟 bridge 重启：新 ExecutionManager 实例读同一 SQL 文件仍可取回历史。
    let db = temp_db_path("restart");
    let em1 = test_execution_manager_with(&db).await;
    em1.record_cognitive_trace("sop-a", step_event("BeforeRestart"));

    // flush 屏障：等待 writer 把队列排空（修复前是轮询等待，失败被静默吞掉）。
    em1.flush_cognitive_traces().await.expect("flush must succeed");

    let em2 = test_execution_manager_with(&db).await;
    let rows = em2.cognitive_trace_history("default", None, 100).await.expect("history");
    assert_eq!(rows.len(), 1);
    assert!(matches!(
        rows[0].event,
        ExecutionEvent::CognitiveStep { ref step, .. } if step == "BeforeRestart"
    ));
    assert_eq!(rows[0].sop_id, "sop-a");
}

/// 回归：`flush_cognitive_traces` 不得对**写入彻底失败**谎报成功。
///
/// 修复前，writer 的 `record_cognitive_trace` 失败只由 `tracing::warn`
/// 记录，而 flush 无论写入结果一律返回 `()`，其文档却承诺
/// 「返回时，此前记录均已写入 `cognitive_traces`」。bridge 优雅关闭因此
/// 把彻底丢失的认知追踪当成已持久化。
#[tokio::test]
async fn test_flush_reports_write_failure_instead_of_silent_success() {
    let dir = std::env::temp_dir().join(format!("sisyphus_cog_trace_ro_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let db = dir.join("readonly.db").to_string_lossy().to_string();

    let em = test_execution_manager_with(&db).await;

    // 删掉目标表：此后每次 INSERT 都必然报「no such table」。
    //
    // 曾试过把目录 chmod 成只读，但 SQLite 已持有的文件描述符不因 chmod 失效，
    // 写入往往照样成功（实测确实如此：flush 返回 Ok）。该手法依赖时序、不可靠，
    // 故改用 DDL 层面的确定性失败——这也让被验证的确实是一次真实 INSERT。
    drop_table(&db).await;

    em.record_cognitive_trace("sop-a", step_event("WillFail"));
    let outcome = em.flush_cognitive_traces().await;

    // 核心断言：写入彻底失败时 flush 必须报错，而不是返回 Ok。
    let err = outcome.expect_err("flush must not claim success when every write failed");
    let msg = err.to_string();
    assert!(
        msg.contains("failed to persist"),
        "error should name the persistence failure, got: {msg}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// 回归：写入恢复正常后，flush 的错误计数不残留，下一次 flush 必须成功。
///
/// 防止 `TraceFlushAck` 累积的失败数被 `mem::take` 之外的方式泄漏到后续屏障，
/// 导致「一次失败、之后永远失败」这种误报。
#[tokio::test]
async fn test_flush_error_count_resets_after_recovery() {
    let dir = std::env::temp_dir().join(format!("sisyphus_cog_trace_reset_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let db = dir.join("reset.db").to_string_lossy().to_string();

    let em = test_execution_manager_with(&db).await;

    drop_table(&db).await;
    em.record_cognitive_trace("sop-a", step_event("Fails"));
    assert!(
        em.flush_cognitive_traces().await.is_err(),
        "目标表缺失时写入必然失败，flush 应当报错"
    );

    // 重建目标表，模拟「故障恢复」。
    recreate_table(&db).await;

    em.record_cognitive_trace("sop-a", step_event("Succeeds"));
    em.flush_cognitive_traces().await.expect("恢复写权限后 flush 必须成功，失败计数不得残留");

    let _ = std::fs::remove_dir_all(&dir);
}

/// 删除 `cognitive_traces` 表，使后续 INSERT 必然失败。
async fn drop_table(db: &str) {
    let pool = sqlx::SqlitePool::connect(db).await.expect("pool");
    sqlx::query("DROP TABLE IF EXISTS cognitive_traces").execute(&pool).await.expect("drop table");
}

/// 按 `MfsStore::with_pool_internal` 的建表语句原样重建 `cognitive_traces`。
async fn recreate_table(db: &str) {
    let pool = sqlx::SqlitePool::connect(db).await.expect("pool");
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS cognitive_traces (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id TEXT NOT NULL,
            sop_id TEXT NOT NULL,
            timestamp_ms INTEGER NOT NULL,
            event_json TEXT NOT NULL
        )",
    )
    .execute(&pool)
    .await
    .expect("recreate table");
}
