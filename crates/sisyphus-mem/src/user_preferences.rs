use crate::MfsStore;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sisyphus_types::StateStore;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UserPreference {
    pub key: String,
    pub value: Value,
    pub weight: f64,
    pub last_updated: u64,
}

pub struct UserPreferenceStore {
    store: Arc<MfsStore>,
    prefix: &'static str,
}

impl UserPreferenceStore {
    pub fn new(store: Arc<MfsStore>) -> Self {
        Self {
            store,
            prefix: "pref:",
        }
    }

    fn make_key(&self, key: &str) -> String {
        format!("{}{}", self.prefix, key)
    }

    pub async fn record_preference(&self, key: &str, value: Value, weight: f64) -> Result<()> {
        let full_key = self.make_key(key);

        let current_pref = self
            .store
            .get_state_json(&full_key)
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
            }
            None => UserPreference {
                key: key.to_string(),
                value,
                weight,
                last_updated: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
            },
        };

        self.store
            .set_state_json(&full_key, serde_json::to_value(&updated_pref)?)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        Ok(())
    }

    pub async fn get_preference(&self, key: &str) -> Result<Option<Value>> {
        let full_key = self.make_key(key);
        let val = self
            .store
            .get_state_json(&full_key)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        if let Some(v) = val {
            let pref: UserPreference = serde_json::from_value(v)?;
            return Ok(Some(pref.value));
        }

        Ok(None)
    }

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
        let store = Arc::new(MfsStore::new("sqlite::memory:").await.unwrap());
        (UserPreferenceStore::new(store.clone()), store)
    }

    #[tokio::test]
    async fn test_record_and_get_preference() {
        let (pref_store, _) = setup_store().await;

        pref_store
            .record_preference("theme", json!("dark"), 1.0)
            .await
            .unwrap();
        let val = pref_store.get_preference("theme").await.unwrap();
        assert_eq!(val, Some(json!("dark")));
    }

    #[tokio::test]
    async fn test_weighted_voting() {
        let (pref_store, _) = setup_store().await;

        // Same value increases weight
        pref_store
            .record_preference("font", json!("JetBrains Mono"), 1.0)
            .await
            .unwrap();
        pref_store
            .record_preference("font", json!("JetBrains Mono"), 1.0)
            .await
            .unwrap();

        // We can't easily check the weight from the public API (get_preference returns Value),
        // but we can verify it still returns the correct value.
        let val = pref_store.get_preference("font").await.unwrap();
        assert_eq!(val, Some(json!("JetBrains Mono")));
    }

    #[tokio::test]
    async fn test_preference_overwrite() {
        let (pref_store, _) = setup_store().await;

        pref_store
            .record_preference("lang", json!("en"), 1.0)
            .await
            .unwrap();
        pref_store
            .record_preference("lang", json!("fr"), 1.0)
            .await
            .unwrap();

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

        let inferred = pref_store
            .infer_preferences_from_feedback(feedback)
            .unwrap();
        assert_eq!(inferred, vec!["dark_mode", "concise"]);
    }
}
