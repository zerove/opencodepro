pub mod agent_channel {
    pub use sisyphus_types::AgentChannel;
}
pub use sisyphus_types::AgentChannel;
pub mod bash;
pub mod config_store;
pub mod dep_manager;
pub mod explore;
pub mod file;
pub mod permissions;
pub mod scaffold;
pub mod system_executor;
pub mod web;
pub mod wisdom;

#[cfg(test)]
mod tests {
    use crate::config_store::ConfigStore;
    use crate::permissions::PermissionChecker;
    use sisyphus_tools_core::PermissionRules;
    use sisyphus_types::Config;
    use std::sync::{Arc, RwLock};

    #[test]
    fn test_config_store_basic() {
        let config = Arc::new(RwLock::new(Config::default()));
        let store = ConfigStore::with_config(config.clone());
        assert!(Arc::ptr_eq(&store.config_ref(), &config));
    }

    #[test]
    fn test_permission_checker_basic() {
        let rules: PermissionRules = vec![];
        let checker = PermissionChecker::new(rules);
        assert!(checker.require("any_tool").is_ok());
    }
}
