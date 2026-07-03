use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use std::path::Path;

use sisyphus_types::{ToolExecutor, TypesError};
use crate::{bash, file};

/// A specialized executor for system-level operations (File I/O, Shell commands).
pub struct SystemExecutor;

#[async_trait]
impl ToolExecutor for SystemExecutor {
    async fn execute(
        &self,
        tool_id: &str,
        args: Vec<u8>,
        _fuel_limit: u64,
    ) -> Result<Vec<u8>, TypesError> {
        let args_str = String::from_utf8_lossy(&args).into_owned();
        
        match tool_id {
            "read" => {
                let path = Path::new(&args_str);
                let content = file::read(path).map_err(|e| TypesError::NodeExecutionError(e.to_string()))?;
                Ok(content.into_bytes())
            }
            "write" => {
                // For write, we expect args to be "path|content"
                let parts: Vec<&str> = args_str.splitn(2, '|').collect();
                if parts.len() < 2 {
                    return Err(TypesError::NodeExecutionError("Write requires 'path|content' args".into()));
                }
                let path = Path::new(parts[0]);
                let content = parts[1];
                file::write(path, content).map_err(|e| TypesError::NodeExecutionError(e.to_string()))?;
                Ok(b"Success".to_vec())
            }
            "shell" => {
                let output = bash::execute(&args_str, None, 5000)
                    .await
                    .map_err(|e| TypesError::NodeExecutionError(e.to_string()))?;
                Ok(output.stdout.into_bytes())
            }
            _ => Err(TypesError::NodeExecutionError(format!("Unknown system tool: {tool_id}"))),
        }
    }
}

impl Default for SystemExecutor {
    fn default() -> Self {
        Self
    }
}
