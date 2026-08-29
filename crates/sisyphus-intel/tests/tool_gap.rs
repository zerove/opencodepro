//! Integration tests for sisyphus-intel `gap_analyzer` module's
//! pure data type: `ToolGap`.

use sisyphus_intel::gap_analyzer::ToolGap;
use sisyphus_types::tools::ToolSpec;

fn sample_tool_spec() -> ToolSpec {
    ToolSpec {
        tool_id: "image_resize".into(),
        required_capability: "image_processing".into(),
        expected_input: serde_json::json!({"path": "string", "size": "u32"}),
        expected_output: serde_json::json!({"resized_path": "string"}),
        success_criteria: "output file exists and is valid".into(),
        test_command: None,
        workdir: None,
    }
}

#[test]
fn tool_gap_field_access() {
    let g = ToolGap {
        gap_id: "gap-001".into(),
        required_capability: "ocr".into(),
        failing_node: "extract_text".into(),
        suggested_tool_spec: sample_tool_spec(),
    };
    assert_eq!(g.gap_id, "gap-001");
    assert_eq!(g.required_capability, "ocr");
    assert_eq!(g.failing_node, "extract_text");
    assert_eq!(g.suggested_tool_spec.tool_id, "image_resize");
}

#[test]
fn tool_gap_clone() {
    let g = ToolGap {
        gap_id: "g".into(),
        required_capability: "c".into(),
        failing_node: "n".into(),
        suggested_tool_spec: sample_tool_spec(),
    };
    let g2 = g.clone();
    assert_eq!(g.gap_id, g2.gap_id);
    assert_eq!(g.suggested_tool_spec.tool_id, g2.suggested_tool_spec.tool_id);
}

#[test]
fn tool_gap_debug() {
    let g = ToolGap {
        gap_id: "g".into(),
        required_capability: "c".into(),
        failing_node: "n".into(),
        suggested_tool_spec: sample_tool_spec(),
    };
    let s = format!("{g:?}");
    assert!(s.contains("g"));
    assert!(s.contains("image_resize"));
}

#[test]
fn tool_gap_different_required_capabilities() {
    let g1 = ToolGap {
        gap_id: "g1".into(),
        required_capability: "ocr".into(),
        failing_node: "n".into(),
        suggested_tool_spec: sample_tool_spec(),
    };
    let g2 = ToolGap {
        gap_id: "g2".into(),
        required_capability: "image_processing".into(),
        failing_node: "n".into(),
        suggested_tool_spec: sample_tool_spec(),
    };
    assert_ne!(g1.required_capability, g2.required_capability);
}

#[test]
fn tool_gap_serde() {
    let g = ToolGap {
        gap_id: "g1".into(),
        required_capability: "ocr".into(),
        failing_node: "n1".into(),
        suggested_tool_spec: sample_tool_spec(),
    };
    let j = serde_json::to_string(&g).unwrap();
    let back: ToolGap = serde_json::from_str(&j).unwrap();
    assert_eq!(back.gap_id, "g1");
    assert_eq!(back.suggested_tool_spec.tool_id, "image_resize");
}
