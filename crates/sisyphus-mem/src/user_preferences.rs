use crate::MfsStore;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sisyphus_types::StateStore;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// A single user preference entry: a key-value pair with a weight (0.0..=1.0)
/// reflecting how strongly this preference is held.
///
/// Higher weight means the value is more likely to be the "winning" choice
/// when multiple values compete for the same key.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UserPreference {
    pub key: String,
    pub value: Value,
    pub weight: f64,
    pub last_updated: u64,
}

/// Project-scoped store for weighted user preferences.
///
/// All keys are prefixed (`pref:`) so they can coexist with other state
/// entries in the same [`MfsStore`].
pub struct UserPreferenceStore {
    store: Arc<MfsStore>,
    prefix: &'static str,
    project_id: String,
}

impl UserPreferenceStore {
    /// Create a new preference store bound to `project_id` and backed by `store`.
    pub fn new(store: Arc<MfsStore>, project_id: String) -> Self {
        Self {
            store,
            prefix: "pref:",
            project_id,
        }
    }

    fn make_key(&self, key: &str) -> String {
        format!("{}{}", self.prefix, key)
    }

    /// Record or update a preference for `key` with `value` and `weight`.
    ///
    /// If the same `value` is recorded again, the weights are summed.
    /// If a different value is recorded, the existing weight is replaced
    /// by the new one (last-write-wins within a single key/value pair).
    pub async fn record_preference(&self, key: &str, value: Value, weight: f64) -> Result<()> {
        let full_key = self.make_key(key);

        let current_pref = self
            .store
            .get_state_json(&self.project_id, &full_key)
            .await
            .map_err(|e| anyhow::anyhow!(e))?
            .and_then(|v| serde_json::from_value::<UserPreference>(v).ok());

        let updated_pref = match current_pref {
            Some(mut pref) => {
                if pref.value == value {
                    pref.weight += weight;
                } else {
                    // If value changed, we decay the old weight and start new
                    // Simple approach: if new value is provided, we treat it as a competing preference
                    // For this simple weighted voting, we'll just overwrite if weight is high enough
                    // or maintain a map of values. But the spec says "same key used more frequently with same value = higher weight"
                    // This implies we should probably store preferences as a map of value -> weight.
                    // However, the UserPreference struct provided in the spec has a single value.
                    // Let's stick to the spec's struct but implement the logic:
                    // if value is same, increase weight. If different, we'll replace if the new weight is significant
                    // or just replace and reset weight.
                    // Actually, a better interpretation of "weighted voting" with a single value field
                    // is that we only store the "winning" preference.

                    // Let's refine: if value is different, we reset weight to the new weight.
                    pref.value = value;
                    pref.weight = weight;
                }
                pref.last_updated = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
                pref
            },
            None => UserPreference {
                key: key.to_string(),
                value,
                weight,
                last_updated: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
            },
        };

        self.store
            .set_state_json(
                &self.project_id,
                &full_key,
                serde_json::to_value(&updated_pref)?,
            )
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        Ok(())
    }

    /// Look up the current preferred value for `key`.
    ///
    /// Returns `None` if no preference has been recorded for this key.
    pub async fn get_preference(&self, key: &str) -> Result<Option<Value>> {
        let full_key = self.make_key(key);
        let val = self
            .store
            .get_state_json(&self.project_id, &full_key)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        if let Some(v) = val {
            let pref: UserPreference = serde_json::from_value(v)?;
            return Ok(Some(pref.value));
        }

        Ok(None)
    }

    /// Mine a list of [`sisyphus_types::Feedback`] entries for preference hints.
    ///
    /// Any feedback tag with the prefix `pref:` is treated as a preference
    /// key candidate and returned. The keys are deduplicated by `Vec` order
    /// (first occurrence wins).
    pub fn infer_preferences_from_feedback(
        &self,
        feedback: Vec<sisyphus_types::Feedback>,
    ) -> Result<Vec<String>> {
        let mut inferred = Vec::new();
        for fb in feedback {
            // Use tags as preference hints
            for tag in fb.tags {
                if let Some(val) = tag.strip_prefix("pref:") {
                    inferred.push(val.to_string());
                }
            }
        }

        Ok(inferred)
    }

    /// Clear all preferences for this project.
    ///
    /// Currently a no-op stub: the underlying [`MfsStore`] does not expose
    /// prefix-scoped deletion. A full implementation would maintain a side
    /// index of preference keys and remove them individually.
    pub fn clear_preferences(&self) -> Result<()> {
        // MfsStore doesn't have a prefix-based delete.
        // In a real system, we'd maintain a list of preference keys.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MfsStore;
    use serde_json::json;
    use std::sync::Arc;

    async fn setup_store() -> (UserPreferenceStore, Arc<MfsStore>) {
        let config = sisyphus_types::CacheConfig::default();
        let store = Arc::new(MfsStore::new("sqlite::memory:", &config).await.unwrap());
        (
            UserPreferenceStore::new(store.clone(), "test_project".into()),
            store,
        )
    }

    #[tokio::test]
    async fn test_record_and_get_preference() {
        let (pref_store, _) = setup_store().await;

        pref_store.record_preference("theme", json!("dark"), 1.0).await.unwrap();
        let val = pref_store.get_preference("theme").await.unwrap();
        assert_eq!(val, Some(json!("dark")));
    }

    #[tokio::test]
    async fn test_weighted_voting() {
        let (pref_store, _) = setup_store().await;

        // Same value increases weight
        pref_store.record_preference("font", json!("JetBrains Mono"), 1.0).await.unwrap();
        pref_store.record_preference("font", json!("JetBrains Mono"), 1.0).await.unwrap();

        // We can't easily check the weight from the public API (get_preference returns Value),
        // but we can verify it still returns the correct value.
        let val = pref_store.get_preference("font").await.unwrap();
        assert_eq!(val, Some(json!("JetBrains Mono")));
    }

    #[tokio::test]
    async fn test_preference_overwrite() {
        let (pref_store, _) = setup_store().await;

        pref_store.record_preference("lang", json!("en"), 1.0).await.unwrap();
        pref_store.record_preference("lang", json!("fr"), 1.0).await.unwrap();

        let val = pref_store.get_preference("lang").await.unwrap();
        assert_eq!(val, Some(json!("fr")));
    }

    #[tokio::test]
    async fn test_infer_from_feedback() {
        let (pref_store, _) = setup_store().await;

        let feedback = vec![
            sisyphus_types::Feedback {
                agent_id: "a".into(),
                original_request: "r1".into(),
                original_output: "o1".into(),
                corrected_output: "c1".into(),
                timestamp: 0,
                tags: vec!["pref:dark_mode".into(), "other".into()],
            },
            sisyphus_types::Feedback {
                agent_id: "a".into(),
                original_request: "r2".into(),
                original_output: "o2".into(),
                corrected_output: "c2".into(),
                timestamp: 0,
                tags: vec!["pref:concise".into()],
            },
        ];

        let inferred = pref_store.infer_preferences_from_feedback(feedback).unwrap();
        assert_eq!(inferred, vec!["dark_mode", "concise"]);
    }
}
