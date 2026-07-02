use std::future::Future;
use std::pin::Pin;

use sisyphus_tools_core::ToolResult;

pub(super) type BoxedFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// A unit of work that can be delegated to a worker.
pub struct WorkUnit {
    pub id: String,
    pub name: String,
    pub handler: Box<dyn FnOnce() -> BoxedFuture<ToolResult<serde_json::Value>> + Send>,
}

impl std::fmt::Debug for WorkUnit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkUnit")
            .field("id", &self.id)
            .field("name", &self.name)
            .finish()
    }
}

/// Status of a work unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

impl std::fmt::Display for WorkStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkStatus::Pending => write!(f, "pending"),
            WorkStatus::Running => write!(f, "running"),
            WorkStatus::Completed => write!(f, "completed"),
            WorkStatus::Failed => write!(f, "failed"),
        }
    }
}

/// Report from a completed work unit.
#[derive(Debug, Clone)]
pub struct WorkReport {
    pub id: String,
    pub name: String,
    pub status: WorkStatus,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
    pub duration_ms: u128,
}

/// Progress notification emitted during execution.
#[derive(Debug, Clone)]
pub enum DelegateEvent {
    Started { id: String, name: String },
    Completed(WorkReport),
    Progress { id: String, message: String },
}

/// Configure and run a batch of work units.
pub struct WorkOrchestrator {
    pub(super) max_concurrent: usize,
}

impl WorkOrchestrator {
    /// Create a new orchestrator with the given max concurrency.
    /// `max_concurrent` is the maximum number of tasks running in parallel.
    /// Use 0 for unlimited (defaults to number of tasks).
    pub fn new(max_concurrent: usize) -> Self {
        Self { max_concurrent }
    }
}
