//! Integration tests for sisyphus-tools-session `precommit::types` module's
//! pure data type: `PrecommitConfig` (default + custom fields).

use sisyphus_tools_session::precommit::types::PrecommitConfig;

#[test]
fn precommit_config_default_backend_is_rust() {
    let c = PrecommitConfig::default();
    assert_eq!(c.backend, "rust");
}

#[test]
fn precommit_config_default_check_fmt() {
    let c = PrecommitConfig::default();
    assert!(c.check_fmt);
}

#[test]
fn precommit_config_default_check_lint() {
    let c = PrecommitConfig::default();
    assert!(c.check_lint);
}

#[test]
fn precommit_config_default_lint_deny_warnings() {
    let c = PrecommitConfig::default();
    assert!(c.lint_deny_warnings);
}

#[test]
fn precommit_config_field_access() {
    let c = PrecommitConfig {
        backend: "python".into(),
        check_fmt: false,
        check_lint: true,
        lint_deny_warnings: false,
    };
    assert_eq!(c.backend, "python");
    assert!(!c.check_fmt);
    assert!(c.check_lint);
    assert!(!c.lint_deny_warnings);
}

#[test]
fn precommit_config_clone() {
    let a = PrecommitConfig::default();
    let b = a.clone();
    assert_eq!(a.backend, b.backend);
    assert_eq!(a.check_fmt, b.check_fmt);
}

#[test]
fn precommit_config_debug() {
    let c = PrecommitConfig::default();
    let s = format!("{c:?}");
    assert!(s.contains("backend"));
    assert!(s.contains("rust"));
}

#[test]
fn precommit_config_all_backends() {
    let backends = vec!["rust", "node", "python", "go", "java", "gradle", "fe"];
    for b in backends {
        let c = PrecommitConfig {
            backend: b.into(),
            check_fmt: true,
            check_lint: true,
            lint_deny_warnings: true,
        };
        assert_eq!(c.backend, b);
    }
}
