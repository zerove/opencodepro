// `double_must_use`：`#[async_trait]` 会给 trait 的 async 方法加 `#[must_use]`，
// 而其返回类型（boxed Future）本身已带 `#[must_use]`，clippy 1.99.0 起判为冗余。
// 本 crate 的 trait 经由该宏生成，无法逐个方法挂 allow。
#![allow(clippy::double_must_use)]

pub mod agent_registry;
pub mod delegate;
pub mod developer;
pub mod jcode_executor;
pub mod project_rules;
pub mod skill_registry;
pub mod team;
pub mod team_config;
pub mod workspace_manager;

#[cfg(test)]
mod tests {
    use super::*;
    use agent_registry::{AgentInfo, AgentRegistry};

    #[test]
    fn test_agent_info_creation() {
        let info = AgentInfo {
            display_name: "Test Agent".into(),
            description: "A test agent".into(),
            tool_prefixes: vec!["test/".into()],
            max_concurrency: 1,
        };
        assert_eq!(info.display_name, "Test Agent");
    }

    #[test]
    fn test_agent_registry_type() {
        fn assert_type<T: 'static>() {}
        assert_type::<AgentRegistry>();
    }
}
