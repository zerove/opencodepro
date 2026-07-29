use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub tool_id: String,
    pub required_capability: String,
    pub expected_input: serde_json::Value,
    pub expected_output: serde_json::Value,
    pub success_criteria: String,
    /// Optional command to run for testing this tool.
    #[serde(default)]
    pub test_command: Option<String>,
    /// Optional working directory for tool execution.
    #[serde(default)]
    pub workdir: Option<String>,
}
