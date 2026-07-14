use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolContext {
    pub tool_id: String,
    pub input: Value,
    pub state: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    pub result: Value,
    pub state_update: Option<Value>,
}

impl ToolOutput {
    pub fn json(val: Value) -> Self {
        Self {
            result: val,
            state_update: None,
        }
    }
}

pub trait SisyphusTool: Send + Sync {
    fn execute(&self, ctx: &ToolContext) -> Result<ToolOutput, String>;
}

pub fn register_tool<T: SisyphusTool + 'static>(tool: T) {
    // In a real implementation, this would register the tool with the WASM host.
    // For the SDK, it's just a marker.
}
