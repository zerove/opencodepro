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
    let sop_store = Arc::new(
        sisyphus_mem::SopSqlStore::new(pool.clone())
            .await
            .expect("sop store"),
    );
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
        let rows = em
            .cognitive_trace_history("default", None, 100)
            .await
            .expect("history");
        if rows.len() >= expected {
            return rows;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    em.cognitive_trace_history("default", None, 100)
        .await
        .expect("history")
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

    let sop_a = em
        .cognitive_trace_history("default", Some("sop-a"), 10)
        .await
        .expect("filtered history");
    assert_eq!(sop_a.len(), 2);
    assert!(sop_a.iter().all(|r| r.sop_id == "sop-a"));

    let limited = em
        .cognitive_trace_history("default", Some("sop-a"), 1)
        .await
        .expect("limited history");
    assert_eq!(limited.len(), 1);
    assert!(matches!(
        limited[0].event,
        ExecutionEvent::CognitiveStep { ref step, .. } if step == "A2"
    ));
}

#[tokio::test]
async fn test_persist_survives_buffer_recreation() {
    // 模拟 bridge 重启：新 ExecutionManager 实例读同一 SQL 文件仍可取回历史。
    let db = temp_db_path("restart");
    let em1 = test_execution_manager_with(&db).await;
    em1.record_cognitive_trace("sop-a", step_event("BeforeRestart"));

    // 等待第一次落盘完成（writer 已消费 channel）。
    let _ = wait_history(&em1, 1).await;
    drop(em1);

    let em2 = test_execution_manager_with(&db).await;
    let rows = wait_history(&em2, 1).await;
    assert_eq!(rows.len(), 1);
    assert!(matches!(
        rows[0].event,
        ExecutionEvent::CognitiveStep { ref step, .. } if step == "BeforeRestart"
    ));
    assert_eq!(rows[0].sop_id, "sop-a");
}
