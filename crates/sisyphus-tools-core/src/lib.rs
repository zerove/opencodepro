#![allow(clippy::cast_precision_loss)]
pub use sisyphus_types::{ToolError, ToolResult};

#[derive(Debug, Clone, PartialEq, Eq)]
/// Permission.
pub enum Permission {
    Allow,
    Deny,
    Ask,
}

pub type PermissionRules = Vec<(String, Permission)>;

pub mod feedback;
pub mod hook;
pub mod process;
pub mod wisdom;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permission_variants() {
        let allow = Permission::Allow;
        let deny = Permission::Deny;
        let ask = Permission::Ask;

        assert_ne!(allow, deny);
        assert_ne!(allow, ask);
        assert_ne!(deny, ask);

        assert_eq!(format!("{:?}", allow), "Allow");
        assert_eq!(format!("{:?}", deny), "Deny");
        assert_eq!(format!("{:?}", ask), "Ask");
    }

    #[test]
    fn test_tool_error_generic_display() {
        let err = ToolError::Generic("test error".to_string());
        assert_eq!(err.to_string(), "Generic error: test error");
    }

    /// ADR-014 phase 1 moved the definition to `sisyphus-types` and left a
    /// re-export here. This pins the two paths to the *same* type: if a later
    /// stage ever re-defines rather than re-exports, callers would silently
    /// end up with two incompatible `ToolError`s.
    #[test]
    fn tool_error_is_reexported_from_types_not_redefined() {
        let via_core: ToolError = ToolError::NotFound("x".into());
        let via_types: sisyphus_types::ToolError = via_core;
        assert_eq!(via_types.to_string(), "Path not found: x");

        fn takes_core_result(_: ToolResult<u8>) {}
        fn takes_types_result(_: sisyphus_types::ToolResult<u8>) {}
        // Both signatures must name the same alias.
        let r: ToolResult<u8> = Ok(1);
        takes_core_result(r);
        takes_types_result(Ok(1));
    }
}
