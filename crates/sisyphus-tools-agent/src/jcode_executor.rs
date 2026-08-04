use async_trait::async_trait;
use sisyphus_types::{ToolExecutor, TypesError};
use std::sync::{Arc, Mutex};

/// Trait for calling a tool on the Jcode platform.
#[async_trait]
pub trait JcodeToolCaller: Send + Sync {
    async fn call_tool(&self, tool_id: &str, args: Vec<u8>) -> Result<Vec<u8>, String>;
}

/// An executor that delegates tasks to the Jcode platform.
/// It uses a lazy caller to break circular dependencies during initialization.
pub struct JcodeToolExecutor {
    caller: Arc<Mutex<Option<Arc<dyn JcodeToolCaller>>>>,
}

impl JcodeToolExecutor {
    pub fn new() -> Self {
        Self {
            caller: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set_caller(&self, caller: Arc<dyn JcodeToolCaller>) {
        // 锁中毒时静默跳过：调用者设置失败不应导致 panic
        if let Ok(mut lock) = self.caller.lock() {
            *lock = Some(caller);
        } else {
            tracing::error!("Jcode caller mutex poisoned, ignoring set_caller");
        }
    }
}

impl Default for JcodeToolExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolExecutor for JcodeToolExecutor {
    async fn execute(
        &self,
        tool_id: &str,
        args: Vec<u8>,
        _fuel_limit: u64,
    ) -> Result<Vec<u8>, TypesError> {
        let caller = match self.caller.lock() {
            Ok(lock) => lock.as_ref().cloned(),
            Err(_) => {
                return Err(TypesError::NodeExecutionError(
                    "Jcode caller mutex poisoned".into(),
                ))
            }
        };

        match caller {
            Some(c) => match c.call_tool(tool_id, args).await {
                Ok(result) => Ok(result),
                Err(e) => Err(TypesError::NodeExecutionError(e)),
            },
            None => Err(TypesError::NodeExecutionError(
                "Jcode caller not yet initialized".into(),
            )),
        }
    }
}
