use thiserror::Error;

#[derive(Debug, Error)]
/// Types Error.
pub enum TypesError {
    #[error("Failed to read config file '{path}': {source}")]
    ConfigReadError {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("Failed to parse config file '{path}': {source}")]
    ConfigParseError {
        path: String,
        #[source]
        source: toml::de::Error,
    },
    #[error("State store error: {0}")]
    StateStoreError(String),
    #[error("Node execution error: {0}")]
    NodeExecutionError(String),
    #[error("Tool execution error for '{tool_id}': {details}")]
    ToolExecutionError { tool_id: String, details: String },
    #[error("Graph error: {0}")]
    GraphError(String),
    #[error("Config validation error: {0}")]
    ConfigValidationError(String),
    #[error("Internal system error: {0}")]
    InternalError(String),

    // ── 结构化错误变体 (新增) ──────────────────────────────
    /// 内置工具命令执行失败 (file, bash, web, agent 等)
    #[error("executor command '{command}' failed: {message}")]
    ExecutorCommandError { command: String, message: String },

    /// Intel 层错误 (合成、模拟、反思、演化)
    #[error("intel error: {0}")]
    IntelError(#[from] IntelError),

    /// ExperienceBank 操作失败
    #[error("experience bank error: {0}")]
    ExperienceError(String),

    /// Consensus 协议错误
    #[error("consensus error: {0}")]
    ConsensusError(String),

    /// LLM 客户端调用错误
    #[error("LLM error: {0}")]
    LlmError(String),

    /// --- 扩充后的结构化领域错误 ---
    #[error("Node execution failed: {0}")]
    ExecutionError(#[from] NodeExecutionError),

    #[error("Cognitive loop error: {0}")]
    CognitiveError(#[from] CognitiveError),

    #[error("Data inconsistency: {0}")]
    DataInconsistency(String),

    #[error("Simulation/Predict error: {0}")]
    SimulationError(String),
}

#[derive(Debug, Error)]
/// Node Execution Error.
pub enum NodeExecutionError {
    #[error("Tool '{tool_id}' failure: {details}")]
    ToolFailure { tool_id: String, details: String },
    #[error("Sandbox runtime error: {0}")]
    SandboxError(String),
    #[error("Intervention timeout")]
    InterventionTimeout,
}

#[derive(Debug, Error)]
/// Cognitive Error.
pub enum CognitiveError {
    #[error("Iteration limit exceeded")]
    MaxIterationsReached,
    #[error("Strategy pivot triggered: {0}")]
    StrategicPivot(String),
    #[error("Cognitive mapping failed: {0}")]
    MappingError(String),
}

impl From<String> for TypesError {
    fn from(msg: String) -> Self {
        Self::InternalError(msg)
    }
}

// ── Intel 错误类型 ─────────────────────────────────────────

/// Intel crate 的结构化错误类型，用于 FlowSynthesizer / Simulator / Reflector / SOPEvolver traits。
#[derive(Debug, Error)]
pub enum IntelError {
    #[error("synthesis failed for goal '{goal}': {reason}")]
    SynthesisFailed { goal: String, reason: String },

    #[error("simulation failed for sop '{sop_id}': {reason}")]
    SimulationFailed { sop_id: String, reason: String },

    #[error("reflection failed: {0}")]
    ReflectionFailed(String),

    #[error("evolution failed: {0}")]
    EvolutionFailed(String),

    #[error("LLM call failed: {0}")]
    LlmError(String),

    #[error("serialization error: {0}")]
    SerializationError(String),

    // ── DAG / graph variants (consolidated from sisyphus-intel/error.rs) ──
    #[error("Node {node_name} is not a WasmSOPNode")]
    NodeTypeMismatch { node_name: String },

    #[error("Evolution resulted in a cycle, violating DAG invariant")]
    EvolutionCycle,

    #[error("Node '{name}' not found in graph")]
    NodeNotFound { name: String },

    #[error("internal error: {0}")]
    Internal(String),
}

impl From<String> for IntelError {
    fn from(msg: String) -> Self {
        Self::Internal(msg)
    }
}

impl From<&str> for IntelError {
    fn from(msg: &str) -> Self {
        Self::Internal(msg.to_string())
    }
}
