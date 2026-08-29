//! 内核计划缓存：短期执行计划的记忆化。
//!
//! `SisyphusKernel` 实现的一部分，为可读性从 `kernel.rs` 拆出。
//! 通过 `pub use kernel::*` 重新导出。

use crate::*;

impl SisyphusKernel {
    pub async fn cache_plan(
        &self,
        plan_id: &str,
        plan: &sisyphus_types::CognitivePlan,
    ) -> Result<(), CoreError> {
        let json = serde_json::to_value(plan).map_err(|e| CoreError::Internal(e.to_string()))?;

        self.execution_manager
            .state_store
            .set_state_json("sys", &format!("plan_cache:{}", plan_id), json)
            .await
            .map_err(|e| CoreError::Internal(e.to_string()))
    }

    /// Get cached plan.
    pub async fn get_cached_plan(
        &self,
        plan_id: &str,
    ) -> Result<Option<sisyphus_types::CognitivePlan>, CoreError> {
        let val = self
            .execution_manager
            .state_store
            .get_state_json("sys", &format!("plan_cache:{}", plan_id))
            .await
            .map_err(|e| CoreError::Internal(e.to_string()))?;

        if let Some(v) = val {
            let plan = serde_json::from_value(v).map_err(|e| CoreError::Internal(e.to_string()))?;

            Ok(Some(plan))
        } else {
            Ok(None)
        }
    }
}
