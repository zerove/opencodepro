use serde_json::json;
use sisyphus_mem::MfsStore;
use sisyphus_types::StateStore;

const PROJECT_ID: &str = "test_project";

#[tokio::test]
async fn test_basic_get_set() {
    // Use in-memory DB for speed
    let store = MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
        .await
        .expect("Failed to create store");

    let key = "test_key";
    let value = json!({"foo": "bar"});

    // 1. Test Set
    store.set_state_json(PROJECT_ID, key, value.clone()).await.expect("Set failed");

    // 2. Test Get (L1 hit)
    let retrieved = store.get_state_json(PROJECT_ID, key).await.expect("Get failed");
    assert_eq!(retrieved, Some(value));

    // 3. Test non-existent key
    let none = store.get_state_json(PROJECT_ID, "ghost").await.expect("Get failed");
    assert_eq!(none, None);
}

#[tokio::test]
async fn test_persistence() {
    let db_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!("sisyphus_test_{}.db", std::process::id()))
        .canonicalize()
        .unwrap_or_else(|_| {
            // If canonicalize fails (file doesn't exist yet), resolve parent first
            let parent = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target")
                .canonicalize()
                .expect("target dir must exist");
            parent.join(format!("sisyphus_test_{}.db", std::process::id()))
        });
    let db_url = format!("sqlite:{}", db_path.to_str().unwrap());

    println!("Testing persistence with DB URL: {}", db_url);

    // Ensure clean start
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(format!("{}-wal", db_path.to_str().unwrap()));
    let _ = std::fs::remove_file(format!("{}-shm", db_path.to_str().unwrap()));

    {
        let store = MfsStore::new(&db_url, &sisyphus_types::CacheConfig::default())
            .await
            .expect("Failed to create store");
        let key = "persist_key";
        let value = json!({"status": "saved"});

        store.set_state_json(PROJECT_ID, key, value.clone()).await.expect("Set failed");
        store.flush().await.expect("Flush failed");
    }

    // Re-open store to verify persistence
    {
        let store = MfsStore::new(&db_url, &sisyphus_types::CacheConfig::default())
            .await
            .expect("Failed to create store");
        let key = "persist_key";
        let retrieved = store.get_state_json(PROJECT_ID, key).await.expect("Get failed");
        assert_eq!(retrieved, Some(json!({"status": "saved"})));
    }

    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn test_snapshot_restore() {
    let store = MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
        .await
        .expect("Failed to create store");

    // Setup initial state
    store.set_state_json(PROJECT_ID, "k1", json!("v1")).await.unwrap();
    store.set_state_json(PROJECT_ID, "k2", json!("v2")).await.unwrap();
    store.flush().await.unwrap();

    // Take snapshot
    let snapshot = store.take_snapshot().await.expect("Snapshot failed");
    assert_eq!(snapshot.data.len(), 2);

    // Modify state
    store.set_state_json(PROJECT_ID, "k1", json!("v1_new")).await.unwrap();
    store.set_state_json(PROJECT_ID, "k3", json!("v3")).await.unwrap();
    store.flush().await.unwrap();

    // Restore
    store.restore_snapshot(snapshot).await.expect("Restore failed");

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
    let store = MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
        .await
        .expect("Failed to create store");

    store.set_state_json(PROJECT_ID, "user.1.name", json!("Alice")).await.unwrap();
    store.set_state_json(PROJECT_ID, "user.2.name", json!("Bob")).await.unwrap();
    store.set_state_json(PROJECT_ID, "sys.config", json!("on")).await.unwrap();
    store.flush().await.unwrap();

    let user_keys =
        store.list_keys_with_prefix(Some(PROJECT_ID), "user").await.expect("List failed");
    assert_eq!(user_keys.len(), 2);
    assert!(user_keys.contains(&"user.1.name".to_string()));
    assert!(user_keys.contains(&"user.2.name".to_string()));

    let sys_keys = store.list_keys_with_prefix(Some(PROJECT_ID), "sys").await.expect("List failed");
    assert_eq!(sys_keys.len(), 1);
    assert_eq!(sys_keys[0], "sys.config");
}

#[tokio::test]
async fn test_concurrent_access() {
    let store = std::sync::Arc::new(
        MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
            .await
            .expect("Failed to create store"),
    );
    let mut handles = Vec::new();

    for i in 0..10 {
        let store_clone = store.clone();
        handles.push(tokio::spawn(async move {
            let key = format!("key_{}", i);
            let val = json!(i);
            store_clone.set_state_json(PROJECT_ID, &key, val).await.unwrap();
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

/// keyset 扫描：严格大于游标、按主键升序、受 limit 限制。
///
/// 反熵分页（ADR-012）依赖这三条：`>` 决定不重发，升序决定游标可推进，
/// limit 决定单页有界。任何一条错了都会表现为「翻页重复」或「尾部丢行」。
#[tokio::test]
async fn experience_keys_after_is_keyset_ordered_and_bounded() {
    let store = MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
        .await
        .expect("Failed to create store");

    for i in 1..=5u64 {
        let exp = sisyphus_types::experience::ExperienceTuple {
            context_text: format!("ctx-{i}"),
            context_hash: format!("hash-{i}"),
            action_id: "act".into(),
            project_id: None,
            outcome_success: true,
            deviation_score: 0.0,
            cost: i,
            timestamp: 1_700_000_000 + i,
            embedding: None,
            visual_embedding: None,
            insight: None,
            version: i,
            vector_clock: Default::default(),
        };
        store
            .record_experience(&exp, &format!("exp:170000000{i}:node-a:{i}"))
            .await
            .expect("record experience");
    }

    assert_eq!(
        store.experience_keys_after("", 3).await.expect("first page"),
        vec![
            "exp:1700000001:node-a:1".to_string(),
            "exp:1700000002:node-a:2".to_string(),
            "exp:1700000003:node-a:3".to_string(),
        ],
        "首页应按主键升序、受 limit 限制"
    );
    assert_eq!(
        store.experience_keys_after("exp:1700000003:node-a:3", 2).await.expect("second page"),
        vec![
            "exp:1700000004:node-a:4".to_string(),
            "exp:1700000005:node-a:5".to_string(),
        ],
        "游标必须严格排除（不重发已送达的键）"
    );
    assert!(
        store.experience_keys_after("exp:1700000005:node-a:5", 10).await.expect("tail").is_empty(),
        "游标之后无键时应为空"
    );
    assert!(
        store.experience_keys_after("", 0).await.expect("zero limit").is_empty(),
        "limit=0 不应返回任何键"
    );
}
