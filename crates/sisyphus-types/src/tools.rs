use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub tool_id: String,
    pub required_capability: String,
    pub expected_input: serde_json::Value,
    pub expected_output: serde_json::Value,
    pub success_criteria: String,
}
