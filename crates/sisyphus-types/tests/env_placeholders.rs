//! Integration tests for sisyphus-types env-var resolution.
//!
//! Covers the `{env:VAR}` and `{env:VAR|default}` placeholder syntax used
//! by `Config::resolve_env_placeholders` (and its single-string helper
//! `resolve_env_placeholders_str`).

use sisyphus_types::env_guard::EnvVarGuard;
use sisyphus_types::resolve_env_placeholders_str;

/// Use a unique env var name per test to avoid racing with other tests
/// that mutate the process environment. Names are process-PID-scoped.
fn scoped_var(label: &str) -> String {
    format!("SISYPHUS_TEST_{}_{}", label, std::process::id())
}

#[test]
fn unknown_var_substitutes_empty_string() {
    let var = scoped_var("MISSING");
    // Make sure it really is missing
    // SAFETY: PID-scoped name; unique to this test.
    let _guard = unsafe { EnvVarGuard::unset_owned(var.clone()) };
    let template = format!("prefix-{{env:{var}}}-suffix");
    let out = resolve_env_placeholders_str(&template);
    assert_eq!(out, "prefix--suffix");
}

#[test]
fn var_with_default_falls_back_when_unset() {
    let var = scoped_var("DEFAULT_FALLBACK");
    // SAFETY: PID-scoped name; unique to this test.
    let _guard = unsafe { EnvVarGuard::unset_owned(var.clone()) };
    let out = resolve_env_placeholders_str(&format!("{{env:{var}|fallback_value}}"));
    assert_eq!(out, "fallback_value");
}

#[test]
fn var_with_default_uses_env_when_set() {
    let var = scoped_var("DEFAULT_PRESENT");
    // SAFETY: PID-scoped name; unique to this test.
    let _guard = unsafe { EnvVarGuard::set_owned(var.clone(), "from_env") };
    let out = resolve_env_placeholders_str(&format!("{{env:{var}|fallback}}"));
    assert_eq!(out, "from_env");
}

#[test]
fn plain_text_passes_through_unchanged() {
    let out = resolve_env_placeholders_str("hello world, no placeholders here");
    assert_eq!(out, "hello world, no placeholders here");
}

#[test]
fn malformed_placeholder_is_left_intact() {
    // `{env:VAR` without closing brace must not panic and must not be replaced.
    let out = resolve_env_placeholders_str("unterminated {env:NO_CLOSE");
    assert_eq!(out, "unterminated {env:NO_CLOSE");
}
