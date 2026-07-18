//! Hook system: registry, matching, execution, and skill integration.
//!
//! ## Sub-modules
//! - [`types`]: Shared types (HookPhase, HookDef, HookEffect, Before/AfterHookResult).

pub mod types;

pub use types::{AfterHookResult, BeforeHookResult, HookDef, HookEffect, HookPhase};

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
    triggers: &[sisyphus_types::TriggerDef],
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
#[path = "hook_tests.rs"]
mod tests;
