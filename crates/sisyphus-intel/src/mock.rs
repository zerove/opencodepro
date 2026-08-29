use async_trait::async_trait;
use sisyphus_types::{ToolExecutor, TypesError};

/// Mock Executor.
pub struct MockExecutor;

#[async_trait]
impl ToolExecutor for MockExecutor {
    async fn execute(&self, _id: &str, _args: Vec<u8>, _fuel: u64) -> Result<Vec<u8>, TypesError> {
        Ok(b"ok".to_vec())
    }
}
