//! Integration tests for sisyphus-tools-scm `github::types` module's
//! pure data types: `WorkflowKind`, `GenerateOptions`, `CiConfig`.

use sisyphus_tools_scm::github::types::{CiConfig, GenerateOptions, WorkflowKind};

// ---------------------------------------------------------------------------
// WorkflowKind
// ---------------------------------------------------------------------------

#[test]
fn workflow_kind_variants() {
    let _ = WorkflowKind::Ci;
    let _ = WorkflowKind::PrReview;
}

#[test]
fn workflow_kind_debug() {
    let s = format!("{:?}", WorkflowKind::Ci);
    assert!(s.contains("Ci"));
    let s = format!("{:?}", WorkflowKind::PrReview);
    assert!(s.contains("PrReview"));
}

#[test]
fn workflow_kind_clone() {
    let a = WorkflowKind::Ci;
    let b = a.clone();
    assert_eq!(format!("{a:?}"), format!("{b:?}"));
}

#[test]
fn workflow_kind_distinct() {
    let a = WorkflowKind::Ci;
    let b = WorkflowKind::PrReview;
    let mut seen = std::collections::HashSet::new();
    seen.insert(format!("{a:?}"));
    seen.insert(format!("{b:?}"));
    assert_eq!(seen.len(), 2);
}

// ---------------------------------------------------------------------------
// GenerateOptions
// ---------------------------------------------------------------------------

#[test]
fn generate_options_field_access() {
    let opts = GenerateOptions {
        rust_toolchain: "nightly".into(),
        cargo_extra_flags: "--release".into(),
        exclude_packages: "bridge,sandbox".into(),
        include_clippy: true,
        include_deny: false,
    };
    assert_eq!(opts.rust_toolchain, "nightly");
    assert_eq!(opts.cargo_extra_flags, "--release");
    assert_eq!(opts.exclude_packages, "bridge,sandbox");
}

#[test]
fn generate_options_default_strings() {
    let opts = GenerateOptions {
        rust_toolchain: "stable".into(),
        cargo_extra_flags: String::new(),
        exclude_packages: String::new(),
        include_clippy: true,
        include_deny: false,
    };
    assert_eq!(opts.rust_toolchain, "stable");
    assert!(opts.cargo_extra_flags.is_empty());
    assert!(opts.exclude_packages.is_empty());
}

#[test]
fn generate_options_clone() {
    let a = GenerateOptions {
        rust_toolchain: "x".into(),
        cargo_extra_flags: "y".into(),
        exclude_packages: "z".into(),
        include_clippy: true,
        include_deny: false,
    };
    let b = a.clone();
    assert_eq!(a.rust_toolchain, b.rust_toolchain);
}

// ---------------------------------------------------------------------------
// CiConfig
// ---------------------------------------------------------------------------

#[test]
fn ci_config_field_access() {
    let c = CiConfig {
        ci_mode: true,
        github_token: Some("ghp_xxx".into()),
        pr_number: Some(42),
        output_format: "plain".into(),
    };
    assert!(c.ci_mode);
    assert_eq!(c.github_token.as_deref(), Some("ghp_xxx"));
    assert_eq!(c.pr_number, Some(42));
}

#[test]
fn ci_config_disabled_no_token() {
    let c = CiConfig {
        ci_mode: false,
        github_token: None,
        pr_number: None,
        output_format: "plain".into(),
    };
    assert!(!c.ci_mode);
    assert!(c.github_token.is_none());
    assert!(c.pr_number.is_none());
}

#[test]
fn ci_config_clone() {
    let a = CiConfig {
        ci_mode: true,
        github_token: Some("t".into()),
        pr_number: Some(1),
        output_format: "plain".into(),
    };
    let b = a.clone();
    assert_eq!(a.ci_mode, b.ci_mode);
    assert_eq!(a.pr_number, b.pr_number);
}

#[test]
fn ci_config_debug() {
    let c = CiConfig {
        ci_mode: true,
        github_token: None,
        pr_number: Some(5),
        output_format: "plain".into(),
    };
    let s = format!("{c:?}");
    assert!(s.contains("ci_mode"));
    assert!(s.contains("5"));
}
