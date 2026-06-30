use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

/// Feedback from a user or another agent regarding a specific output.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Feedback {
    pub agent_id: String,
    pub original_request: String,
    pub original_output: String,
    pub corrected_output: String,
    pub timestamp: i64,
    pub tags: Vec<String>,
}

/// Descriptor for a runtime-loadable WASM plugin tool.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PluginDescriptor {
    pub name: String,
    pub version: String,
    pub wasm_path: PathBuf,
    pub tool_id: String,
    pub fuel_limit: u64,
    /// SHA-256 hash of the .wasm binary, used for cache invalidation.
    #[serde(default)]
    pub checksum: Option<String>,
}

/// A single vector embedding record for cross-session similarity search.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VectorRecord {
    pub key: String,
    pub embedding: Vec<f32>,
    pub metadata: Value,
    pub timestamp: u64,
}

/// Structured message for inter-agent communication.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AgentMessage {
    pub from: String,
    pub to: String,
    /// Message kind: "request" / "response" / "publish" / "subscribe"
    pub kind: String,
    pub payload: Value,
    /// Correlation ID for pairing request/response pairs.
    pub correlation_id: String,
}

/// Strategy for selecting which LLM model to route a request to.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum ModelRouteStrategy {
    /// Pick the cheapest model that meets the complexity threshold.
    CostOptimized,
    /// Always use the most capable model.
    PerformanceOptimized,
    /// Try models in order; fall back to the next on failure.
    FallbackChain,
}

/// State of a circuit breaker protecting a resource.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

/// A single benchmark measurement.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BenchmarkRun {
    pub name: String,
    pub timestamp: u64,
    pub duration_secs: f64,
    pub success: bool,
    pub metrics: Value,
}

/// Configuration for platform-specific behaviour.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct PlatformConfig {
    /// Whether the current OS is Windows (inferred at runtime or overridden).
    pub is_windows: bool,
    /// Path-style separator (inherited from std::path::MAIN_SEPARATOR).
    pub path_sep: String,
    /// SHELL environment variable value (e.g. "/bin/bash", "powershell.exe").
    pub shell: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum WorkMode {
    /// Standard mode: Balanced speed and accuracy.
    Standard,
    /// UltraWork mode: Zero-compromise, evidence-bound, high-rigor mode.
    UltraWork,
}

impl Default for WorkMode {
    fn default() -> Self {
        Self::Standard
    }
}

/// Topological evolution operation on a SOP graph.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum EvolutionOp {
    /// Increase fuel limit for a named node.
    AdjustFuel { node_name: String, delta: i64 },
    /// Wrap a node with a retry decorator.
    AddRetry { node_name: String, max_retries: u32 },
    /// Insert a new node between two existing nodes.
    InsertNode {
        after_node: String,
        new_name: String,
        tool_id: String,
    },
    /// Remove a node and re-wire edges around it.
    RemoveNode { node_name: String },
    /// Reorder two adjacent nodes.
    SwapNodes { node_a: String, node_b: String },
}

/// Configuration for formal verification runners (kani, loom, proptest).
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct FormalVerificationConfig {
    pub enable_kani: bool,
    pub enable_loom: bool,
    pub enable_proptest: bool,
    pub max_proptest_cases: u32,
}

/// A single data point for meta-learning (strategy → outcome).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MetaLearningSample {
    pub context: Value,
    pub strategy: String,
    pub success: bool,
    pub duration_secs: f64,
    pub timestamp: u64,
}

/// Definition of a custom tool loaded from `.opencode/tools/*.toml`.
///
/// Each custom tool has a name, description, JSON Schema for argument validation,
/// and a compiled WASM binary that handles execution.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CustomToolDefinition {
    /// Tool name (used as tool_id).
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Path to the `.wasm` handler binary.
    pub handler: PathBuf,
    /// JSON Schema for argument validation.
    #[serde(default)]
    pub schema: Value,
    /// Default fuel limit for WASM execution.
    #[serde(default = "default_tool_fuel")]
    pub fuel_limit: u64,
}

fn default_tool_fuel() -> u64 {
    1_000_000
}

use im::HashMap as ImMap;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SisyphusNotepad {
    /// Structural sections of the working memory.
    /// Key: Section ID (e.g., "plan.architecture"), Value: Content.
    /// Using im::HashMap for O(1) structural sharing snapshots.
    pub sections: ImMap<String, String>,
    /// Track changes for regression and auditing.
    pub history: Vec<NotepadSnapshot>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NotepadSnapshot {
    pub timestamp: u64,
    pub snapshot: ImMap<String, String>,
    pub trigger_node: String,
}

impl SisyphusNotepad {
    pub fn write_section(&mut self, id: String, content: String) {
        self.sections.insert(id, content);
    }

    pub fn read_section(&self, id: &str) -> Option<&String> {
        self.sections.get(id)
    }

    pub fn take_snapshot(&mut self, node_name: String) {
        self.history.push(NotepadSnapshot {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            snapshot: self.sections.clone(), // O(1) structural clone
            trigger_node: node_name,
        });
    }
}
