//! Integration tests for sisyphus-tools-system PermissionChecker.
//!
//! Exercises the public permission resolution rules used by every tool
//! invocation. The `check_permission` function takes a list of
//! `(pattern, Permission)` rules and returns the first match.

use sisyphus_tools_core::{Permission, PermissionRules};
use sisyphus_tools_system::permissions::{check_permission, PermissionChecker};

fn allow<S: Into<String>>(pattern: S) -> (String, Permission) {
    (pattern.into(), Permission::Allow)
}

fn deny<S: Into<String>>(pattern: S) -> (String, Permission) {
    (pattern.into(), Permission::Deny)
}

fn ask<S: Into<String>>(pattern: S) -> (String, Permission) {
    (pattern.into(), Permission::Ask)
}

#[test]
fn empty_rules_allows_everything() {
    let rules: PermissionRules = vec![];
    assert_eq!(check_permission(&rules, "anything"), Permission::Allow);
    assert_eq!(check_permission(&rules, "bash"), Permission::Allow);
}

#[test]
fn first_matching_pattern_wins() {
    // 'web' pattern is before 'fetch'; 'web_fetch' should match 'web'
    let rules: PermissionRules = vec![allow("web"), deny("fetch")];
    assert_eq!(check_permission(&rules, "web_fetch"), Permission::Allow);
    // 'myfetch' matches 'fetch' but not 'web' → Deny
    assert_eq!(check_permission(&rules, "myfetch"), Permission::Deny);
}

#[test]
fn substring_match_works() {
    // The matching is `contains`, not a glob. Confirm a pattern embedded
    // in the middle of a tool name still matches.
    let rules: PermissionRules = vec![deny("secret")];
    assert_eq!(check_permission(&rules, "read_secret_file"), Permission::Deny);
    assert_eq!(check_permission(&rules, "secret"), Permission::Deny);
    assert_eq!(check_permission(&rules, "public"), Permission::Allow);
}

#[test]
fn unmatched_tool_defaults_to_allow() {
    let rules: PermissionRules = vec![deny("dangerous")];
    assert_eq!(check_permission(&rules, "dangerous_tool"), Permission::Deny);
    assert_eq!(check_permission(&rules, "safe_tool"), Permission::Allow);
}

#[test]
fn permission_ask_preserved_through_chain() {
    let rules: PermissionRules = vec![ask("risky")];
    assert_eq!(check_permission(&rules, "risky_op"), Permission::Ask);
}

#[test]
fn checker_check_delegates_to_check_permission() {
    let rules: PermissionRules = vec![deny("forbidden")];
    let checker = PermissionChecker::new(rules);
    assert_eq!(checker.check("forbidden_op"), Permission::Deny);
    assert_eq!(checker.check("allowed_op"), Permission::Allow);
}

#[test]
fn checker_require_returns_ok_for_allowed() {
    let checker = PermissionChecker::new(vec![]);
    assert!(checker.require("anything").is_ok());
}

#[test]
fn checker_require_returns_err_for_deny() {
    let rules: PermissionRules = vec![deny("blocked")];
    let checker = PermissionChecker::new(rules);
    assert!(checker.require("blocked_tool").is_err());
}

#[test]
fn checker_with_external_directories_builder_pattern() {
    let checker = PermissionChecker::new(vec![]).with_external_directories(vec![]);
    // Empty external_directories → no path restriction → all paths OK.
    let result = checker.check_path_allowed(std::path::Path::new("/anywhere"));
    assert!(result.is_ok());
}

#[test]
fn checker_check_path_allows_paths_under_allowed_dir() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sub = dir.path().join("sub");
    std::fs::create_dir_all(&sub).expect("mkdir");
    let target = sub.join("file.txt");
    std::fs::write(&target, "hi").expect("write");

    let checker = PermissionChecker::new(vec![]).with_external_directories(vec![dir.path().to_path_buf()]);

    let result = checker.check_path_allowed(&target);
    assert!(result.is_ok(), "path under allowed dir should be ok: {result:?}");
}

#[test]
fn checker_check_path_denies_paths_outside_allowed_dir() {
    let allowed = tempfile::tempdir().expect("tempdir");
    let outside = tempfile::tempdir().expect("tempdir");
    let target = outside.path().join("file.txt");
    std::fs::write(&target, "hi").expect("write");

    let checker = PermissionChecker::new(vec![])
        .with_external_directories(vec![allowed.path().to_path_buf()]);

    let result = checker.check_path_allowed(&target);
    assert!(result.is_err(), "path outside allowed dir should be denied");
}
