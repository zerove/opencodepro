//! Hook system: registry, matching, execution, and skill integration.
//!
//! ## Sub-modules
//! - [`types`]: Shared types (HookPhase, HookDef, HookEffect, Before/AfterHookResult).

pub mod types;

pub use types::{
    AfterHookResult, BeforeHookResult, HookDef, HookEffect, HookPhase,
};

// ── Hook Registry ───────────────────────────────────────────

/// Thread-safe registry that stores and matches hooks against tool IDs.
///
/// Hooks are stored in registration order. Matching is done by:
/// 1. Exact tool_id match
/// 2. Hook with no tool_filter (matches all)
#[derive(Debug, Default, Clone)]
pub struct HookRegistry {
    hooks: Vec<HookDef>,
}

impl HookRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self { hooks: Vec::new() }
    }

    /// Register a hook definition.
    pub fn register(&mut self, hook: HookDef) {
        self.hooks.push(hook);
    }

    /// Register multiple hooks at once.
    pub fn register_all(&mut self, hooks: Vec<HookDef>) {
        self.hooks.extend(hooks);
    }

    /// Find all hooks matching a given tool_id and phase, in registration order.
    pub fn match_hooks(&self, tool_id: &str, phase: HookPhase) -> Vec<&HookDef> {
        self.hooks
            .iter()
            .filter(|h| h.phase == phase)
            .filter(|h| {
                h.tool_filter.as_deref().is_none_or(|filter| {
                    filter == "*" || filter == tool_id || tool_id.starts_with(filter)
                })
            })
            .collect()
    }

    /// Execute all matching before-hooks for a tool. Returns the first non-Allow effect.
    /// Collects instructions from all matching inject hooks.
    pub fn run_before(&self, tool_id: &str) -> BeforeHookResult {
        let matched = self.match_hooks(tool_id, HookPhase::Before);
        let mut denied = None;
        let mut instructions = Vec::new();
        let mut logs = Vec::new();

        for hook in &matched {
            match hook.effect_type.as_str() {
                "deny" => {
                    denied = Some(hook.effect_payload.clone());
                    break; // Deny is terminal — stop processing
                }
                "inject" => {
                    instructions.push(hook.effect_payload.clone());
                }
                "log" => {
                    logs.push(hook.effect_payload.clone());
                }
                _ => {}
            }
        }

        let hook_names: Vec<String> = matched.iter().map(|h| h.name.clone()).collect();

        BeforeHookResult {
            allowed: denied.is_none(),
            deny_reason: denied,
            injected_instructions: instructions,
            logs,
            matched_hooks: hook_names,
        }
    }

    /// Execute all matching after-hooks for a tool. Collects logs and instructions.
    pub fn run_after(&self, tool_id: &str) -> AfterHookResult {
        let matched = self.match_hooks(tool_id, HookPhase::After);
        let mut instructions = Vec::new();
        let mut logs = Vec::new();

        for hook in &matched {
            match hook.effect_type.as_str() {
                "inject" => {
                    instructions.push(hook.effect_payload.clone());
                }
                "log" => {
                    logs.push(hook.effect_payload.clone());
                }
                _ => {}
            }
        }

        let hook_names: Vec<String> = matched.iter().map(|h| h.name.clone()).collect();

        AfterHookResult {
            injected_instructions: instructions,
            logs,
            matched_hooks: hook_names,
        }
    }

    /// Total number of registered hooks.
    pub fn len(&self) -> usize {
        self.hooks.len()
    }

    /// Returns true if no hooks are registered.
    pub fn is_empty(&self) -> bool {
        self.hooks.is_empty()
    }
}

// ── Skill Integration ───────────────────────────────────────

/// Parse hook definitions from a skill's frontmatter triggers.
/// Each trigger with `hook_phase` becomes a hook.
pub fn hooks_from_skill(
    skill_name: &str,
    triggers: &[crate::skill::TriggerDef],
    instructions: &str,
) -> Vec<HookDef> {
    triggers
        .iter()
        .filter(|t| t.hook_phase.is_some())
        .map(|t| {
            let phase = match t.hook_phase.as_deref() {
                Some("after") => HookPhase::After,
                _ => HookPhase::Before,
            };
            let effect_type = t.hook_effect.as_deref().unwrap_or("inject").to_string();
            let effect_payload = match effect_type.as_str() {
                "deny" => t
                    .hook_message
                    .clone()
                    .unwrap_or_else(|| format!("Blocked by skill '{skill_name}'")),
                "log" => t.hook_message.clone().unwrap_or_else(|| {
                    format!(
                        "Hook '{}' fired",
                        t.description.as_deref().unwrap_or(skill_name)
                    )
                }),
                _ => instructions.to_string(),
            };

            // Tool filter: use the first tool in the trigger's tools list, or "*"
            let tool_filter = t.tools.first().cloned();

            HookDef {
                name: format!(
                    "{}/{}",
                    skill_name,
                    t.description.as_deref().unwrap_or("hook")
                ),
                phase,
                tool_filter,
                effect_type,
                effect_payload,
                description: t
                    .description
                    .clone()
                    .unwrap_or_else(|| format!("Hook from skill '{skill_name}'")),
                source_skill: Some(skill_name.to_string()),
            }
        })
        .collect()
}

// ── Executor Integration ───────────────────────────────────

/// Execute a tool with hook interception.
///
/// 1. Runs before-hooks (may deny execution)
/// 2. Calls the inner executor
/// 3. Runs after-hooks
/// 4. Returns result (modified by after-hooks if applicable)
pub async fn execute_with_hooks<F, Fut>(
    tool_id: &str,
    args: Vec<u8>,
    registry: &HookRegistry,
    executor: F,
) -> Result<Vec<u8>, String>
where
    F: FnOnce(Vec<u8>) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<u8>, String>>,
{
    // 1. Before hooks
    let before = registry.run_before(tool_id);

    if !before.allowed {
        let reason = before
            .deny_reason
            .unwrap_or_else(|| "Blocked by hook".into());
        return Err(format!("Hook denied: {reason}"));
    }

    // If hooks injected instructions, we pass them as a side-channel
    // by enriching the args JSON
    let enriched_args = if before.injected_instructions.is_empty() {
        args
    } else if let Ok(mut v) = serde_json::from_slice::<serde_json::Value>(&args) {
        if let serde_json::Value::Object(ref mut map) = v {
            map.insert(
                "_hook_instructions".to_string(),
                serde_json::Value::Array(
                    before
                        .injected_instructions
                        .iter()
                        .map(|s| serde_json::Value::String(s.clone()))
                        .collect(),
                ),
            );
        }
        serde_json::to_vec(&v).unwrap_or(args)
    } else {
        args
    };

    // 2. Execute
    let result = executor(enriched_args).await;

    // 3. After hooks
    let after = registry.run_after(tool_id);

    // If after-hooks injected instructions, pass them as side-channel
    if !after.injected_instructions.is_empty() {
        if let Ok(ref ok_result) = result {
            if let Ok(mut v) = serde_json::from_slice::<serde_json::Value>(ok_result) {
                if let serde_json::Value::Object(ref mut map) = v {
                    map.insert(
                        "_hook_feedback".to_string(),
                        serde_json::Value::Array(
                            after
                                .injected_instructions
                                .iter()
                                .map(|s| serde_json::Value::String(s.clone()))
                                .collect(),
                        ),
                    );
                }
                return serde_json::to_vec(&v).map_err(|e| format!("serialize: {e}"));
            }
        }
    }

    result
}

// ── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skill::TriggerDef;

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

        let result =
            execute_with_hooks(
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

        let result =
            execute_with_hooks(
                "test_tool",
                b"{}".to_vec(),
                &reg,
                |args| async move { Ok(args) },
            )
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Not allowed"));
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
}
