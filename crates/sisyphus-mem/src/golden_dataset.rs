use async_trait::async_trait;
use sisyphus_types::{GoldenDataset, GoldenSop, SOPGraphConfig, TypesError};
use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct InMemoryGoldenDataset {
    data: Arc<RwLock<HashMap<String, GoldenSop>>>,
}

impl InMemoryGoldenDataset {
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl GoldenDataset for InMemoryGoldenDataset {
    async fn enroll(&self, goal: &str, config: SOPGraphConfig, metric: f64) -> Result<(), TypesError> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
            
        let golden = GoldenSop {
            goal: goal.to_string(),
            sop_config: config,
            success_metric: metric,
            timestamp,
        };
        
        self.data.write().insert(goal.to_string(), golden);
        Ok(())
    }

    async fn get_golden_sop(&self, goal: &str) -> Result<Option<GoldenSop>, TypesError> {
        Ok(self.data.read().get(goal).cloned())
    }

    async fn verify_no_regression(&self, goal: &str, _candidate: &SOPGraphConfig, current_metric: f64) -> Result<bool, TypesError> {
        if let Some(golden) = self.data.read().get(goal) {
            // Regression if the current metric is significantly lower than the golden metric
            // For now, we use a simple threshold: 10% degradation
            if current_metric < golden.success_metric * 0.9 {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
