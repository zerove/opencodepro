use serde_json::Value;
use std::collections::HashMap;
use tokio::sync::oneshot;

/// Operations that can be buffered for L2 persistence.
#[derive(Debug)]
pub enum WriteOp {
    Set { key: String, value: Value },
    Delete { key: String },
    Flush(oneshot::Sender<anyhow::Result<()>>),
    Clear,
}

/// A point-in-time snapshot of the MFS state.
pub struct Snapshot {
    pub data: HashMap<String, Value>,
}
