//! P0 修复验证 ①：`execution_traces` 缺 `project_id` 列（原缺陷：查询恒 500）。
//!
//! 修复内容：
//! - 建表语句补 `project_id TEXT NOT NULL DEFAULT 'default'`；
//! - 旧库迁移：pragma 检查 + `ALTER TABLE ADD COLUMN`（与 experiences 迁移同模式）；
//! - `record_trace_event` 增加 `project_id` 参数并写入该列。
//!
//! 修复前（缺陷固化）：查询报 `no such column: project_id`；
//! 修复后（本文件断言）：查询成功、项目隔离生效、旧库迁移可用。
//!
//! 运行：
//! ```bash
//! cargo test -p sisyphus-mem --test repro_p0_traces -- --nocapture
//! ```

use sisyphus_mem::MfsStore;
use sisyphus_types::{CacheConfig, StateStore};

fn db_path_for(test_name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target").join(format!(
        "sisyphus_repro_p0_traces_{}_{}.db",
        test_name,
        std::process::id()
    ))
}

fn cleanup(db_path: &std::path::Path) {
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

#[tokio::test]
async fn fix_p0_1_traces_query_and_project_isolation() {
    // 文件库保证跨连接一致（与 mfs_store_test.rs::test_persistence 相同模式）
    let db_path = db_path_for("isolation");
    cleanup(&db_path);
    let db_url = format!("sqlite:{}", db_path.display());

    let store = MfsStore::new(&db_url, &CacheConfig::default()).await.expect("MfsStore::new");

    // 1) 写入两条 trace：proj-1 一条、proj-2 一条
    store
        .record_trace_event(
            "proj-1",
            "sop-1",
            "node-1",
            "error",
            Some("boom".to_string()),
            1,
            2,
        )
        .await
        .expect("record_trace_event(proj-1)");
    store
        .record_trace_event(
            "proj-2",
            "sop-1",
            "node-2",
            "ok",
            Some("fine".to_string()),
            3,
            4,
        )
        .await
        .expect("record_trace_event(proj-2)");

    // 2) 对照组：不涉及 project_id 的 SELECT 正常
    let failed = store
        .get_most_failed_nodes(1)
        .await
        .expect("control SELECT on execution_traces should succeed");
    assert_eq!(
        failed,
        vec![("node-1".to_string(), 1)],
        "control query reads back the error row"
    );

    // 3) 修复路径：查询成功且只返回本项目的行
    let rows = store
        .get_traces_for_sop("proj-1", "sop-1")
        .await
        .expect("get_traces_for_sop must succeed after fix");
    assert_eq!(
        rows,
        vec![(
            "node-1".to_string(),
            "error".to_string(),
            "boom".to_string(),
            1,
            2
        )]
    );

    // 4) 项目隔离：其他项目查不到
    let other =
        store.get_traces_for_sop("proj-2", "sop-1").await.expect("get_traces_for_sop for proj-2");
    assert_eq!(
        other,
        vec![(
            "node-2".to_string(),
            "ok".to_string(),
            "fine".to_string(),
            3,
            4
        )]
    );

    let missing =
        store.get_traces_for_sop("proj-3", "sop-1").await.expect("get_traces_for_sop for proj-3");
    assert!(
        missing.is_empty(),
        "cross-project query must not leak rows; got {missing:?}"
    );

    drop(store);
    cleanup(&db_path);
}

#[tokio::test]
async fn fix_p0_1_legacy_db_migration_backfills_project_id() {
    let db_path = db_path_for("migration");
    cleanup(&db_path);
    let db_url = format!("sqlite:{}", db_path.display());

    // 1) 构造"旧库"：修复前的表结构（无 project_id）+ 一行旧数据
    let opts = sisyphus_mem::sqlite_config::sqlite_options(&db_url).expect("sqlite options");
    let pool = sqlx::SqlitePool::connect_with(opts).await.expect("raw pool");
    sqlx::query(
        "CREATE TABLE execution_traces (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            sop_id TEXT NOT NULL,
            node_id TEXT NOT NULL,
            status TEXT NOT NULL,
            output TEXT,
            fuel_consumed INTEGER,
            duration_ms INTEGER,
            timestamp INTEGER NOT NULL
        )",
    )
    .execute(&pool)
    .await
    .expect("create legacy table");
    sqlx::query(
        "INSERT INTO execution_traces (sop_id, node_id, status, output, fuel_consumed, duration_ms, timestamp)
         VALUES ('sop-legacy', 'node-legacy', 'error', 'old-boom', 7, 8, 100)",
    )
    .execute(&pool)
    .await
    .expect("insert legacy row");
    pool.close().await;

    // 2) 用当前代码打开 → 应自动迁移
    let store = MfsStore::new(&db_url, &CacheConfig::default()).await.expect("MfsStore::new");

    // 3) 旧数据回填为默认项目，且修复后的查询可用
    let rows = store
        .get_traces_for_sop("default", "sop-legacy")
        .await
        .expect("post-migration query must succeed");
    assert_eq!(
        rows,
        vec![(
            "node-legacy".to_string(),
            "error".to_string(),
            "old-boom".to_string(),
            7,
            8
        )],
        "legacy row must be backfilled with project_id='default'"
    );

    drop(store);
    cleanup(&db_path);
}
