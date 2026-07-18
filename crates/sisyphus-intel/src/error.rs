// ---------------------------------------------------------------------------
// IntelError — typed error for the intel crate
// ---------------------------------------------------------------------------

use thiserror::Error;

#[derive(Debug, Error)]
pub enum IntelError {
    #[error("Node {node_name} is not a WasmSOPNode")]
    NodeTypeMismatch { node_name: String },

    #[error("Evolution resulted in a cycle, violating DAG invariant")]
    EvolutionCycle,

    #[error("Node '{name}' not found in graph")]
    NodeNotFound { name: String },

    #[error("Internal intel error: {0}")]
    Internal(String),
}
