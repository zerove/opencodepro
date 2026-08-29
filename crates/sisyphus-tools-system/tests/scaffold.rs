//! Integration tests for sisyphus-tools-system `scaffold` module.
//!
//! Exercises the pure-data surfaces of [`TemplateType`] and
//! [`TemplateInfo`], plus `list_templates`. File-creating `scaffold`
//! requires a real filesystem and is left out of scope.

use sisyphus_tools_system::scaffold::{list_templates, TemplateInfo, TemplateType};

// ---------------------------------------------------------------------------
// TemplateType
// ---------------------------------------------------------------------------

#[test]
fn template_type_from_str_opt() {
    assert_eq!(TemplateType::from_str_opt("rust-lib"), Some(TemplateType::RustLib));
    assert_eq!(TemplateType::from_str_opt("lib"), Some(TemplateType::RustLib));
    assert_eq!(TemplateType::from_str_opt("rust-bin"), Some(TemplateType::RustBin));
    assert_eq!(TemplateType::from_str_opt("bin"), Some(TemplateType::RustBin));
    assert_eq!(
        TemplateType::from_str_opt("rust-workspace"),
        Some(TemplateType::RustWorkspace)
    );
    assert_eq!(
        TemplateType::from_str_opt("workspace"),
        Some(TemplateType::RustWorkspace)
    );
    assert!(TemplateType::from_str_opt("unknown").is_none());
}

#[test]
fn template_type_from_str_opt_case_insensitive() {
    assert_eq!(
        TemplateType::from_str_opt("RUST-LIB"),
        Some(TemplateType::RustLib)
    );
    assert_eq!(TemplateType::from_str_opt("Lib"), Some(TemplateType::RustLib));
}

#[test]
fn template_type_from_str_opt_trims_whitespace() {
    assert_eq!(
        TemplateType::from_str_opt("  rust-lib  "),
        Some(TemplateType::RustLib)
    );
}

#[test]
fn template_type_description_nonempty() {
    for t in [
        TemplateType::RustLib,
        TemplateType::RustBin,
        TemplateType::RustWorkspace,
    ] {
        assert!(!t.description().is_empty());
    }
}

#[test]
fn template_type_equality() {
    assert_eq!(TemplateType::RustLib, TemplateType::RustLib);
    assert_ne!(TemplateType::RustLib, TemplateType::RustBin);
}

// ---------------------------------------------------------------------------
// TemplateInfo
// ---------------------------------------------------------------------------

#[test]
fn template_info_field_assignment() {
    let info = TemplateInfo {
        name: "rust-lib",
        description: "A Rust library",
    };
    assert_eq!(info.name, "rust-lib");
    assert_eq!(info.description, "A Rust library");
}

// ---------------------------------------------------------------------------
// list_templates
// ---------------------------------------------------------------------------

#[test]
fn list_templates_returns_three() {
    let list = list_templates();
    assert_eq!(list.len(), 3);
    // Verify all three types are listed.
    let names: Vec<&str> = list.iter().map(|t| t.name).collect();
    assert!(names.contains(&"rust-lib"));
    assert!(names.contains(&"rust-bin"));
    assert!(names.contains(&"rust-workspace"));
}

#[test]
fn list_templates_descriptions_nonempty() {
    for t in list_templates() {
        assert!(!t.description.is_empty());
        assert!(!t.name.is_empty());
    }
}
