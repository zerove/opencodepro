//! SqlStore 基础集成测试 (ADR-007 §2: 公共 API 必带测试)
//!
//! 覆盖:
//! - new() / with_config() 构造路径
//! - store_embedding + search_similar 基础 happy path
//! - flush() 正常完成
//! - cache_stats / len / capacity 辅助函数
//! - 容错: 不可用 DB URL 返回 Err 而非 panic (ADR-007 §1 回归)

use sisyphus_mem::{SqlStore, SqlStoreConfig};
use tempfile::TempDir;

// sqlx::any::AnyPool 在测试时需要显式注册 driver.
// sqlx-sqlite::DRIVER 是 const AnyDriver, 传 &[DRIVER] 给 install_drivers.
static INIT_DRIVERS: std::sync::Once = std::sync::Once::new();
fn init_drivers() {
    INIT_DRIVERS.call_once(|| {
        // 安装 sqlite + postgres driver (与 main deps 对齐)
        let _ = sqlx::any::install_drivers(&[sqlx_sqlite::any::DRIVER]);
    });
}

const DIM: usize = 4;

fn cfg_in_memory() -> SqlStoreConfig {
    SqlStoreConfig {
        database_url: "sqlite::memory:".to_string(),
        enable_vectors: true,
        ..Default::default()
    }
}

// file-based SQLite, 跨连接共享 schema. tempdir 由调用方持有以避免被删除.
fn cfg_file(dir: &TempDir) -> SqlStoreConfig {
    let path = dir.path().join("sisyphus.db");
    SqlStoreConfig {
        database_url: format!("sqlite:///{}?mode=rwc", path.display()),
        enable_vectors: true,
        ..Default::default()
    }
}

fn make_vec(seed: f32) -> Vec<f32> {
    (0..DIM).map(|i| (seed + i as f32) * 0.1).collect()
}

#[tokio::test]
async fn new_in_memory_succeeds() {
    init_drivers();
    let store = SqlStore::new("sqlite::memory:").await;
    assert!(
        store.is_ok(),
        "in-memory SqlStore should construct (see error chain if Err)"
    );
}

#[tokio::test]
async fn new_with_config_in_memory_succeeds() {
    init_drivers();
    let store = SqlStore::with_config(cfg_in_memory()).await;
    assert!(store.is_ok());
}

#[tokio::test]
async fn new_invalid_url_returns_err_not_panic() {
    init_drivers();
    // ADR-007 §1: 错误路径必须 Err, 不 panic.
    // 故意构造无法解析的 URL 触发构造失败.
    let result = SqlStore::new("not-a-valid-url-at-all").await;
    if let Ok(store) = result {
        let res = store
            .store_embedding(
                Some("proj"),
                "col",
                "id1",
                &make_vec(0.0),
                &serde_json::json!({}),
            )
            .await;
        assert!(
            res.is_err(),
            "store_embedding against invalid URL must return Err"
        );
    }
}

#[tokio::test]
async fn store_and_search_embedding_roundtrip() {
    init_drivers();
    // file-based SQLite: pool 跨连接看到同一数据库 (sqlite::memory: 每个连接独立,
    // init_schema 建的表在后续 fetch 连接上看不到).
    let dir = tempfile::tempdir().expect("tempdir");
    let store = SqlStore::with_config(cfg_file(&dir)).await.expect("construct");
    let proj = Some("proj_a");
    let coll = "vectors";

    // store 3 vectors
    for (id, seed) in [("v1", 1.0_f32), ("v2", 2.0), ("v3", 5.0)] {
        store
            .store_embedding(proj, coll, id, &make_vec(seed), &serde_json::json!({}))
            .await
            .expect("store_embedding");
    }
    store.flush().await.expect("flush");
    // Verify rows via pool
    let pool = store.pool();
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM vectors")
        .fetch_one(pool)
        .await
        .expect("count");
    eprintln!("DEBUG row count: {}", row.0);

    // search similar to v1
    let query = make_vec(1.0);
    let hits = store
        .search_similar(proj, coll, &query, 3)
        .await
        .expect("search_similar");

    // SQLite 路径走 fetch_all + cosine similarity, store_embedding 直接 INSERT
    // 所以 hits 应该非空. 接受 self-similarity 最高 (v1 = v1) 但允许
    // 任何 v1/v2 接近 (因 query 接近 v1/v2)
    assert!(!hits.is_empty(), "search should return hits, got: {hits:?}");
}

#[tokio::test]
async fn flush_on_empty_store_succeeds() {
    init_drivers();
    let store = SqlStore::new("sqlite::memory:").await.expect("construct");
    // flush() 在空 buffer 应立即返回 Ok
    let res = store.flush().await;
    assert!(res.is_ok(), "flush on empty store must succeed");
}

#[tokio::test]
async fn cache_stats_reflects_capacity() {
    init_drivers();
    let cfg = SqlStoreConfig {
        database_url: "sqlite::memory:".to_string(),
        l1_capacity: 42,
        ..Default::default()
    };
    let store = SqlStore::with_config(cfg).await.expect("construct");
    let stats = store.cache_stats();
    assert_eq!(stats.capacity, 42, "capacity should match config");
    assert_eq!(store.capacity(), 42);
    assert_eq!(store.len(), 0, "new store has zero entries");
}

#[tokio::test]
async fn clear_cache_resets_state() {
    init_drivers();
    let store = SqlStore::new("sqlite::memory:").await.expect("construct");
    // 存一个向量后再清空
    store
        .store_embedding(
            Some("p"),
            "c",
            "x",
            &make_vec(0.0),
            &serde_json::json!({}),
        )
        .await
        .expect("store");
    store.flush().await.expect("flush");
    store.clear_cache();
    assert_eq!(store.len(), 0, "clear_cache should empty cache");
}

#[tokio::test]
async fn flush_repeated_under_load_does_not_panic() {
    init_drivers();
    // 回归测试 (commit b0d7925):
    // 之前 flush_batch 内部 WriteOp::Flush 分支用 unreachable! panic.
    // 现在返回 Err 让调用方决定. 此测试从公共 flush() 路径
    // 间接验证 — 正常路径不会触发, 但确保 flush 不会 panic.
    let store = SqlStore::new("sqlite::memory:").await.expect("construct");
    let res = store.flush().await;
    assert!(res.is_ok(), "flush must return Ok (not panic) on empty");

    // 连环 store+flush 多次模拟压力
    for i in 0..5 {
        store
            .store_embedding(
                Some("p"),
                "c",
                &format!("id_{i}"),
                &make_vec(i as f32),
                &serde_json::json!({"i": i}),
            )
            .await
            .expect("store");
    }
    store.flush().await.expect("flush");
}
