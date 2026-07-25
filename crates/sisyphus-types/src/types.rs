use crate::experience::ExperienceTuple;
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CognitiveEvent {
    Predict {
        prediction: serde_json::Value,
    },
    Execute {
        node: String,
        result: crate::execution::NodeOutput,
    },
    Reflect {
        report: serde_json::Value,
    },
    Evolve {
        revised_plan: crate::graph::SOPGraphConfig,
    },
    Verify {
        score: f64,
        decision: bool,
        reason: String,
    },
    StrategicShift {
        analysis: serde_json::Value,
    },
    ToolGapDetected {
        gap: serde_json::Value,
    },
    HotStart {
        asset_id: String,
        confidence: f64,
    },
    Complete {
        final_result: crate::execution::NodeOutput,
        converged: bool,
    },
    // Self-Programming Lifecycle
    SimulationStarted {
        sop_id: String,
        iteration: usize,
    },
    SimulationFlawDetected {
        node_id: String,
        flaw: String,
        severity: String,
    },
    SimulationPassed {
        confidence: f64,
    },
    ToolGenerationStarted {
        tool_id: String,
        capability: String,
    },
    ToolVerified {
        tool_id: String,
        score: f64,
    },
    ToolDeployed {
        tool_id: String,
        asset_id: String,
    },
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
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
pub enum ModelRouteStrategy {
    /// Pick the cheapest model that meets the complexity threshold.
    CostOptimized,
    /// Always use the most capable model.
    PerformanceOptimized,
    /// Try models in order; fall back to the next on failure.
    FallbackChain,
}

/// State of a circuit breaker protecting a resource.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
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

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Default)]
pub enum WorkMode {
    /// Standard mode: Balanced speed and accuracy.
    #[default]
    Standard,
    /// UltraWork mode: Zero-compromise, evidence-bound, high-rigor mode.
    UltraWork,
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
    pub params: Value,
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

    /// Appends an insight to a specific section with a timestamp.
    pub fn append_insight(&mut self, section_id: String, insight: String) {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let entry = format!("[{}] {}\n", timestamp, insight);

        let current_content = self.sections.get(&section_id).cloned().unwrap_or_default();
        let new_content = format!("{}{}", current_content, entry);
        self.sections.insert(section_id, new_content);
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

    pub fn prune(&mut self, max_insights: usize) {
        let mut to_update = Vec::new();
        for (id, content) in self.sections.iter() {
            let lines: Vec<&str> = content.lines().collect();
            if lines.len() > max_insights {
                let pruned_content = lines[lines.len() - max_insights..].join("\n");
                to_update.push((id.clone(), pruned_content));
            }
        }
        for (id, content) in to_update {
            self.sections.insert(id, content);
        }
    }
}

/// A trigger definition, corresponding to triggers in SKILL.md frontmatter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerDef {
    /// Trigger keyword/regex pattern.
    pub pattern: Option<String>,
    /// Associated tool names (empty means all).
    #[serde(default)]
    pub tools: Vec<String>,
    /// Trigger description.
    pub description: Option<String>,
    /// Hook phase: "before" or "after".
    #[serde(default)]
    pub hook_phase: Option<String>,
    /// Hook effect: "deny", "inject", "log".
    #[serde(default)]
    pub hook_effect: Option<String>,
    /// Hook message/payload.
    #[serde(default)]
    pub hook_message: Option<String>,
}

#[async_trait::async_trait]
pub trait ExperienceRetriever: Send + Sync {
    async fn retrieve_relevant(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ExperienceTuple>, crate::errors::TypesError>;
    async fn record_experience(
        &self,
        exp: ExperienceTuple,
    ) -> Result<(), crate::errors::TypesError>;
}
