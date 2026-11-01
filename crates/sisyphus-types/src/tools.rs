use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Tool Spec.
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

/// 预置的常用本地工具模板库（用于加速标准工具的自举）。
///
/// 这些模板是纯数据（`ToolSpec`），因此放在依赖根 `sisyphus-types` 中，
/// 让 CLI（不依赖 `sisyphus-core`）也能列出与校验它们。
pub struct PresetToolTemplates;

impl PresetToolTemplates {
    /// 所有内置预置模板 ID（用于 CLI 列表展示与测试枚举）。
    pub const ALL_IDS: &'static [&'static str] = &["local/log_analyzer", "local/json_formatter"];

    /// 按 ID 获取预置工具规格。
    pub fn get_preset(tool_id: &str) -> Option<ToolSpec> {
        match tool_id {
            "local/log_analyzer" => Some(ToolSpec {
                tool_id: "local/log_analyzer".into(),
                required_capability: "Parse log files and aggregate error frequency".into(),
                expected_input: serde_json::json!({"log_path": "string"}),
                expected_output: serde_json::json!({"error_counts": "object"}),
                success_criteria: "Returns valid JSON object with error frequency stats".into(),
                test_command: Some("cargo test -p sisyphus-core".into()),
                workdir: None,
            }),
            "local/json_formatter" => Some(ToolSpec {
                tool_id: "local/json_formatter".into(),
                required_capability: "Pretty-print and validate JSON content".into(),
                expected_input: serde_json::json!({"raw": "string"}),
                expected_output: serde_json::json!({"formatted": "string", "valid": "bool"}),
                success_criteria: "Validates JSON syntax and formats output".into(),
                test_command: Some("cargo test -p sisyphus-core".into()),
                workdir: None,
            }),
            _ => None,
        }
    }

    /// 所有预置模板的完整规格（顺序与 [`Self::ALL_IDS`] 一致）。
    pub fn all() -> Vec<ToolSpec> {
        Self::ALL_IDS.iter().filter_map(|id| Self::get_preset(id)).collect()
    }
}

#[cfg(test)]
mod preset_tests {
    use super::*;

    #[test]
    fn all_ids_resolve_to_specs_with_matching_tool_id() {
        let specs = PresetToolTemplates::all();
        assert_eq!(specs.len(), PresetToolTemplates::ALL_IDS.len());
        for (spec, id) in specs.iter().zip(PresetToolTemplates::ALL_IDS.iter()) {
            assert_eq!(&spec.tool_id, id, "spec tool_id must match the listed id");
            assert!(!spec.required_capability.is_empty());
            assert!(!spec.success_criteria.is_empty());
        }
    }

    #[test]
    fn unknown_preset_returns_none() {
        assert!(PresetToolTemplates::get_preset("local/does_not_exist").is_none());
        assert!(PresetToolTemplates::get_preset("").is_none());
    }

    #[test]
    fn preset_specs_are_self_consistent() {
        for spec in PresetToolTemplates::all() {
            // 输入/输出契约必须是对象，便于下游按字段取参数
            assert!(spec.expected_input.is_object(), "{}", spec.tool_id);
            assert!(spec.expected_output.is_object(), "{}", spec.tool_id);
        }
    }
}
