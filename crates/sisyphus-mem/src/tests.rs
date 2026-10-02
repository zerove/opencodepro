//! MfsStore 状态存取回归测试。
//!
//! 这些用例此前寄居在本文件却未被 `lib.rs` 引用，因此从未参与编译，
//! 属于静默失效的覆盖。`lib.rs` 已通过 `#[cfg(test)] mod tests;` 接入。
//! 原实现停留在 `MfsStore::new(db_url)` 单参数与同步 `restore_snapshot`
//! 的旧接口上，此处按当前签名重写。

use crate::store::MfsStore;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sisyphus_types::StateStore;
use std::sync::Arc;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
struct TestState {
    count: i32,
    name: String,
}

const PROJECT_ID: &str = "test_project";

async fn setup_store() -> MfsStore {
    let config = sisyphus_types::CacheConfig::default();
    MfsStore::new("sqlite::memory:", &config).await.expect("初始化内存状态库失败")
}

#[tokio::test]
async fn test_basic_set_get() {
    let store = setup_store().await;
    let state = TestState {
        count: 42,
        name: "Sisyphus".to_string(),
    };

    store
        .set_state_json(
            PROJECT_ID,
            "user_1",
            serde_json::to_value(&state).expect("序列化失败"),
        )
        .await
        .expect("写入状态失败");
    let retrieved_val = store
        .get_state_json(PROJECT_ID, "user_1")
        .await
        .expect("读取状态失败")
        .expect("写入后必须能读到");
    let retrieved: TestState = serde_json::from_value(retrieved_val).expect("反序列化失败");

    assert_eq!(retrieved, state);
}

#[tokio::test]
async fn test_get_missing_key() {
    let store = setup_store().await;
    let val =
        store.get_state_json(PROJECT_ID, "nonexistent").await.expect("读取不存在的键不应报错");
    assert_eq!(val, None);
}

#[tokio::test]
async fn test_overwrite_value() {
    let store = setup_store().await;
    store.set_state_json(PROJECT_ID, "k", json!("first")).await.expect("首次写入失败");
    store.set_state_json(PROJECT_ID, "k", json!("second")).await.expect("覆盖写入失败");
    let val =
        store.get_state_json(PROJECT_ID, "k").await.expect("读取失败").expect("覆盖后必须能读到");
    assert_eq!(val, json!("second"));
}

#[tokio::test]
async fn test_update_then_revert() {
    let store = setup_store().await;

    store.set_state_json(PROJECT_ID, "k", json!("v1")).await.expect("写入 v1 失败");

    let snap = store.take_snapshot().expect("创建快照失败");

    store.set_state_json(PROJECT_ID, "k", json!("v2")).await.expect("写入 v2 失败");
    assert_eq!(
        store.get_state_json(PROJECT_ID, "k").await.expect("读取失败").expect("v2 必须可读"),
        json!("v2")
    );

    store.restore_snapshot(snap).await.expect("恢复快照失败");
    assert_eq!(
        store.get_state_json(PROJECT_ID, "k").await.expect("恢复后读取失败").expect("v1 必须可读"),
        json!("v1")
    );
}

#[tokio::test]
async fn test_snapshot_restore() {
    let store = setup_store().await;
    store.set_state_json(PROJECT_ID, "k1", json!(1)).await.expect("写入 k1 失败");
    store.set_state_json(PROJECT_ID, "k2", json!(2)).await.expect("写入 k2 失败");

    let snapshot = store.take_snapshot().expect("创建快照失败");

    store.set_state_json(PROJECT_ID, "k1", json!(10)).await.expect("覆盖 k1 失败");
    store.set_state_json(PROJECT_ID, "k3", json!(3)).await.expect("新增 k3 失败");

    store.restore_snapshot(snapshot).await.expect("恢复快照失败");

    let v1 =
        store.get_state_json(PROJECT_ID, "k1").await.expect("读取 k1 失败").expect("k1 必须存在");
    let v2 =
        store.get_state_json(PROJECT_ID, "k2").await.expect("读取 k2 失败").expect("k2 必须存在");
    let v3 = store.get_state_json(PROJECT_ID, "k3").await.expect("读取 k3 失败");

    assert_eq!(v1, json!(1));
    assert_eq!(v2, json!(2));
    assert_eq!(v3, None, "快照之后新增的键必须被回滚");
}

#[tokio::test]
async fn test_concurrent_access() {
    let store = Arc::new(setup_store().await);
    let mut handles = vec![];

    for i in 0..10 {
        let s = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            s.set_state_json(PROJECT_ID, &format!("key_{}", i), json!(i))
                .await
                .expect("并发写入失败");
        }));
    }

    for h in handles {
        h.await.expect("并发写入任务 panic");
    }

    for i in 0..10 {
        let val = store
            .get_state_json(PROJECT_ID, &format!("key_{}", i))
            .await
            .expect("读取失败")
            .expect("并发写入的键必须全部可读");
        assert_eq!(val, json!(i));
    }
}
