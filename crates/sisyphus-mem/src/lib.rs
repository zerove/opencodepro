use anyhow::{Context, Result};
use async_trait::async_trait;
use dashmap::DashMap;
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
use std::sync::Arc;

pub struct Snapshot {
    data: std::collections::HashMap<String, serde_json::Value>,
}

pub struct MfsStore {
    l1: Arc<DashMap<String, serde_json::Value>>,
    l2: SqlitePool,
}

impl MfsStore {
    /// Creates a new MfsStore.
    /// `db_url` should be a sqlite connection string (e.g., "sqlite:state.db").
    pub async fn new(db_url: &str) -> Result<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(db_url)
            .await
            .context("Failed to connect to SQLite pool")?;

        // Enable WAL mode for better concurrency
        sqlx::query("PRAGMA journal_mode=WAL;")
            .execute(&pool)
            .await?;
        sqlx::query("PRAGMA synchronous=NORMAL;")
            .execute(&pool)
            .await?;

        // Initialize state table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS state (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
        )
        .execute(&pool)
        .await?;

        Ok(Self {
            l1: Arc::new(DashMap::new()),
            l2: pool,
        })
    }

    /// Captures a point-in-time snapshot of the current state.
    pub async fn take_snapshot(&self) -> Result<Snapshot> {
        let mut data = std::collections::HashMap::new();
        for entry in self.l1.iter() {
            data.insert(entry.key().clone(), entry.value().clone());
        }
        Ok(Snapshot { data })
    }

    /// Restores the state from a given snapshot.
    pub async fn restore_snapshot(&self, snapshot: Snapshot) -> Result<()> {
        // 1. Clear L1
        self.l1.clear();

        // 2. Clear L2
        sqlx::query("DELETE FROM state").execute(&self.l2).await?;

        // 3. Restore from snapshot
        for (key, value) in snapshot.data {
            let value_str = serde_json::to_string(&value)?;
            sqlx::query("INSERT INTO state (key, value) VALUES (?, ?)")
                .bind(&key)
                .bind(&value_str)
                .execute(&self.l2)
                .await?;
            self.l1.insert(key, value);
        }

        Ok(())
    }

    /// Internal method to load all state from L2 into L1.
    #[allow(dead_code)]
    async fn warm_up(&self) -> Result<()> {
        let rows: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM state")
            .fetch_all(&self.l2)
            .await?;

        for (key, value) in rows {
            if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&value) {
                self.l1.insert(key, json_val);
            }
        }
        Ok(())
    }
}

#[async_trait]
impl sisyphus_types::StateStore for MfsStore {
    async fn get_state_json(&self, key: &str) -> Result<Option<serde_json::Value>, String> {
        // 1. Try L1
        if let Some(val) = self.l1.get(key) {
            return Ok(Some(val.clone()));
        }

        // 2. Try L2
        let row: Option<(String, String)> =
            sqlx::query_as("SELECT key, value FROM state WHERE key = ?")
                .bind(key)
                .fetch_optional(&self.l2)
                .await
                .map_err(|e| e.to_string())?;

        if let Some((_, value_str)) = row {
            let json_val: serde_json::Value =
                serde_json::from_str(&value_str).map_err(|e| e.to_string())?;
            // Update L1
            self.l1.insert(key.to_string(), json_val.clone());
            return Ok(Some(json_val));
        }

        Ok(None)
    }

    async fn set_state_json(&self, key: &str, value: serde_json::Value) -> Result<(), String> {
        let value_str = serde_json::to_string(&value).map_err(|e| e.to_string())?;

        // Write-Through: Update L2 first to ensure persistence
        sqlx::query(
            "INSERT INTO state (key, value) VALUES (?, ?) 
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(&value_str)
        .execute(&self.l2)
        .await
        .map_err(|e| e.to_string())?;

        // Update L1
        self.l1.insert(key.to_string(), value);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use sisyphus_types::StateStore;

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

        let snap = store.take_snapshot().await.unwrap();

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

        let snapshot = store.take_snapshot().await.unwrap();

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
}
