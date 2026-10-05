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
    // 2026-10-05 记录一个**尚未闭环**的偶发现象，勿当成本用例的已知行为：
    // 全量 `cargo test -p sisyphus-core` 期间曾出现一次 `1/10 rounds lost`。
    // 事后定向复跑约 130 轮（单测 x3、6 并发、8 并发、全量 core、CI test-light）
    // 均未复现。插桩测量显示 flush 阶段增量恒为约 0.1ms（峰值 0.19s）、
    // `record_cognitive_trace` 写入峰值约 1s，都远低于 flush 内 5s 的超时，
    // 因此「超时导致回执被丢弃」这一假说已被证伪；FIFO 屏障逻辑本身也正确。
    // 根因至今未能定位，如再现请记录当次完整日志（含 SQLITE/IO 报错）。
    let mut lost = 0;
    const ROUNDS: usize = 10;
    for i in 0..ROUNDS {
        let db = temp_db_path(&format!("flush_{i}"));
        let _ = std::fs::remove_file(&db);

        let em1 = test_execution_manager_with(&db).await;
        em1.record_cognitive_trace("sop-a", step_event("Flushed"));
        em1.flush_cognitive_traces().await;
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
    em1.flush_cognitive_traces().await;

    let em2 = test_execution_manager_with(&db).await;
    let rows = em2.cognitive_trace_history("default", None, 100).await.expect("history");
    assert_eq!(rows.len(), 1);
    assert!(matches!(
        rows[0].event,
        ExecutionEvent::CognitiveStep { ref step, .. } if step == "BeforeRestart"
    ));
    assert_eq!(rows[0].sop_id, "sop-a");
}
