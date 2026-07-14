use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use sisyphus_types::{ToolExecutor, TypesError};

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
        let mut lock = self.caller.lock().unwrap();
        *lock = Some(caller);
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
        let caller = {
            let lock = self.caller.lock().unwrap();
            lock.as_ref().cloned()
        };

        match caller {
            Some(c) => match c.call_tool(tool_id, args).await {
                Ok(result) => Ok(result),
                Err(e) => Err(TypesError::NodeExecutionError(e)),
            },
            None => Err(TypesError::NodeExecutionError("Jcode caller not yet initialized".into())),
        }
    }
}
