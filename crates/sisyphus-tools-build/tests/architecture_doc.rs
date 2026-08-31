//! 集成测试：tools-build architecture（模块分析 + ADR 生成）
//!
//! 覆盖 `sisyphus_tools_build::architecture` 中的纯同步模块：
//! - `ModuleReport`/`RefactoringSuggestion`/`ArchitectureDoc` 字段 + serde
//! - `ArchitectureAnalyzer::generate_adr` Markdown 输出

use sisyphus_tools_build::architecture::{
    ArchitectureAnalyzer, ArchitectureDoc, ModuleReport, RefactoringSuggestion,
};

fn make_report(name: &str, coupling: f64) -> ModuleReport {
    ModuleReport {
        name: name.to_string(),
        file_path: format!("src/{name}.rs"),
        public_fns: vec!["foo".to_string(), "bar".to_string()],
        public_types: vec!["Baz".to_string()],
        imports: vec!["std::collections".to_string()],
        coupling_score: coupling,
    }
}

fn make_suggestion(module: &str, severity: &str) -> RefactoringSuggestion {
    RefactoringSuggestion {
        module: module.to_string(),
        issue: "high coupling".to_string(),
        severity: severity.to_string(),
        suggestion: "split module".to_string(),
        estimated_effort: "medium".to_string(),
    }
}

fn make_doc(name: &str) -> ArchitectureDoc {
    ArchitectureDoc {
        module_name: name.to_string(),
        reports: vec![make_report("a", 0.3), make_report("b", 0.7)],
        coupling_analysis: "Average coupling is 0.5".to_string(),
        suggestions: vec![make_suggestion("a", "high"), make_suggestion("b", "low")],
        generated_at: 1_700_000_000,
    }
}

// ---------------------------------------------------------------------------
// ModuleReport
// ---------------------------------------------------------------------------

#[test]
fn report_field_access() {
    let r = make_report("foo", 0.5);
    assert_eq!(r.name, "foo");
    assert_eq!(r.file_path, "src/foo.rs");
    assert_eq!(r.public_fns.len(), 2);
    assert_eq!(r.public_types, vec!["Baz".to_string()]);
    assert_eq!(r.imports.len(), 1);
    assert!((r.coupling_score - 0.5).abs() < 1e-9);
}

#[test]
fn report_clone() {
    let r = make_report("foo", 0.5);
    let r2 = r.clone();
    assert_eq!(r.name, r2.name);
    assert_eq!(r.coupling_score, r2.coupling_score);
}

#[test]
fn report_debug() {
    let r = make_report("foo", 0.5);
    let s = format!("{r:?}");
    assert!(s.contains("ModuleReport"));
    assert!(s.contains("foo"));
}

// ---------------------------------------------------------------------------
// RefactoringSuggestion
// ---------------------------------------------------------------------------

#[test]
fn suggestion_field_access() {
    let s = make_suggestion("m", "high");
    assert_eq!(s.module, "m");
    assert_eq!(s.issue, "high coupling");
    assert_eq!(s.severity, "high");
    assert_eq!(s.suggestion, "split module");
    assert_eq!(s.estimated_effort, "medium");
}

#[test]
fn suggestion_clone() {
    let s = make_suggestion("m", "high");
    let s2 = s.clone();
    assert_eq!(s.module, s2.module);
    assert_eq!(s.severity, s2.severity);
}

// ---------------------------------------------------------------------------
// ArchitectureDoc
// ---------------------------------------------------------------------------

#[test]
fn doc_field_access() {
    let d = make_doc("test");
    assert_eq!(d.module_name, "test");
    assert_eq!(d.reports.len(), 2);
    assert_eq!(d.suggestions.len(), 2);
    assert_eq!(d.coupling_analysis, "Average coupling is 0.5");
    assert_eq!(d.generated_at, 1_700_000_000);
}

#[test]
fn doc_clone() {
    let d = make_doc("test");
    let d2 = d.clone();
    assert_eq!(d.module_name, d2.module_name);
    assert_eq!(d.reports.len(), d2.reports.len());
}

#[test]
fn doc_empty() {
    let d = ArchitectureDoc {
        module_name: "empty".to_string(),
        reports: vec![],
        coupling_analysis: String::new(),
        suggestions: vec![],
        generated_at: 0,
    };
    assert!(d.reports.is_empty());
    assert!(d.suggestions.is_empty());
    assert_eq!(d.generated_at, 0);
}

// ---------------------------------------------------------------------------
// ArchitectureAnalyzer::generate_adr
// ---------------------------------------------------------------------------

#[test]
fn generate_adr_includes_module_name() {
    let d = make_doc("my_module");
    let md = ArchitectureAnalyzer::generate_adr(&d);
    assert!(md.contains("my_module"));
    assert!(md.contains("# Architecture Analysis"));
}

#[test]
fn generate_adr_includes_coupling_analysis() {
    let d = make_doc("test");
    let md = ArchitectureAnalyzer::generate_adr(&d);
    assert!(md.contains("Average coupling is 0.5"));
    assert!(md.contains("Summary"));
}

#[test]
fn generate_adr_includes_suggestions() {
    let d = make_doc("test");
    let md = ArchitectureAnalyzer::generate_adr(&d);
    assert!(md.contains("Refactoring Suggestions"));
    assert!(md.contains("high coupling"));
    assert!(md.contains("a")); // suggestion module
    assert!(md.contains("b"));
}

#[test]
fn generate_adr_no_suggestions_section_when_empty() {
    let d = ArchitectureDoc {
        module_name: "clean".to_string(),
        reports: vec![],
        coupling_analysis: "ok".to_string(),
        suggestions: vec![],
        generated_at: 0,
    };
    let md = ArchitectureAnalyzer::generate_adr(&d);
    // When no suggestions, the section is omitted
    assert!(!md.contains("Refactoring Suggestions"));
}

#[test]
fn generate_adr_includes_timestamp() {
    let d = ArchitectureDoc {
        module_name: "x".to_string(),
        reports: vec![],
        coupling_analysis: "y".to_string(),
        suggestions: vec![],
        generated_at: 1_234_567_890,
    };
    let md = ArchitectureAnalyzer::generate_adr(&d);
    assert!(md.contains("1234567890"));
}

#[test]
fn generate_adr_single_suggestion() {
    let d = ArchitectureDoc {
        module_name: "single".to_string(),
        reports: vec![],
        coupling_analysis: "low".to_string(),
        suggestions: vec![make_suggestion("only", "medium")],
        generated_at: 0,
    };
    let md = ArchitectureAnalyzer::generate_adr(&d);
    assert!(md.contains("Refactoring Suggestions"));
    assert!(md.contains("only"));
    assert!(md.contains("medium"));
}

#[test]
fn generate_adr_severity_and_effort_in_output() {
    let d = ArchitectureDoc {
        module_name: "x".to_string(),
        reports: vec![],
        coupling_analysis: "y".to_string(),
        suggestions: vec![make_suggestion("m", "high")],
        generated_at: 0,
    };
    let md = ArchitectureAnalyzer::generate_adr(&d);
    assert!(md.contains("[high]")); // severity
    assert!(md.contains("[medium]")); // estimated effort
}
