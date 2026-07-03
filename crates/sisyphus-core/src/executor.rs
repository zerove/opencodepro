//! DAG executor — topology‑aware node dispatch with event streaming.

use petgraph::algo::toposort;
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio::task::JoinSet;

use crate::{CoreError, ExecutionContext, ExecutionEvent, NodeOutput, SOPGraph};

/// A DAG-based executor that runs SOP graphs with parallel node dispatch
/// and optional event streaming.
///
/// The executor:
/// - Validates the graph is acyclic before starting
/// - Spawns zero-in-degree nodes concurrently
/// - Streams `ExecutionEvent`s (NodeStarted / NodeCompleted / NodeFailed) when a sender is provided
/// - Stores results into a shared `ExecutionContext`
/// - Propagates success through dataflow edges (failure does not halt siblings)
pub struct Executor;

impl Default for Executor {
    fn default() -> Self {
        Self::new()
    }
}

impl Executor {
    pub fn new() -> Self {
        Self
    }

    /// Execute a SOP graph, optionally starting with a pre-populated context
    /// (e.g. with cross-graph dependency outputs injected).
    pub async fn execute_with_context(
        &self,
        sop_graph: &SOPGraph,
        _start_node: NodeIndex,
        initial_ctx: Option<Arc<ExecutionContext>>,
        event_tx: Option<mpsc::UnboundedSender<ExecutionEvent>>,
    ) -> Result<Arc<ExecutionContext>, CoreError> {
        let span = tracing::info_span!(
            "executor.execute",
            graph_nodes = sop_graph.graph.node_count()
        );
        let _enter = span.enter();

        let start = Instant::now();
        if toposort(&sop_graph.graph, None).is_err() {
            tracing::error!("SOPGraph contains cycles");
            return Err(CoreError::CycleDetected);
        }
        let ctx = initial_ctx.unwrap_or_else(|| Arc::new(ExecutionContext::new()));

        let mut join_set = JoinSet::new();
        let mut in_degree = HashMap::new();
        let nodes = sop_graph.graph.node_indices();
        for node in nodes {
            let degree = sop_graph
                .graph
                .edges_directed(node, petgraph::Direction::Incoming)
                .count();
            in_degree.insert(node, degree);
        }

        tracing::debug!(
            "Starting DAG execution with {} ready nodes",
            in_degree.values().filter(|&&d| d == 0).count()
        );

        for (&node, &degree) in in_degree.iter() {
            if degree == 0 {
                let node_ref = match sop_graph.graph.node_weight(node) {
                    Some(n) => Arc::clone(n),
                    None => {
                        tracing::error!(node_idx = ?node, "Node not found in graph during execution");
                        continue;
                    }
                };
                let ctx_ref = Arc::clone(&ctx);
                let tx = event_tx.clone();
                join_set.spawn(async move {
                    let node_span = tracing::info_span!("executor.node", node = node_ref.name());
                    let _enter = node_span.enter();
                    let name = node_ref.name().to_string();

                    if let Some(ref tx) = tx {
                        if let Err(e) = tx.send(ExecutionEvent::NodeStarted { node_name: name.clone() }) {
                            tracing::warn!(node = %name, error = %e, "Failed to send NodeStarted event");
                        }
                    }

                    let result = node_ref.execute(&ctx_ref).await;

                    if let Some(ref tx) = tx {
                        match &result {
                            Ok(out) => {
                                if let Err(e) = tx.send(ExecutionEvent::NodeCompleted {
                                    node_name: name.clone(),
                                    output: out.clone()
                                }) {
                                    tracing::warn!(node = %name, error = %e, "Failed to send NodeCompleted event");
                                }
                            }
                            Err(e) => {
                                if let Err(send_err) = tx.send(ExecutionEvent::NodeFailed {
                                    node_name: name.clone(),
                                    error: e.to_string()
                                }) {
                                    tracing::warn!(node = %name, error = %send_err, "Failed to send NodeFailed event");
                                }
                            }
                        }
                    }

                    tracing::debug!(node = %name, "Node execution completed");
                    (node, name, result)
                });
            }
        }

        while let Some(res) = join_set.join_next().await {
            let (node, name, result) = res.map_err(|e| CoreError::Internal(e.to_string()))?;
            let output = match result {
                Ok(out) => out,
                Err(e) => {
                    tracing::warn!(node = %name, error = %e, "Node execution failed");
                    NodeOutput(Value::String(e.to_string()))
                }
            };
            ctx.set_result(name.clone(), output.clone());

            // --- UltraWork Notepad Sync Bridge ---
            if name.starts_with("Rigorous-") {
                let section_id = name.replace("Rigorous-", "notepad.").to_lowercase();
                ctx.write_notepad(section_id, output.to_string());
                tracing::debug!(node = %name, section = %"notepad", "Auto-synced node output to Notepad");
            }
            tracing::debug!(node = %name, "Node result stored");

            // Handle branching logic: if the node is a BranchSOPNode, only follow the chosen path.
            let chosen_target = if let Some(node_weight) = sop_graph.graph.node_weight(node) {
                if let Some(branch_node) = node_weight.as_any().downcast_ref::<sisyphus_types::BranchSOPNode>() {
                    output.as_f64().map(|idx| NodeIndex::new(idx as usize))
                } else {
                    None
                }
            } else {
                None
            };

            for edge in sop_graph.graph.edges(node) {
                let target = edge.target();
                
                if let Some(chosen) = chosen_target {
                    if target != chosen {
                        if let Some(degree) = in_degree.get_mut(&target) {
                            *degree -= 1;
                        }
                        continue;
                    }
                }

                if let Some(degree) = in_degree.get_mut(&target) {
                    *degree -= 1;
                    if *degree == 0 {
                        let Some(node_ref) = sop_graph.graph.node_weight(target).map(Arc::clone) else {
                            tracing::error!(node_idx = ?target, "Target node not found in graph after edge traversal");
                            continue;
                        };
                        let ctx_ref = Arc::clone(&ctx);
                        let tx = event_tx.clone();
                        join_set.spawn(async move {
                            let node_span =
                                tracing::info_span!("executor.node", node = node_ref.name());
                            let _enter = node_span.enter();
                            let name = node_ref.name().to_string();

                            if let Some(ref tx) = tx {
                                if let Err(e) = tx.send(ExecutionEvent::NodeStarted { node_name: name.clone() }) {
                                    tracing::warn!(node = %name, error = %e, "Failed to send NodeStarted event");
                                }
                            }

                            let result = node_ref.execute(&ctx_ref).await;

                            if let Some(ref tx) = tx {
                                match &result {
                                    Ok(out) => {
                                        if let Err(e) = tx.send(ExecutionEvent::NodeCompleted {
                                            node_name: name.clone(),
                                            output: out.clone()
                                        }) {
                                            tracing::warn!(node = %name, error = %e, "Failed to send NodeCompleted event");
                                        }
                                    }
                                    Err(e) => {
                                        if let Err(send_err) = tx.send(ExecutionEvent::NodeFailed {
                                            node_name: name.clone(),
                                            error: e.to_string()
                                        }) {
                                            tracing::warn!(node = %name, error = %send_err, "Failed to send NodeFailed event");
                                        }
                                    }
                                }
                            }

                            tracing::debug!(node = %name, "Node execution completed");
                            (target, name, result)
                        });
                    }
                }
            }
        }

        let duration = start.elapsed();
        tracing::info!(
            duration_ms = duration.as_millis(),
            "DAG execution completed"
        );
        Ok(ctx)
    }

    /// Execute a SOP graph (backward-compatible wrapper).
    pub async fn execute(
        &self,
        sop_graph: &SOPGraph,
        start_node: NodeIndex,
    ) -> Result<Arc<ExecutionContext>, CoreError> {
        self.execute_with_context(sop_graph, start_node, None, None)
            .await
    }
}
