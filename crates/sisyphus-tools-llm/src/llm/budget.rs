//! Budget provider trait and request context for LLM cost tracking.

/// Context used to attribute LLM costs to specific roadmap goals.
#[derive(Debug, Clone)]
pub struct RequestContext {
    pub project_id: String,
    pub goal_id: String,
}
