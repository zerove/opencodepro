//! 集成测试：tools-system scaffold 模板类型与列表
//!
//! 覆盖 `sisyphus_tools_system::scaffold` 中的纯同步模块：
//! - `TemplateType::from_str_opt` 多种字符串/大小写
//! - `TemplateType::description` 三种类型
//! - `list_templates` 返回 3 项

use sisyphus_tools_system::scaffold::{list_templates, TemplateType};

// ---------------------------------------------------------------------------
// TemplateType::from_str_opt
// ---------------------------------------------------------------------------

#[test]
fn from_str_rust_lib_long() {
    assert_eq!(
        TemplateType::from_str_opt("rust-lib"),
        Some(TemplateType::RustLib)
    );
}

#[test]
fn from_str_rust_lib_short() {
    assert_eq!(
        TemplateType::from_str_opt("lib"),
        Some(TemplateType::RustLib)
    );
}

#[test]
fn from_str_rust_bin_long() {
    assert_eq!(
        TemplateType::from_str_opt("rust-bin"),
        Some(TemplateType::RustBin)
    );
}

#[test]
fn from_str_rust_bin_short() {
    assert_eq!(
        TemplateType::from_str_opt("bin"),
        Some(TemplateType::RustBin)
    );
}

#[test]
fn from_str_rust_workspace_long() {
    assert_eq!(
        TemplateType::from_str_opt("rust-workspace"),
        Some(TemplateType::RustWorkspace)
    );
}

#[test]
fn from_str_rust_workspace_short() {
    assert_eq!(
        TemplateType::from_str_opt("workspace"),
        Some(TemplateType::RustWorkspace)
    );
}

#[test]
fn from_str_case_insensitive() {
    assert_eq!(
        TemplateType::from_str_opt("RUST-LIB"),
        Some(TemplateType::RustLib)
    );
    assert_eq!(
        TemplateType::from_str_opt("Lib"),
        Some(TemplateType::RustLib)
    );
    assert_eq!(
        TemplateType::from_str_opt("WORKSPACE"),
        Some(TemplateType::RustWorkspace)
    );
}

#[test]
fn from_str_trims_whitespace() {
    assert_eq!(
        TemplateType::from_str_opt("  lib  "),
        Some(TemplateType::RustLib)
    );
    assert_eq!(
        TemplateType::from_str_opt("\tbin\n"),
        Some(TemplateType::RustBin)
    );
}

#[test]
fn from_str_unknown_returns_none() {
    assert_eq!(TemplateType::from_str_opt("python"), None);
    assert_eq!(TemplateType::from_str_opt(""), None);
    assert_eq!(TemplateType::from_str_opt("rust"), None);
    assert_eq!(TemplateType::from_str_opt("go-module"), None);
}

// ---------------------------------------------------------------------------
// TemplateType::description
// ---------------------------------------------------------------------------

#[test]
fn description_rust_lib() {
    let d = TemplateType::RustLib.description();
    assert!(d.contains("library"));
    assert!(d.contains("Cargo.toml"));
}

#[test]
fn description_rust_bin() {
    let d = TemplateType::RustBin.description();
    assert!(d.contains("binary"));
    assert!(d.contains("src/main.rs"));
}

#[test]
fn description_rust_workspace() {
    let d = TemplateType::RustWorkspace.description();
    assert!(d.contains("workspace"));
}

// ---------------------------------------------------------------------------
// TemplateType trait derives
// ---------------------------------------------------------------------------

#[test]
fn template_type_eq_copy() {
    let a = TemplateType::RustLib;
    let b = a; // Copy
    assert_eq!(a, b);
}

#[test]
fn template_type_clone() {
    let a = TemplateType::RustBin;
    // rustc 1.98 clippy::clone_on_copy 修复：直接 Copy 复制
    let b = a;
    assert_eq!(a, b);
}

#[test]
fn template_type_debug() {
    let s = format!("{:?}", TemplateType::RustWorkspace);
    assert!(s.contains("RustWorkspace"));
}

#[test]
fn template_type_distinct_variants() {
    assert_ne!(TemplateType::RustLib, TemplateType::RustBin);
    assert_ne!(TemplateType::RustBin, TemplateType::RustWorkspace);
    assert_ne!(TemplateType::RustLib, TemplateType::RustWorkspace);
}

// ---------------------------------------------------------------------------
// list_templates
// ---------------------------------------------------------------------------

#[test]
fn list_templates_returns_three() {
    let templates = list_templates();
    assert_eq!(templates.len(), 3);
}

#[test]
fn list_templates_first_is_lib() {
    let templates = list_templates();
    assert_eq!(templates[0].name, "rust-lib");
    assert!(!templates[0].description.is_empty());
}

#[test]
fn list_templates_second_is_bin() {
    let templates = list_templates();
    assert_eq!(templates[1].name, "rust-bin");
}

#[test]
fn list_templates_third_is_workspace() {
    let templates = list_templates();
    assert_eq!(templates[2].name, "rust-workspace");
}

#[test]
fn list_templates_all_have_names_and_descriptions() {
    for t in list_templates() {
        assert!(!t.name.is_empty());
        assert!(!t.description.is_empty());
    }
}
