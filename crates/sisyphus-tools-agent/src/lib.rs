pub mod agent_registry;
pub mod skill_registry;
pub mod workspace_manager;
pub mod delegate;
pub mod developer;
pub mod jcode_executor;
pub mod project_rules;
pub mod team;

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

