#[cfg(test)]
mod tests {
    use crate::store::MfsStore;
    use sisyphus_types::StateStore;
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use std::num::NonZeroUsize;
    use std::sync::Arc;

    #[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
    struct TestState {
        count: i32,
        name: String,
    }

    async fn setup_store() -> (MfsStore, String) {
        let db_path = ":memory:".to_string();
        let store = MfsStore::new(&format!("sqlite:{}", db_path)).await.unwrap();
        (store, db_path)
    }

    #[tokio::test]
    async fn test_basic_set_get() {
        let (store, _) = setup_store().await;
        let state = TestState {
            count: 42,
            name: "Sisyphus".to_string(),
        };
        let key = "user_1";

        store
            .set_state_json(key, serde_json::to_value(&state).unwrap())
            .await
            .unwrap();
        let retrieved_val = store.get_state_json(key).await.unwrap().unwrap();
        let retrieved: TestState = serde_json::from_value(retrieved_val).unwrap();

        assert_eq!(retrieved, state);
    }

    #[tokio::test]
    async fn test_get_missing_key() {
        let (store, _) = setup_store().await;
        let val = store.get_state_json("nonexistent").await.unwrap();
        assert_eq!(val, None);
    }

    #[tokio::test]
    async fn test_overwrite_value() {
        let (store, _) = setup_store().await;
        store.set_state_json("k", json!("first")).await.unwrap();
        store.set_state_json("k", json!("second")).await.unwrap();
        let val = store.get_state_json("k").await.unwrap().unwrap();
        assert_eq!(val, json!("second"));
    }

    #[tokio::test]
    async fn test_update_then_revert() {
        let (store, _) = setup_store().await;

        store.set_state_json("k", json!("v1")).await.unwrap();

        let snap = store.take_snapshot().unwrap();

        store.set_state_json("k", json!("v2")).await.unwrap();
        assert_eq!(
            store.get_state_json("k").await.unwrap().unwrap(),
            json!("v2")
        );

        store.restore_snapshot(snap).await.unwrap();
        assert_eq!(
            store.get_state_json("k").await.unwrap().unwrap(),
            json!("v1")
        );
    }

    #[tokio::test]
    async fn test_snapshot_restore() {
        let (store, _) = setup_store().await;

        store.set_state_json("k1", json!(1)).await.unwrap();
        store.set_state_json("k2", json!(2)).await.unwrap();

        let snapshot = store.take_snapshot().unwrap();

        store.set_state_json("k1", json!(10)).await.unwrap();
        store.set_state_json("k3", json!(3)).await.unwrap();

        store.restore_snapshot(snapshot).await.unwrap();

        let v1 = store.get_state_json("k1").await.unwrap().unwrap();
        let v2 = store.get_state_json("k2").await.unwrap().unwrap();
        let v3 = store.get_state_json("k3").await.unwrap();

        assert_eq!(v1, json!(1));
        assert_eq!(v2, json!(2));
        assert_eq!(v3, None);
    }

    #[tokio::test]
    async fn test_concurrent_access() {
        let (store, _) = setup_store().await;
        let store = Arc::new(store);
        let mut handles = vec![];

        for i in 0..10 {
            let s = Arc::clone(&store);
            handles.push(tokio::spawn(async move {
                s.set_state_json(&format!("key_{}", i), json!(i))
                    .await
                    .unwrap();
            }));
        }

        for h in handles {
            h.await.unwrap();
        }

        for i in 0..10 {
            let val = store
                .get_state_json(&format!("key_{}", i))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(val, json!(i));
        }
    }

    #[tokio::test]
    async fn test_lru_eviction() {
        let db_path = ":memory:".to_string();
        let store = MfsStore::with_capacity(
            &format!("sqlite:{}", db_path),
            NonZeroUsize::new(3).unwrap(),
        )
        .await
        .unwrap();

        // Insert 4 items with capacity 3 - L1 must stay bounded at 3
        store.set_state_json("a", json!(1)).await.unwrap();
        store.set_state_json("b", json!(2)).await.unwrap();
        store.set_state_json("c", json!(3)).await.unwrap();
        assert_eq!(store.len(), 3, "L1 cache should hold 3 items");

        store.set_state_json("d", json!(4)).await.unwrap();
        assert_eq!(
            store.len(),
            3,
            "L1 cache must not exceed capacity after eviction"
        );

        // Force flush to ensure evicted items are persisted to L2
        store.flush().await.unwrap();

        // All data still accessible via L2 fallback (write-through persistence)
        assert_eq!(
            store.get_state_json("a").await.unwrap(),
            Some(json!(1)),
            "L2 fallback should serve evicted keys"
        );
        assert_eq!(store.get_state_json("b").await.unwrap(), Some(json!(2)), "");
        assert_eq!(store.get_state_json("c").await.unwrap(), Some(json!(3)), "");
        assert_eq!(store.get_state_json("d").await.unwrap(), Some(json!(4)), "");

        // After fetching from L2, L1 is re-populated; make sure it stays bounded
        assert_eq!(
            store.len(),
            3,
            "L1 cache should still be bounded after L2 fallback reads"
        );
    }
}
