use super::*;
use sisyphus_types::TriggerDef;

#[test]
fn test_empty_registry() {
    let reg = HookRegistry::new();
    assert!(reg.is_empty());
    let result = reg.run_before("file_read");
    assert!(result.allowed);
    assert!(result.matched_hooks.is_empty());
}

#[test]
fn test_deny_hook() {
    let mut reg = HookRegistry::new();
    reg.register(HookDef {
        name: "block-read".into(),
        phase: HookPhase::Before,
        tool_filter: Some("file_read".into()),
        effect_type: "deny".into(),
        effect_payload: "File reads are not allowed".into(),
        description: "Block all file reads".into(),
        source_skill: None,
    });

    let result = reg.run_before("file_read");
    assert!(!result.allowed);
    assert_eq!(result.deny_reason.unwrap(), "File reads are not allowed");
}

#[test]
fn test_allow_hook() {
    let mut reg = HookRegistry::new();
    reg.register(HookDef {
        name: "log-read".into(),
        phase: HookPhase::Before,
        tool_filter: Some("file_read".into()),
        effect_type: "log".into(),
        effect_payload: "file_read called".into(),
        description: "Log file reads".into(),
        source_skill: None,
    });

    let result = reg.run_before("file_read");
    assert!(result.allowed);
    assert_eq!(result.logs.len(), 1);
    assert_eq!(result.matched_hooks.len(), 1);
}

#[test]
fn test_wildcard_filter() {
    let mut reg = HookRegistry::new();
    reg.register(HookDef {
        name: "global-deny".into(),
        phase: HookPhase::Before,
        tool_filter: Some("*".into()),
        effect_type: "deny".into(),
        effect_payload: "Global block".into(),
        description: "Block everything".into(),
        source_skill: None,
    });

    assert!(!reg.run_before("file_read").allowed);
    assert!(!reg.run_before("bash_exec").allowed);
    assert!(!reg.run_before("anything").allowed);
}

#[test]
fn test_prefix_filter() {
    let mut reg = HookRegistry::new();
    reg.register(HookDef {
        name: "block-file-ops".into(),
        phase: HookPhase::Before,
        tool_filter: Some("file_".into()),
        effect_type: "deny".into(),
        effect_payload: "File ops blocked".into(),
        description: "Block all file operations".into(),
        source_skill: None,
    });

    assert!(!reg.run_before("file_read").allowed);
    assert!(!reg.run_before("file_write").allowed);
    assert!(reg.run_before("bash_exec").allowed);
    assert!(reg.run_before("web_search").allowed);
}

#[test]
fn test_no_filter_matches_all() {
    let mut reg = HookRegistry::new();
    reg.register(HookDef {
        name: "log-all".into(),
        phase: HookPhase::Before,
        tool_filter: None,
        effect_type: "log".into(),
        effect_payload: "something executed".into(),
        description: "Log all".into(),
        source_skill: None,
    });

    assert_eq!(reg.run_before("file_read").matched_hooks.len(), 1);
    assert_eq!(reg.run_before("bash_exec").matched_hooks.len(), 1);
    assert!(reg.run_before("file_read").allowed);
}

#[test]
fn test_phase_isolation() {
    let mut reg = HookRegistry::new();
    reg.register(HookDef {
        name: "before-hook".into(),
        phase: HookPhase::Before,
        tool_filter: None,
        effect_type: "log".into(),
        effect_payload: "before".into(),
        description: "".into(),
        source_skill: None,
    });
    reg.register(HookDef {
        name: "after-hook".into(),
        phase: HookPhase::After,
        tool_filter: None,
        effect_type: "log".into(),
        effect_payload: "after".into(),
        description: "".into(),
        source_skill: None,
    });

    assert_eq!(reg.run_before("test").matched_hooks.len(), 1);
    assert_eq!(reg.run_after("test").matched_hooks.len(), 1);
}

#[test]
fn test_inject_instructions() {
    let mut reg = HookRegistry::new();
    reg.register(HookDef {
        name: "inject-read".into(),
        phase: HookPhase::Before,
        tool_filter: Some("file_read".into()),
        effect_type: "inject".into(),
        effect_payload: "Be careful with file reading".into(),
        description: "Inject instructions".into(),
        source_skill: None,
    });

    let result = reg.run_before("file_read");
    assert!(result.allowed);
    assert_eq!(result.injected_instructions.len(), 1);
    assert_eq!(
        result.injected_instructions[0],
        "Be careful with file reading"
    );
}

#[test]
fn test_deny_takes_precedence() {
    let mut reg = HookRegistry::new();
    reg.register(HookDef {
        name: "inject".into(),
        phase: HookPhase::Before,
        tool_filter: Some("file_read".into()),
        effect_type: "inject".into(),
        effect_payload: "instructions".into(),
        description: "".into(),
        source_skill: None,
    });
    reg.register(HookDef {
        name: "deny".into(),
        phase: HookPhase::Before,
        tool_filter: Some("file_read".into()),
        effect_type: "deny".into(),
        effect_payload: "blocked".into(),
        description: "".into(),
        source_skill: None,
    });

    // Deny comes after inject but should still block
    let result = reg.run_before("file_read");
    assert!(!result.allowed);
}

#[test]
fn test_hooks_from_skill_with_hook_phase() {
    let triggers = vec![TriggerDef {
        pattern: None,
        tools: vec!["file_read".into()],
        description: Some("file-read-guard".into()),
        hook_phase: Some("before".into()),
        hook_effect: Some("deny".into()),
        hook_message: Some("No file reads in this project".into()),
    }];

    let hooks = hooks_from_skill("my-skill", &triggers, "instructions body");
    assert_eq!(hooks.len(), 1);
    assert_eq!(hooks[0].phase, HookPhase::Before);
    assert_eq!(hooks[0].effect_type, "deny");
    assert_eq!(hooks[0].effect_payload, "No file reads in this project");
    assert_eq!(hooks[0].tool_filter.as_deref(), Some("file_read"));
    assert_eq!(hooks[0].source_skill.as_deref(), Some("my-skill"));
}

#[test]
fn test_hooks_from_skill_default_inject() {
    let triggers = vec![TriggerDef {
        pattern: None,
        tools: vec!["bash_exec".into()],
        description: Some("bash-guard".into()),
        hook_phase: Some("before".into()),
        hook_effect: None, // defaults to "inject"
        hook_message: None,
    }];

    let hooks = hooks_from_skill("safe-bash", &triggers, "Only use trusted commands");
    assert_eq!(hooks.len(), 1);
    assert_eq!(hooks[0].effect_type, "inject");
    assert_eq!(hooks[0].effect_payload, "Only use trusted commands");
}

#[test]
fn test_hooks_from_skill_after_phase() {
    let triggers = vec![TriggerDef {
        pattern: None,
        tools: vec!["web_search".into()],
        description: Some("log-search".into()),
        hook_phase: Some("after".into()),
        hook_effect: Some("log".into()),
        hook_message: Some("Web search completed".into()),
    }];

    let hooks = hooks_from_skill("logger", &triggers, "");
    assert_eq!(hooks.len(), 1);
    assert_eq!(hooks[0].phase, HookPhase::After);
    assert_eq!(hooks[0].effect_type, "log");
    assert_eq!(hooks[0].effect_payload, "Web search completed");
}

#[tokio::test]
async fn test_execute_with_hooks_allowed() {
    let mut reg = HookRegistry::new();
    reg.register(HookDef {
        name: "log".into(),
        phase: HookPhase::Before,
        tool_filter: None,
        effect_type: "log".into(),
        effect_payload: "executing".into(),
        description: "".into(),
        source_skill: None,
    });

    let result = execute_with_hooks(
        "test_tool",
        b"{}".to_vec(),
        &reg,
        |args| async move { Ok(args) },
    )
    .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_execute_with_hooks_denied() {
    let mut reg = HookRegistry::new();
    reg.register(HookDef {
        name: "block".into(),
        phase: HookPhase::Before,
        tool_filter: None,
        effect_type: "deny".into(),
        effect_payload: "Not allowed".into(),
        description: "".into(),
        source_skill: None,
    });

    let result = execute_with_hooks(
        "test_tool",
        b"{}".to_vec(),
        &reg,
        |args| async move { Ok(args) },
    )
    .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Not allowed"));
}

#[tokio::test]
async fn test_execute_with_hooks_inject_instructions() {
    let mut reg = HookRegistry::new();
    reg.register(HookDef {
        name: "inject".into(),
        phase: HookPhase::Before,
        tool_filter: None,
        effect_type: "inject".into(),
        effect_payload: "Be careful".into(),
        description: "".into(),
        source_skill: None,
    });

    let result = execute_with_hooks("test_tool", b"{}".to_vec(), &reg, |args| async move {
        // Verify instructions were injected into args
        let v: serde_json::Value = serde_json::from_slice(&args).unwrap();
        let instr = v["_hook_instructions"].as_array();
        assert!(instr.is_some());
        assert_eq!(instr.unwrap()[0], "Be careful");
        Ok(args)
    })
    .await;

    assert!(result.is_ok());
}

#[test]
fn test_register_all() {
    let mut reg = HookRegistry::new();
    reg.register_all(vec![
        HookDef {
            name: "h1".into(),
            phase: HookPhase::Before,
            tool_filter: None,
            effect_type: "log".into(),
            effect_payload: "".into(),
            description: "".into(),
            source_skill: None,
        },
        HookDef {
            name: "h2".into(),
            phase: HookPhase::After,
            tool_filter: None,
            effect_type: "log".into(),
            effect_payload: "".into(),
            description: "".into(),
            source_skill: None,
        },
    ]);
    assert_eq!(reg.len(), 2);
}
