// `double_must_use`：`#[async_trait]` 会给 trait 的 async 方法加 `#[must_use]`，
// 而其返回类型（boxed Future）本身已带 `#[must_use]`，clippy 1.99.0 起判为冗余。
// 本 crate 的 trait 经由该宏生成，无法逐个方法挂 allow。
#![allow(clippy::double_must_use)]

pub mod custom_tool;
pub mod mcp;
pub mod tool_registry;

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
