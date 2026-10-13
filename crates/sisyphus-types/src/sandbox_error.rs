//! Structured error transport for the sandbox daemon protocol.
//!
//! # Why this type exists
//!
//! The daemon used to answer a failed execute with a bare `{"error": "..."}`
//! string, and the client wrapped it in a synthetic
//! `ToolExecutionError { tool_id: "sandboxd" }`. Two things were lost:
//!
//! * **The tool id.** The client overwrote the real failing tool with the name
//!   of the *transport*. A caller saw `tool_id = "sandboxd"` and could not tell
//!   which of its tools had failed.
//! * **The category.** Every failure — fuel exhaustion, a wall-clock deadline, a
//!   guest `unreachable`, a missing `.wasm` file — arrived as the same opaque
//!   string. Callers that wanted to distinguish them had to parse English.
//!
//! [`SandboxErrorKind`] carries the category across the wire, and
//! [`ToolExecutionError`] carries the real tool id. Both degrade gracefully for
//! older daemons, so this can ship without a coordinated upgrade.
//!
//! # Compatibility
//!
//! An older daemon replies with a plain string and no `tool_id`/`kind` fields.
//! [`SandboxErrorKind::from_wire`] maps the absent and unrecognised cases to
//! [`SandboxErrorKind::Unknown`] rather than failing to deserialize, so a new
//! client keeps working against an old daemon and vice versa. The human-readable
//! `message` is always present, so a client that only ever prints it still
//! produces useful output.

use serde::{Deserialize, Serialize};
use std::fmt;

/// What kind of failure stopped a sandboxed tool.
///
/// This is a deliberately small, closed set. It exists so a caller can branch on
/// *why* execution failed without parsing prose — in particular to separate "a
/// limit we imposed fired" (worth retrying with a bigger budget) from "the module
/// is broken" (retrying is pointless).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxErrorKind {
    /// The guest exhausted its fuel budget. Retrying with more fuel may work.
    FuelExhausted,
    /// The guest outran the wall-clock deadline. Retrying on a faster host may
    /// work; the module itself is not at fault.
    WallClockExceeded,
    /// The guest trapped on its own (`unreachable`, out-of-bounds, bad
    /// signature). **Not** a sandbox limit — retrying will fail identically.
    GuestTrap,
    /// The tool's `.wasm` could not be read, compiled, or instantiated.
    ToolLoadFailed,
    /// The module has no `execute` export.
    MissingExport,
    /// The sandbox refused to start or serve.
    SandboxUnavailable,
    /// The daemon could not be reached, or sent something unparseable.
    Transport,
    /// The peer does not speak this protocol version, or reported a category we
    /// do not know. The `message` still carries the original text.
    ///
    /// This is what an old daemon produces, so it is a normal value, not an
    /// error condition.
    #[serde(other)]
    Unknown,
}

impl Default for SandboxErrorKind {
    /// Absent means "the peer did not say", which is exactly
    /// [`SandboxErrorKind::Unknown`] — never a guess at a more specific category.
    fn default() -> Self {
        Self::Unknown
    }
}

impl SandboxErrorKind {
    /// Map a wire value to a kind, never failing.
    ///
    /// Used instead of plain `serde_json::from_value` so that an unrecognised or
    /// missing `kind` degrades to [`SandboxErrorKind::Unknown`] with the message
    /// intact. A new daemon adding a kind must not break an old client.
    pub fn from_wire(value: Option<&serde_json::Value>) -> Self {
        let Some(value) = value else {
            return Self::Unknown;
        };
        // Accept either the kind field of a structured object or a bare kind
        // string. Both degrades to `Unknown` if the text is unrecognised.
        match value {
            serde_json::Value::Object(map) => map
                .get("kind")
                .cloned()
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or(Self::Unknown),
            _ => serde_json::from_value(value.clone()).unwrap_or(Self::Unknown),
        }
    }

    /// True when a sandbox-imposed limit stopped the guest.
    ///
    /// Callers use this to decide whether the failure is tunable. Note that a
    /// `GuestTrap` is *not* a limit, even though it also stops execution —
    /// conflating the two is exactly the bug this type exists to prevent.
    pub fn is_resource_limit(self) -> bool {
        matches!(
            self,
            Self::FuelExhausted | Self::WallClockExceeded | Self::SandboxUnavailable
        )
    }
}

impl fmt::Display for SandboxErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::FuelExhausted => "fuel exhausted",
            Self::WallClockExceeded => "wall-clock deadline exceeded",
            Self::GuestTrap => "guest trap",
            Self::ToolLoadFailed => "tool load failed",
            Self::MissingExport => "missing export",
            Self::SandboxUnavailable => "sandbox unavailable",
            Self::Transport => "transport failure",
            Self::Unknown => "unknown failure",
        };
        f.write_str(s)
    }
}

/// A tool failure as it travels across the sandbox daemon protocol.
///
/// `tool_id` is the tool that actually failed, not the name of the transport.
/// That distinction is the whole point: a caller with twenty registered tools
/// needs to know which one broke.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolExecutionError {
    /// The tool that failed, e.g. `"memory_hog"`.
    ///
    /// Defaults to `"unknown"` because an old daemon does not send it, and a
    /// missing id must not fail deserialization on its own.
    #[serde(default = "unknown_tool_id")]
    pub tool_id: String,

    /// Coarse category. See [`SandboxErrorKind`].
    #[serde(default)]
    pub kind: SandboxErrorKind,

    /// Human-readable detail. Always present so that a caller which only prints
    /// this still says something useful, and so no information is ever lost —
    /// this is the same text the daemon would have sent as a bare string.
    pub message: String,
}

fn unknown_tool_id() -> String {
    "unknown".to_string()
}

impl ToolExecutionError {
    /// Build an error from any displayable source.
    pub fn new(
        tool_id: impl Into<String>,
        kind: SandboxErrorKind,
        message: impl Into<String>,
    ) -> Self {
        Self {
            tool_id: tool_id.into(),
            kind,
            message: message.into(),
        }
    }
}

impl fmt::Display for ToolExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Keep the `ResourceExhausted` token on resource-limit failures: the
        // reflection layer classifies errors by scanning for that exact
        // substring, so dropping it would silently reclassify a fuel limit as an
        // ordinary failure.
        if self.kind.is_resource_limit() {
            write!(
                f,
                "ResourceExhausted: {} failed ({}): {}",
                self.tool_id, self.kind, self.message
            )
        } else {
            write!(
                f,
                "WASM execution failed: {} ({})",
                self.tool_id, self.message
            )
        }
    }
}

impl std::error::Error for ToolExecutionError {}

#[cfg(test)]
mod tests {
    use super::*;

    /// An old daemon's reply has no `kind` and no `tool_id`. It must still parse.
    #[test]
    fn a_legacy_string_only_error_still_deserializes() {
        let raw = serde_json::json!({ "error": "something went wrong" });
        let legacy: LegacyReply = serde_json::from_value(raw).expect("legacy reply must parse");
        let err = ToolExecutionError {
            tool_id: unknown_tool_id(),
            kind: SandboxErrorKind::from_wire(None),
            message: legacy.error,
        };
        assert_eq!(err.kind, SandboxErrorKind::Unknown);
        assert_eq!(err.tool_id, "unknown");
        assert!(err.message.contains("something went wrong"));
    }

    /// A kind this build has never heard of must not fail the parse.
    ///
    /// This is the forward-compatibility contract: adding a variant to the enum
    /// later must not break an already-deployed client.
    #[test]
    fn an_unknown_kind_degrades_instead_of_failing() {
        let raw = serde_json::json!({ "kind": "some_future_kind" });
        assert_eq!(
            SandboxErrorKind::from_wire(Some(&raw)),
            SandboxErrorKind::Unknown
        );
    }

    /// A known kind round-trips, so the happy path is not merely untested.
    #[test]
    fn a_known_kind_round_trips() {
        let raw = serde_json::json!({ "kind": "fuel_exhausted" });
        assert_eq!(
            SandboxErrorKind::from_wire(Some(&raw)),
            SandboxErrorKind::FuelExhausted
        );
        let err = ToolExecutionError::new("evil", SandboxErrorKind::FuelExhausted, "out of fuel");
        let encoded = serde_json::to_value(&err).expect("must serialize");
        assert_eq!(encoded["kind"], "fuel_exhausted");
        assert_eq!(encoded["tool_id"], "evil");
    }

    /// Only genuine limits may present themselves as resource exhaustion.
    ///
    /// A guest trap that reported itself as `ResourceExhausted` would tell the
    /// reflection layer a buggy module had hit a tunable ceiling, which is the
    /// exact misclassification this type was introduced to remove.
    #[test]
    fn only_real_limits_are_labelled_resource_exhausted() {
        let limit = ToolExecutionError::new("loop", SandboxErrorKind::FuelExhausted, "x");
        assert!(limit.to_string().contains("ResourceExhausted"));

        let guest = ToolExecutionError::new("buggy", SandboxErrorKind::GuestTrap, "unreachable");
        assert!(!guest.to_string().contains("ResourceExhausted"));
        assert!(guest.to_string().contains("buggy"));
    }

    #[derive(Deserialize)]
    struct LegacyReply {
        error: String,
    }
}
