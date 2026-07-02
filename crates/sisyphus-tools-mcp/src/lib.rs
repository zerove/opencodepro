pub mod mcp;
pub mod tool_registry;
pub mod custom_tool;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_info_debug() {
        let info = tool_registry::ToolInfo {
            name: "test".into(),
            description: "desc".into(),
            parameters: None,
            source: tool_registry::ToolSource::Builtin,
        };
        let debug_str = format!("{:?}", info);
        assert!(debug_str.contains("ToolInfo"));
        assert!(debug_str.contains("name: \"test\""));
    }

    #[test]
    fn test_tool_registry_empty() {
        let reg = tool_registry::ToolRegistry::new();
        assert!(reg.is_empty());
    }
}

