// ---------------------------------------------------------------------------
// IntelError — typed error for the intel crate
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum IntelError {
    NodeTypeMismatch { node_name: String },
    EvolutionCycle,
    NodeNotFound { name: String },
}

impl std::fmt::Display for IntelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IntelError::NodeTypeMismatch { node_name } => {
                write!(f, "Node {} is not a WasmSOPNode", node_name)
            }
            IntelError::EvolutionCycle => {
                write!(f, "Evolution resulted in a cycle, violating DAG invariant")
            }
            IntelError::NodeNotFound { name } => {
                write!(f, "Node '{}' not found in graph", name)
            }
        }
    }
}

impl std::error::Error for IntelError {}

impl From<IntelError> for String {
    fn from(e: IntelError) -> Self {
        e.to_string()
    }
}
