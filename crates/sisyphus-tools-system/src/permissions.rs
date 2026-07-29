use sisyphus_tools_core::{Permission, PermissionRules, ToolError, ToolResult};

pub fn check_permission(rules: &PermissionRules, tool_name: &str) -> Permission {
    for (pattern, permission) in rules {
        if tool_name.contains(pattern) {
            return permission.clone();
        }
    }
    Permission::Allow
}

pub struct PermissionChecker {
    rules: PermissionRules,
    /// Allowed external directories. If non-empty, file tools outside these dirs are denied.
    pub external_directories: Vec<std::path::PathBuf>,
}

impl PermissionChecker {
    pub fn new(rules: PermissionRules) -> Self {
        Self {
            rules,
            external_directories: Vec::new(),
        }
    }

    pub fn with_external_directories(mut self, dirs: Vec<std::path::PathBuf>) -> Self {
        self.external_directories = dirs;
        self
    }

    pub fn check(&self, tool_name: &str) -> Permission {
        check_permission(&self.rules, tool_name)
    }

    /// Check if a file path is within allowed external directories.
    /// Returns Ok(()) if allowed, Err with description if denied.
    pub fn check_path_allowed(&self, path: &std::path::Path) -> Result<(), ToolError> {
        if self.external_directories.is_empty() {
            return Ok(()); // No restrictions configured
        }
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        for allowed in &self.external_directories {
            let allowed_canonical = allowed.canonicalize().unwrap_or_else(|_| allowed.clone());
            if canonical.starts_with(&allowed_canonical) {
                return Ok(());
            }
        }
        Err(ToolError::PermissionDenied(format!(
            "Path '{}' is outside allowed external directories",
            path.display()
        )))
    }

    pub fn require(&self, tool_name: &str) -> ToolResult<()> {
        match self.check(tool_name) {
            Permission::Allow => Ok(()),
            Permission::Ask => Ok(()), // In a real system, this would trigger a prompt
            Permission::Deny => Err(ToolError::PermissionDenied(tool_name.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_permission_allow_by_default() {
        let rules: PermissionRules = vec![];
        assert_eq!(check_permission(&rules, "read"), Permission::Allow);
    }

    #[test]
    fn test_check_permission_deny() {
        let rules = vec![("write".into(), Permission::Deny)];
        assert_eq!(check_permission(&rules, "write"), Permission::Deny);
    }

    #[test]
    fn test_check_permission_allow_explicit() {
        let rules = vec![("read".into(), Permission::Allow)];
        assert_eq!(check_permission(&rules, "read"), Permission::Allow);
    }

    #[test]
    fn test_check_permission_ask() {
        let rules = vec![("exec".into(), Permission::Ask)];
        assert_eq!(check_permission(&rules, "exec"), Permission::Ask);
    }

    #[test]
    fn test_check_permission_pattern_match() {
        let rules = vec![("write".into(), Permission::Deny)];
        assert_eq!(check_permission(&rules, "write_big"), Permission::Deny);
    }

    #[test]
    fn test_permission_checker_require() {
        let rules = vec![("read".into(), Permission::Allow)];
        let checker = PermissionChecker::new(rules);
        assert!(checker.require("read").is_ok());
    }

    #[test]
    fn test_permission_checker_deny() {
        let rules = vec![("write".into(), Permission::Deny)];
        let checker = PermissionChecker::new(rules);
        let result = checker.require("write");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ToolError::PermissionDenied(_)
        ));
    }

    #[test]
    fn test_check_permission_no_match_returns_allow() {
        let rules = vec![("write".into(), Permission::Deny)];
        assert_eq!(check_permission(&rules, "read"), Permission::Allow);
    }

    #[test]
    fn test_check_permission_first_match_wins() {
        let rules: PermissionRules = vec![
            ("read".into(), Permission::Ask),
            ("read".into(), Permission::Allow),
        ];
        assert_eq!(check_permission(&rules, "read"), Permission::Ask);
    }

    #[test]
    fn test_permission_checker_ask_returns_ok() {
        let rules = vec![("exec".into(), Permission::Ask)];
        let checker = PermissionChecker::new(rules);
        assert!(checker.require("exec").is_ok());
    }

    #[test]
    fn test_permission_checker_empty_rules() {
        let checker = PermissionChecker::new(vec![]);
        assert!(checker.require("anything").is_ok());
    }

    #[test]
    fn test_permission_checker_multiple_rules() {
        let rules: PermissionRules = vec![
            ("write".into(), Permission::Deny),
            ("exec".into(), Permission::Ask),
            ("read".into(), Permission::Allow),
        ];
        let checker = PermissionChecker::new(rules);
        assert!(checker.require("write").is_err());
        assert!(checker.require("exec").is_ok());
        assert!(checker.require("read").is_ok());
        assert!(checker.require("delete").is_ok());
    }
}
