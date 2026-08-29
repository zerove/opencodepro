//! Integration tests for sisyphus-tools-core `hook` module's
//! pure data types: `HookPhase`, `HookEffect`, `HookDef`,
//! `BeforeHookResult`, `AfterHookResult`.

use sisyphus_tools_core::hook::{AfterHookResult, BeforeHookResult, HookDef, HookEffect, HookPhase};

// ---------------------------------------------------------------------------
// HookPhase
// ---------------------------------------------------------------------------

#[test]
fn hook_phase_as_str() {
    assert_eq!(HookPhase::Before.as_str(), "before");
    assert_eq!(HookPhase::After.as_str(), "after");
}

#[test]
fn hook_phase_serde_round_trip() {
    for phase in [HookPhase::Before, HookPhase::After] {
        let j = serde_json::to_string(&phase).unwrap();
        let back: HookPhase = serde_json::from_str(&j).unwrap();
        assert_eq!(back, phase);
    }
}

#[test]
fn hook_phase_serde_uses_lowercase() {
    assert_eq!(serde_json::to_string(&HookPhase::Before).unwrap(), "\"before\"");
    assert_eq!(serde_json::to_string(&HookPhase::After).unwrap(), "\"after\"");
}

#[test]
fn hook_phase_equality() {
    assert_eq!(HookPhase::Before, HookPhase::Before);
    assert_ne!(HookPhase::Before, HookPhase::After);
}

// ---------------------------------------------------------------------------
// HookEffect
// ---------------------------------------------------------------------------

#[test]
fn hook_effect_variants_constructible() {
    let _ = HookEffect::Allow;
    let _ = HookEffect::Deny("nope".into());
    let _ = HookEffect::InjectInstructions("do X first".into());
    let _ = HookEffect::Log("trace".into());
}

#[test]
fn hook_effect_debug_distinct() {
    let variants = vec![
        HookEffect::Allow,
        HookEffect::Deny("a".into()),
        HookEffect::InjectInstructions("b".into()),
        HookEffect::Log("c".into()),
    ];
    let mut seen = std::collections::HashSet::new();
    for v in &variants {
        let s = format!("{v:?}");
        assert!(seen.insert(s));
    }
    assert_eq!(seen.len(), 4);
}

// ---------------------------------------------------------------------------
// HookDef
// ---------------------------------------------------------------------------

#[test]
fn hook_def_field_access() {
    let h = HookDef {
        name: "audit".into(),
        phase: HookPhase::Before,
        tool_filter: Some("file_edit".into()),
        effect_type: "log".into(),
        effect_payload: "user is editing".into(),
        description: "Audits file edits".into(),
        source_skill: Some("coding-style".into()),
    };
    assert_eq!(h.name, "audit");
    assert_eq!(h.phase, HookPhase::Before);
    assert_eq!(h.tool_filter.as_deref(), Some("file_edit"));
    assert_eq!(h.effect_type, "log");
    assert_eq!(h.effect_payload, "user is editing");
    assert_eq!(h.description, "Audits file edits");
    assert_eq!(h.source_skill.as_deref(), Some("coding-style"));
}

#[test]
fn hook_def_default_effect_type() {
    // When deserializing without effect_type, default is "allow"
    let j = r#"{"name":"x","phase":"before"}"#;
    let h: HookDef = serde_json::from_str(j).unwrap();
    assert_eq!(h.effect_type, "allow");
    assert_eq!(h.effect_payload, "");
    assert_eq!(h.description, "");
    assert!(h.tool_filter.is_none());
}

#[test]
fn hook_def_serde_full() {
    let h = HookDef {
        name: "block-bash".into(),
        phase: HookPhase::Before,
        tool_filter: Some("bash".into()),
        effect_type: "deny".into(),
        effect_payload: "no rm -rf".into(),
        description: "blocks destructive bash".into(),
        source_skill: None,
    };
    let j = serde_json::to_string(&h).unwrap();
    let back: HookDef = serde_json::from_str(&j).unwrap();
    assert_eq!(back.name, "block-bash");
    assert_eq!(back.phase, HookPhase::Before);
    assert_eq!(back.effect_type, "deny");
    assert_eq!(back.effect_payload, "no rm -rf");
    // source_skill is skipped
    assert!(back.source_skill.is_none());
}

#[test]
fn hook_def_source_skill_not_serialized() {
    let h = HookDef {
        name: "x".into(),
        phase: HookPhase::After,
        tool_filter: None,
        effect_type: "allow".into(),
        effect_payload: String::new(),
        description: String::new(),
        source_skill: Some("my-skill".into()),
    };
    let j = serde_json::to_string(&h).unwrap();
    assert!(!j.contains("source_skill"));
    assert!(!j.contains("my-skill"));
}

#[test]
fn hook_def_with_glob_filter() {
    let h = HookDef {
        name: "all".into(),
        phase: HookPhase::Before,
        tool_filter: Some("*".into()),
        effect_type: "log".into(),
        effect_payload: "trace".into(),
        description: "log everything".into(),
        source_skill: None,
    };
    assert_eq!(h.tool_filter.as_deref(), Some("*"));
}

// ---------------------------------------------------------------------------
// BeforeHookResult
// ---------------------------------------------------------------------------

#[test]
fn before_hook_result_defaults_via_fields() {
    let r = BeforeHookResult {
        allowed: true,
        deny_reason: None,
        injected_instructions: vec![],
        logs: vec![],
        matched_hooks: vec![],
    };
    assert!(r.allowed);
    assert!(r.deny_reason.is_none());
    assert!(r.injected_instructions.is_empty());
    assert!(r.logs.is_empty());
    assert!(r.matched_hooks.is_empty());
}

#[test]
fn before_hook_result_denied_state() {
    let r = BeforeHookResult {
        allowed: false,
        deny_reason: Some("policy violation".into()),
        injected_instructions: vec![],
        logs: vec!["warning".into()],
        matched_hooks: vec!["policy-hook".into()],
    };
    assert!(!r.allowed);
    assert_eq!(r.deny_reason.as_deref(), Some("policy violation"));
    assert_eq!(r.matched_hooks, vec!["policy-hook"]);
}

#[test]
fn before_hook_result_instructions() {
    let r = BeforeHookResult {
        allowed: true,
        deny_reason: None,
        injected_instructions: vec!["add docstring".into(), "use type hints".into()],
        logs: vec![],
        matched_hooks: vec!["style-1".into(), "style-2".into()],
    };
    assert_eq!(r.injected_instructions.len(), 2);
    assert_eq!(r.matched_hooks.len(), 2);
}

// ---------------------------------------------------------------------------
// AfterHookResult
// ---------------------------------------------------------------------------

#[test]
fn after_hook_result_empty() {
    let r = AfterHookResult {
        injected_instructions: vec![],
        logs: vec![],
        matched_hooks: vec![],
    };
    assert!(r.injected_instructions.is_empty());
    assert!(r.logs.is_empty());
    assert!(r.matched_hooks.is_empty());
}

#[test]
fn after_hook_result_with_logs() {
    let r = AfterHookResult {
        injected_instructions: vec!["wrap up".into()],
        logs: vec!["completed".into(), "success".into()],
        matched_hooks: vec!["hook-1".into()],
    };
    assert_eq!(r.injected_instructions, vec!["wrap up"]);
    assert_eq!(r.logs.len(), 2);
    assert_eq!(r.matched_hooks, vec!["hook-1"]);
}

#[test]
fn after_hook_result_distinct_type_from_before() {
    let a = AfterHookResult {
        injected_instructions: vec![],
        logs: vec![],
        matched_hooks: vec![],
    };
    let _b: BeforeHookResult = BeforeHookResult {
        allowed: true,
        deny_reason: None,
        injected_instructions: vec![],
        logs: vec![],
        matched_hooks: vec![],
    };
    // AfterHookResult has no `allowed` field — just confirm both debug-format.
    let _ = format!("{a:?}");
}
