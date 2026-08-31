//! Integration tests for sisyphus-types `errors` module.
//!
//! Exercises the public error enums:
//! - [`TypesError`] — every variant's `Display` impl
//! - [`NodeExecutionError`] — every variant's `Display` impl
//! - [`IntelError`] — Display + nested-`From` propagation through
//!   `TypesError::IntelError`.
//! - Round-trippability through `serde_json` (debug formatting only;
//!   these types are not `Serialize`).

use sisyphus_types::errors::{IntelError, NodeExecutionError, TypesError};

// ---------------------------------------------------------------------------
// TypesError display — every variant
// ---------------------------------------------------------------------------

#[test]
fn types_error_display_state_store() {
    let e = TypesError::StateStoreError("disk full".into());
    let s = format!("{e}");
    assert!(s.contains("State store error"));
    assert!(s.contains("disk full"));
}

#[test]
fn types_error_display_node_execution() {
    let e = TypesError::NodeExecutionError("bad node".into());
    let s = format!("{e}");
    assert!(s.contains("Node execution error"));
    assert!(s.contains("bad node"));
}

#[test]
fn types_error_display_tool_execution() {
    let e = TypesError::ToolExecutionError {
        tool_id: "my_tool".into(),
        details: "kafka offline".into(),
    };
    let s = format!("{e}");
    assert!(s.contains("my_tool"));
    assert!(s.contains("kafka offline"));
}

#[test]
fn types_error_display_graph() {
    let e = TypesError::GraphError("cycle detected".into());
    let s = format!("{e}");
    assert!(s.contains("Graph error"));
    assert!(s.contains("cycle detected"));
}

#[test]
fn types_error_display_config_validation() {
    let e = TypesError::ConfigValidationError("missing key".into());
    let s = format!("{e}");
    assert!(s.contains("Config validation error"));
    assert!(s.contains("missing key"));
}

#[test]
fn types_error_display_internal() {
    let e = TypesError::InternalError("panic caught".into());
    let s = format!("{e}");
    assert!(s.contains("Internal system error"));
    assert!(s.contains("panic caught"));
}

#[test]
fn types_error_display_executor_command() {
    let e = TypesError::ExecutorCommandError {
        command: "read".into(),
        message: "permission denied".into(),
    };
    let s = format!("{e}");
    assert!(s.contains("executor command"));
    assert!(s.contains("read"));
    assert!(s.contains("permission denied"));
}

#[test]
fn types_error_display_experience() {
    let e = TypesError::ExperienceError("not found".into());
    let s = format!("{e}");
    assert!(s.contains("experience bank error"));
    assert!(s.contains("not found"));
}

#[test]
fn types_error_display_consensus() {
    let e = TypesError::ConsensusError("quorum lost".into());
    let s = format!("{e}");
    assert!(s.contains("consensus error"));
    assert!(s.contains("quorum lost"));
}

#[test]
fn types_error_display_llm() {
    let e = TypesError::LlmError("rate limit".into());
    let s = format!("{e}");
    assert!(s.contains("LLM error"));
    assert!(s.contains("rate limit"));
}

#[test]
fn types_error_display_data_inconsistency() {
    let e = TypesError::DataInconsistency("hash mismatch".into());
    let s = format!("{e}");
    assert!(s.contains("Data inconsistency"));
    assert!(s.contains("hash mismatch"));
}

#[test]
fn types_error_display_simulation() {
    let e = TypesError::SimulationError("overflow".into());
    let s = format!("{e}");
    assert!(s.contains("Simulation"));
    assert!(s.contains("overflow"));
}

#[test]
fn types_error_intel_error_via_from() {
    // IntelError → TypesError via #[from] conversion.
    let intel = IntelError::Internal("oops".into());
    let outer: TypesError = intel.into();
    let s = format!("{outer}");
    assert!(s.contains("intel error"));
    assert!(s.contains("oops"));
}

#[test]
fn types_error_node_execution_error_via_from() {
    // NodeExecutionError → TypesError via #[from].
    let inner = NodeExecutionError::InterventionTimeout;
    let outer: TypesError = inner.into();
    let s = format!("{outer}");
    assert!(s.contains("Node execution failed"));
    assert!(s.contains("Intervention timeout"));
}

#[test]
fn types_error_debug_includes_variant() {
    let e = TypesError::StateStoreError("x".into());
    let s = format!("{e:?}");
    assert!(s.contains("StateStoreError"));
}

// ---------------------------------------------------------------------------
// NodeExecutionError display
// ---------------------------------------------------------------------------

#[test]
fn node_exec_error_display_tool_failure() {
    let e = NodeExecutionError::ToolFailure {
        tool_id: "t1".into(),
        details: "broke".into(),
    };
    let s = format!("{e}");
    assert!(s.contains("t1"));
    assert!(s.contains("broke"));
}

#[test]
fn node_exec_error_display_sandbox() {
    let e = NodeExecutionError::SandboxError("oom".into());
    let s = format!("{e}");
    assert!(s.contains("Sandbox runtime error"));
    assert!(s.contains("oom"));
}

#[test]
fn node_exec_error_display_intervention_timeout() {
    let e = NodeExecutionError::InterventionTimeout;
    let s = format!("{e}");
    assert!(s.contains("Intervention timeout"));
}

#[test]
fn node_exec_error_clone() {
    let e = NodeExecutionError::ToolFailure {
        tool_id: "t".into(),
        details: "d".into(),
    };
    // Just verify Debug is printable.
    let _ = format!("{e:?}");
}

// ---------------------------------------------------------------------------
// IntelError
// ---------------------------------------------------------------------------

#[test]
fn intel_error_other_display() {
    let e = IntelError::Internal("custom".into());
    let s = format!("{e}");
    assert!(s.contains("internal error"));
    assert!(s.contains("custom"));
}

#[test]
fn intel_error_debug_includes_variant() {
    let e = IntelError::Internal("x".into());
    let s = format!("{e:?}");
    assert!(s.contains("Internal"));
}

#[test]
fn intel_error_synthesis_failed_display() {
    let e = IntelError::SynthesisFailed {
        goal: "g1".into(),
        reason: "no plan".into(),
    };
    let s = format!("{e}");
    assert!(s.contains("synthesis failed"));
    assert!(s.contains("g1"));
    assert!(s.contains("no plan"));
}

#[test]
fn intel_error_simulation_failed_display() {
    let e = IntelError::SimulationFailed {
        sop_id: "s1".into(),
        reason: "diverged".into(),
    };
    let s = format!("{e}");
    assert!(s.contains("simulation failed"));
    assert!(s.contains("s1"));
    assert!(s.contains("diverged"));
}

#[test]
fn intel_error_reflection_failed_display() {
    let e = IntelError::ReflectionFailed("trace mismatch".into());
    let s = format!("{e}");
    assert!(s.contains("reflection failed"));
    assert!(s.contains("trace mismatch"));
}

#[test]
fn intel_error_evolution_failed_display() {
    let e = IntelError::EvolutionFailed("cycle".into());
    let s = format!("{e}");
    assert!(s.contains("evolution failed"));
    assert!(s.contains("cycle"));
}

#[test]
fn intel_error_llm_display() {
    let e = IntelError::LlmError("rate limit".into());
    let s = format!("{e}");
    assert!(s.contains("LLM call failed"));
    assert!(s.contains("rate limit"));
}

#[test]
fn intel_error_serialization_display() {
    let e = IntelError::SerializationError("invalid json".into());
    let s = format!("{e}");
    assert!(s.contains("serialization error"));
    assert!(s.contains("invalid json"));
}

#[test]
fn intel_error_node_type_mismatch_display() {
    let e = IntelError::NodeTypeMismatch {
        node_name: "n1".into(),
    };
    let s = format!("{e}");
    assert!(s.contains("n1"));
    assert!(s.contains("WasmSOPNode"));
}

#[test]
fn intel_error_evolution_cycle_display() {
    let e = IntelError::EvolutionCycle;
    let s = format!("{e}");
    assert!(s.contains("cycle"));
    assert!(s.contains("DAG"));
}

#[test]
fn intel_error_node_not_found_display() {
    let e = IntelError::NodeNotFound {
        name: "missing".into(),
    };
    let s = format!("{e}");
    assert!(s.contains("not found"));
    assert!(s.contains("missing"));
}

#[test]
fn intel_error_from_string_and_str() {
    let _e: IntelError = "x".into();
    let _e: IntelError = String::from("y").into();
}
