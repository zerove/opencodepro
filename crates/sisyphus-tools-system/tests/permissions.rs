//! Integration tests for sisyphus-tools-system `permissions`.
//!
//! Exercises the public [`PermissionChecker`] API: the standalone
//! `check_permission` free function and the methods on
//! `PermissionChecker` (constructor, `with_external_directories`,
//! `check`, `check_path_allowed`, `require`).

use sisyphus_tools_core::{Permission, PermissionRules};
use sisyphus_tools_system::permissions::{check_permission, PermissionChecker};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// check_permission (free function)
// ---------------------------------------------------------------------------

#[test]
fn check_permission_empty_rules_allows_everything() {
    let rules: PermissionRules = vec![];
    assert_eq!(check_permission(&rules, "any_tool"), Permission::Allow);
    assert_eq!(check_permission(&rules, "another"), Permission::Allow);
}

#[test]
fn check_permission_explicit_deny() {
    let rules = vec![("write".into(), Permission::Deny)];
    assert_eq!(check_permission(&rules, "write"), Permission::Deny);
}

#[test]
fn check_permission_explicit_ask() {
    let rules = vec![("exec".into(), Permission::Ask)];
    assert_eq!(check_permission(&rules, "exec"), Permission::Ask);
}

#[test]
fn check_permission_pattern_substring_match() {
    let rules = vec![("write".into(), Permission::Deny)];
    // "write" is a substring of "write_big" → matches.
    assert_eq!(check_permission(&rules, "write_big"), Permission::Deny);
    assert_eq!(check_permission(&rules, "small_write"), Permission::Deny);
}

#[test]
fn check_permission_first_match_wins() {
    let rules = vec![
        ("read".into(), Permission::Allow),
        ("read_secret".into(), Permission::Deny),
    ];
    // "read" appears first → Allow (even though "read_secret" would match "read" too).
    assert_eq!(
        check_permission(&rules, "read_secret_file"),
        Permission::Allow
    );
}

#[test]
fn check_permission_no_substring_match() {
    let rules = vec![("write".into(), Permission::Deny)];
    // "read" doesn't contain "write" → no match → default Allow.
    assert_eq!(check_permission(&rules, "read"), Permission::Allow);
}

// ---------------------------------------------------------------------------
// PermissionChecker
// ---------------------------------------------------------------------------

#[test]
fn checker_new_starts_with_no_external_dirs() {
    let checker = PermissionChecker::new(vec![]);
    // No external dirs → check_path_allowed always returns Ok.
    let result = checker.check_path_allowed(&PathBuf::from("/anywhere"));
    assert!(result.is_ok());
}

#[test]
fn checker_with_external_directories_sets_dirs() {
    let checker =
        PermissionChecker::new(vec![]).with_external_directories(vec![PathBuf::from("/tmp")]);
    // Path under /tmp is allowed.
    let result = checker.check_path_allowed(&PathBuf::from("/tmp/foo"));
    assert!(result.is_ok());
}

#[test]
fn checker_check_delegates_to_check_permission() {
    let rules = vec![("write".into(), Permission::Deny)];
    let checker = PermissionChecker::new(rules);
    assert_eq!(checker.check("write"), Permission::Deny);
    assert_eq!(checker.check("read"), Permission::Allow);
}

#[test]
fn checker_require_allow() {
    let rules: PermissionRules = vec![];
    let checker = PermissionChecker::new(rules);
    assert!(checker.require("anything").is_ok());
}

#[test]
fn checker_require_deny() {
    let rules = vec![("dangerous".into(), Permission::Deny)];
    let checker = PermissionChecker::new(rules);
    let result = checker.require("dangerous_op");
    assert!(result.is_err());
}

#[test]
fn checker_require_ask_succeeds() {
    // In this implementation, Ask → Ok (a real system would prompt).
    let rules = vec![("ask_me".into(), Permission::Ask)];
    let checker = PermissionChecker::new(rules);
    assert!(checker.require("ask_me_op").is_ok());
}

// ---------------------------------------------------------------------------
// check_path_allowed
// ---------------------------------------------------------------------------

#[test]
fn check_path_allowed_with_no_directories_is_open() {
    let checker = PermissionChecker::new(vec![]);
    let result = checker.check_path_allowed(&PathBuf::from("/etc/passwd"));
    assert!(result.is_ok());
}

#[test]
fn check_path_allowed_within_allowed_dir() {
    let tmp = std::env::temp_dir();
    let checker = PermissionChecker::new(vec![]).with_external_directories(vec![tmp.clone()]);
    let result = checker.check_path_allowed(&tmp.join("test.txt"));
    // The tmp dir is allowed; the file is under it.
    assert!(result.is_ok());
}

#[test]
fn check_path_allowed_outside_allowed_dir() {
    let allowed = PathBuf::from("/tmp/very_specific_dir_that_likely_does_not_exist_xyz");
    let checker = PermissionChecker::new(vec![]).with_external_directories(vec![allowed]);
    // Try a different path.
    let result = checker.check_path_allowed(&PathBuf::from("/etc"));
    assert!(result.is_err());
}
