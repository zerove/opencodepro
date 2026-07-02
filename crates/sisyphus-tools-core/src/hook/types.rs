//! Shared types for the Hook system.
//!
//! Defines phases, effects, definitions, and results used by the hook
//! registry and executor integration.

use serde::{Deserialize, Serialize};

// ── Hook Phase ──────────────────────────────────────────────

/// When a hook fires relative to tool execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HookPhase {
    /// Fires before tool execution — can block or modify arguments.
    #[serde(rename = "before")]
    Before,
    /// Fires after tool execution — can inspect or modify results.
    #[serde(rename = "after")]
    After,
}

impl HookPhase {
    pub fn as_str(&self) -> &'static str {
        match self {
            HookPhase::Before => "before",
            HookPhase::After => "after",
        }
    }
}

// ── Hook Effect ─────────────────────────────────────────────

/// What a hook instructs the executor to do.
#[derive(Debug, Clone)]
pub enum HookEffect {
    /// Allow execution to proceed (default).
    Allow,
    /// Block execution with a reason string.
    Deny(String),
    /// Inject extra instructions into the execution context.
    InjectInstructions(String),
    /// Log a message (observability-only, does not affect execution).
    Log(String),
}

// ── Hook Definition ─────────────────────────────────────────

/// A single hook definition, typically originating from a skill's frontmatter
/// or registered programmatically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookDef {
    /// Unique name for this hook.
    pub name: String,
    /// When to fire.
    pub phase: HookPhase,
    /// Optional tool ID glob/prefix filter. Empty or `"*"` matches all tools.
    #[serde(default)]
    pub tool_filter: Option<String>,
    /// Effect type: "deny", "inject", "log"
    #[serde(default = "default_effect_type")]
    pub effect_type: String,
    /// Effect payload (deny reason, instructions to inject, log message)
    #[serde(default)]
    pub effect_payload: String,
    /// Human-readable description.
    #[serde(default)]
    pub description: String,
    /// Name of the skill that owns this hook (populated at registration time).
    #[serde(skip)]
    pub source_skill: Option<String>,
}

fn default_effect_type() -> String {
    "allow".into()
}

// ── Hook Results ────────────────────────────────────────────

/// Result of running before-hooks.
#[derive(Debug, Clone)]
pub struct BeforeHookResult {
    /// Whether execution is allowed to continue.
    pub allowed: bool,
    /// If denied, the reason.
    pub deny_reason: Option<String>,
    /// Instructions injected by matching hooks.
    pub injected_instructions: Vec<String>,
    /// Log messages from matching hooks.
    pub logs: Vec<String>,
    /// Names of hooks that matched.
    pub matched_hooks: Vec<String>,
}

/// Result of running after-hooks.
#[derive(Debug, Clone)]
pub struct AfterHookResult {
    /// Instructions injected by matching hooks.
    pub injected_instructions: Vec<String>,
    /// Log messages from matching hooks.
    pub logs: Vec<String>,
    /// Names of hooks that matched.
    pub matched_hooks: Vec<String>,
}
