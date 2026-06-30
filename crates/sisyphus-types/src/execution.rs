use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use async_trait::async_trait;

use crate::errors::TypesError;

#[async_trait]
pub trait StateStore: Send + Sync {
    async fn get_state_json(&self, key: &str) -> Result<Option<serde_json::Value>, TypesError>;
    async fn set_state_json(&self, key: &str, value: serde_json::Value) -> Result<(), TypesError>;
}

/// Strongly-typed wrapper around `serde_json::Value` for SOP node outputs.
/// Provides type safety and prevents accidental mixing with raw JSON values.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct NodeOutput(pub Value);

impl NodeOutput {
    pub fn into_inner(self) -> Value {
        self.0
    }
}

impl Default for NodeOutput {
    fn default() -> Self {
        NodeOutput(Value::Null)
    }
}

impl From<Value> for NodeOutput {
    fn from(v: Value) -> Self {
        NodeOutput(v)
    }
}

impl std::ops::Deref for NodeOutput {
    type Target = Value;
    fn deref(&self) -> &Value {
        &self.0
    }
}

impl std::ops::DerefMut for NodeOutput {
    fn deref_mut(&mut self) -> &mut Value {
        &mut self.0
    }
}

impl std::fmt::Display for NodeOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl PartialEq<Value> for NodeOutput {
    fn eq(&self, other: &Value) -> bool {
        &self.0 == other
    }
}

impl PartialEq<NodeOutput> for Value {
    fn eq(&self, other: &NodeOutput) -> bool {
        self == &other.0
    }
}

impl PartialEq for NodeOutput {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ExecutionEvent {
    NodeStarted {
        node_name: String,
    },
    NodeCompleted {
        node_name: String,
        output: NodeOutput,
    },
    NodeFailed {
        node_name: String,
        error: String,
    },
    ExecutionComplete {
        result: NodeOutput,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct HistoryRecord {
    pub timestamp: u64,
    pub trace: String,
    pub deviation_score: f64,
    pub success: bool,
    pub cost: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ExecutionHistory {
    pub sop_id: String,
    pub records: Vec<HistoryRecord>,
}

use crate::types::SisyphusNotepad;

#[derive(Debug)]
pub struct ExecutionContext {
    pub results: Arc<RwLock<HashMap<String, NodeOutput>>>,
    pub notepad: Arc<RwLock<SisyphusNotepad>>,
}

impl Default for ExecutionContext {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutionContext {
    pub fn new() -> Self {
        Self {
            results: Arc::new(RwLock::new(HashMap::new())),
            notepad: Arc::new(RwLock::new(SisyphusNotepad::default())),
        }
    }

    /// Creates an ExecutionContext with a pre-allocated results HashMap.
    /// Use when the approximate number of node outputs is known.
    pub fn with_capacity(n: usize) -> Self {
        Self {
            results: Arc::new(RwLock::new(HashMap::with_capacity(n))),
            notepad: Arc::new(RwLock::new(SisyphusNotepad::default())),
        }
    }

    pub fn set_result(&self, node_name: String, output: NodeOutput) {
        match self.results.write() {
            Ok(mut results) => {
                results.insert(node_name, output);
            }
            Err(e) => {
                tracing::warn!(error = %e, "ExecutionContext RwLock poisoned in set_result");
            }
        }
    }

    pub fn get_result(&self, node_name: &str) -> Option<NodeOutput> {
        match self.results.read() {
            Ok(results) => results.get(node_name).cloned(),
            Err(e) => {
                tracing::warn!(error = %e, "ExecutionContext RwLock poisoned in get_result");
                None
            }
        }
    }

    pub fn write_notepad(&self, section: String, content: String) {
        match self.notepad.write() {
            Ok(mut notepad) => {
                notepad.write_section(section, content);
            }
            Err(e) => {
                tracing::warn!(error = %e, "ExecutionContext RwLock poisoned in write_notepad");
            }
        }
    }

    pub fn read_notepad(&self, section: &str) -> Option<String> {
        match self.notepad.read() {
            Ok(notepad) => notepad.read_section(section).cloned(),
            Err(e) => {
                tracing::warn!(error = %e, "ExecutionContext RwLock poisoned in read_notepad");
                None
            }
        }
    }

    pub fn snapshot_notepad(&self, node_name: String) {
        match self.notepad.write() {
            Ok(mut notepad) => {
                notepad.take_snapshot(node_name);
            }
            Err(e) => {
                tracing::warn!(error = %e, "ExecutionContext RwLock poisoned in snapshot_notepad");
            }
        }
    }

    pub fn interpolate(&self, template: &str) -> String {
        let results = match self.results.read() {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, "ExecutionContext RwLock poisoned in interpolate");
                return template.to_string();
            }
        };
        let mut output = template.to_string();
        for (name, node_output) in results.iter() {
            let placeholder = format!("{{{{{}.output}}}}", name);
            let val_str = match &node_output.0 {
                Value::String(s) => s.clone(),
                _ => node_output.0.to_string(),
            };
            output = output.replace(&placeholder, &val_str);
        }
        output
    }
}
