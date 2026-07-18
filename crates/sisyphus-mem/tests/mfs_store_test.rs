use serde_json::json;
use sisyphus_mem::MfsStore;
use sisyphus_types::StateStore;
use tokio;

const PROJECT_ID: &str = "test_project";

#[tokio::test]
async fn test_basic_get_set() {
    // Use in-memory DB for speed
    let store = MfsStore::new("sqlite::memory:")
        .await
        .expect("Failed to create store");

    let key = "test_key";
    let value = json!({"foo": "bar"});

    // 1. Test Set
    store
        .set_state_json(PROJECT_ID, key, value.clone())
        .await
        .expect("Set failed");

    // 2. Test Get (L1 hit)
    let retrieved = store
        .get_state_json(PROJECT_ID, key)
        .await
        .expect("Get failed");
    assert_eq!(retrieved, Some(value));

    // 3. Test non-existent key
    let none = store
        .get_state_json(PROJECT_ID, "ghost")
        .await
        .expect("Get failed");
    assert_eq!(none, None);
}

#[tokio::test]
async fn test_persistence() {
    let db_path = format!("sisyphus_test_{}.db", std::process::id());
    let db_url = format!("sqlite:{}", db_path);

    println!("Testing persistence with DB URL: {}", db_url);

    // Ensure clean start
    let _ = std::fs::remove_file(&db_path);

    {
        let store = MfsStore::new(&db_url)
            .await
            .expect("Failed to create store");
        let key = "persist_key";
        let value = json!({"status": "saved"});

        store
            .set_state_json(PROJECT_ID, key, value.clone())
            .await
            .expect("Set failed");
        store.flush().await.expect("Flush failed");
    }

    // Re-open store to verify persistence
    {
        let store = MfsStore::new(&db_url)
            .await
            .expect("Failed to create store");
        let key = "persist_key";
        let retrieved = store
            .get_state_json(PROJECT_ID, key)
            .await
            .expect("Get failed");
        assert_eq!(retrieved, Some(json!({"status": "saved"})));
    }

    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn test_snapshot_restore() {
    let store = MfsStore::new("sqlite::memory:")
        .await
        .expect("Failed to create store");

    // Setup initial state
    store
        .set_state_json(PROJECT_ID, "k1", json!("v1"))
        .await
        .unwrap();
    store
        .set_state_json(PROJECT_ID, "k2", json!("v2"))
        .await
        .unwrap();
    store.flush().await.unwrap();

    // Take snapshot
    let snapshot = store.take_snapshot().expect("Snapshot failed");
    assert_eq!(snapshot.data.len(), 2);

    // Modify state
    store
        .set_state_json(PROJECT_ID, "k1", json!("v1_new"))
        .await
        .unwrap();
    store
        .set_state_json(PROJECT_ID, "k3", json!("v3"))
        .await
        .unwrap();
    store.flush().await.unwrap();

    // Restore
    store
        .restore_snapshot(snapshot)
        .await
        .expect("Restore failed");

    // Verify
    assert_eq!(
        store.get_state_json(PROJECT_ID, "k1").await.unwrap(),
        Some(json!("v1"))
    );
    assert_eq!(
        store.get_state_json(PROJECT_ID, "k2").await.unwrap(),
        Some(json!("v2"))
    );
    assert_eq!(store.get_state_json(PROJECT_ID, "k3").await.unwrap(), None);
}

#[tokio::test]
async fn test_prefix_listing() {
    let store = MfsStore::new("sqlite::memory:")
        .await
        .expect("Failed to create store");

    store
        .set_state_json(PROJECT_ID, "user.1.name", json!("Alice"))
        .await
        .unwrap();
    store
        .set_state_json(PROJECT_ID, "user.2.name", json!("Bob"))
        .await
        .unwrap();
    store
        .set_state_json(PROJECT_ID, "sys.config", json!("on"))
        .await
        .unwrap();
    store.flush().await.unwrap();

    let user_keys = store
        .list_keys_with_prefix(Some(PROJECT_ID), "user")
        .await
        .expect("List failed");
    assert_eq!(user_keys.len(), 2);
    assert!(user_keys.contains(&"user.1.name".to_string()));
    assert!(user_keys.contains(&"user.2.name".to_string()));

    let sys_keys = store
        .list_keys_with_prefix(Some(PROJECT_ID), "sys")
        .await
        .expect("List failed");
    assert_eq!(sys_keys.len(), 1);
    assert_eq!(sys_keys[0], "sys.config");
}

#[tokio::test]
async fn test_concurrent_access() {
    let store = std::sync::Arc::new(
        MfsStore::new("sqlite::memory:")
            .await
            .expect("Failed to create store"),
    );
    let mut handles = Vec::new();

    for i in 0..10 {
        let store_clone = store.clone();
        handles.push(tokio::spawn(async move {
            let key = format!("key_{}", i);
            let val = json!(i);
            store_clone
                .set_state_json(PROJECT_ID, &key, val)
                .await
                .unwrap();
            let res = store_clone.get_state_json(PROJECT_ID, &key).await.unwrap();
            assert_eq!(res, Some(json!(i)));
        }));
    }

    for h in handles {
        h.await.expect("Task panicked");
    }

    store.flush().await.unwrap();
    assert_eq!(store.len(), 10);
}
