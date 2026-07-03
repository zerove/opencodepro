//! Cognitive execution — iterative predict → execute → reflect → evolve loop.
//!
//! Contains the loop state types (`CognitiveIteration`, `CognitiveReport`),
//! loop constants, and the `execute_with_cognition` method on `SisyphusKernel`.

use petgraph::graph::NodeIndex;
use std::sync::Arc;
use std::time::{Instant, SystemTime};
use tokio::sync::mpsc;

use sisyphus_metrics::record_cognitive_iteration;
use sisyphus_types::AgentMessage;

use sisyphus_types::StateStore;

use crate::{CoreError, ExecutionContext, ExecutionEvent, NodeOutput, SisyphusKernel, SOPGraph};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single iteration in a cognitive execution cycle.
pub struct CognitiveIteration {
    pub iteration: usize,
    pub prediction: sisyphus_intel::SimulationResult,
    pub deviation_score: f64,
    pub suggestions: Vec<String>,
    pub evolved: bool,
}

/// Full report from a recursive cognitive execution.
pub struct CognitiveReport {
    pub iterations: usize,
    pub converged: bool,
    pub final_deviation: f64,
    pub history: Vec<CognitiveIteration>,
}

// ---------------------------------------------------------------------------
// SisyphusKernel — cognitive loop
// ---------------------------------------------------------------------------

impl SisyphusKernel {
    /// Execute a SOP graph with recursive cognitive feedback.
    ///
    /// Loop: predict → execute → reflect → evolve
    ///   - If deviation < CONVERGENCE_THRESHOLD (0.3): stop, converged.
    ///   - If deviation >= threshold AND evolved graph available: re-execute.
    ///   - Max MAX_COGNITIVE_ITERATIONS (3) iterations.
    ///
    /// `initial_ctx` can carry cross-graph dependency outputs into the
    /// first iteration's execution context.
    ///
    /// Returns (result, report_string, opt_evolved_graph).
    pub async fn execute_with_cognition(
        &self,
        sop_graph: &SOPGraph,
        sop_id: &str,
        initial_ctx: Option<Arc<ExecutionContext>>,
        event_tx: Option<mpsc::UnboundedSender<ExecutionEvent>>,
    ) -> Result<(NodeOutput, String, Option<SOPGraph>), CoreError> {
        let span = tracing::info_span!("kernel.execute_with_cognition", sop_id = %sop_id, graph_nodes = sop_graph.graph.node_count());
        let _enter = span.enter();

        let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs();

        // Initialize active execution status
        self.active_executions.insert(sop_id.to_string(), crate::ExecutionStatus {
            goal_id: sop_id.to_string(),
            status: "Initializing".into(),
            current_node: None,
            thought_logs: vec!["Starting cognitive loop...".into()],
            progress: 0.0,
            last_updated: now,
        });

        self.broadcast_event(sop_id, ExecutionEvent::CognitiveStep {
            step: "Initializing".into(),
            message: "Starting cognitive loop...".into(),
        });

        let max_iterations = self.cognitive_loop_config.max_iterations as usize;
        let mut own_graph: Option<SOPGraph>;
        let mut current_ref: &SOPGraph = sop_graph;
        let mut current_start = sop_graph.start_node;
        let mut iterations: Vec<CognitiveIteration> = Vec::with_capacity(max_iterations);
        let mut final_result = NodeOutput::default();
        let mut final_evolved: Option<SOPGraph> = None;
        // initial_ctx is passed to the executor on the first iteration only;
        // subsequent iterations (evolved graphs) use fresh contexts.
        let initial_ctx_for_iter = initial_ctx;

        // Publish cognitive loop start event via AgentChannel
        let _ = self.channel.publish(
            "cognitive_events",
            AgentMessage {
                from: "kernel".into(),
                to: "*".into(),
                kind: "CognitiveLoopStarted".into(),
                payload: serde_json::json!({
                    "sop_id": sop_id,
                    "graph_nodes": sop_graph.graph.node_count(),
                    "max_iterations": self.cognitive_loop_config.max_iterations,
                }),
                correlation_id: format!("{}-start", sop_id),
            },
        );

        for iteration in 0..max_iterations {
            let iter_start = Instant::now();
            let iter_span = tracing::info_span!("cognitive_iteration", iteration, sop_id = %sop_id);
            let _iter_enter = iter_span.enter();

            // Publish iteration start via AgentChannel
            let _ = self.channel.publish(
                "cognitive_events",
                AgentMessage {
                    from: "kernel".into(),
                    to: "*".into(),
                    kind: "IterationStarted".into(),
                    payload: serde_json::json!({
                        "sop_id": sop_id,
                        "iteration": iteration,
                    }),
                    correlation_id: format!("{}-iter-{}", sop_id, iteration),
                },
            );

            let ctx = ExecutionContext::with_capacity(current_ref.graph.node_count());

            // 1. Predict
            let iter_now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs();
            if let Some(mut status) = self.active_executions.get_mut(sop_id) {
                status.status = "Predicting".into();
                status.thought_logs.push(format!("Iteration {}: Simulating potential paths...", iteration));
                status.last_updated = iter_now;
            }
            self.broadcast_event(sop_id, ExecutionEvent::CognitiveStep {
                step: "Predicting".into(),
                message: format!("Iteration {}: Simulating potential paths...", iteration),
            });

            let pred_start = Instant::now();
            let prediction = self
                .intel
                .simulator
                .simulate(current_ref, &ctx, sop_id)
                .await
                .map_err(|e| CoreError::Internal(e.to_string()))?;
            let pred_duration = pred_start.elapsed();
            tracing::debug!(path_id = %prediction.path_id, success_prob = prediction.success_probability, estimated_cost = prediction.estimated_cost, duration_ms = pred_duration.as_millis(), "Simulation completed");

            let exec_ctx = if iteration == 0 {
                initial_ctx_for_iter.clone()
            } else {
                None
            };

            if let Some(mut status) = self.active_executions.get_mut(sop_id) {
                status.status = "Executing".into();
                status.thought_logs.push(format!("Iteration {}: Executing SOP nodes...", iteration));
                status.last_updated = iter_now;
            }
            self.broadcast_event(sop_id, ExecutionEvent::CognitiveStep {
                step: "Executing".into(),
                message: format!("Iteration {}: Executing SOP nodes...", iteration),
            });

            let final_ctx = self
                .executor
                .execute_with_context(current_ref, current_start, exec_ctx, event_tx.clone())
                .await
                .map_err(|e| CoreError::Internal(e.to_string()))?;
            let start_name = current_ref
                .graph
                .node_weight(current_start)
                .map(|n| n.name().to_string())
                .ok_or_else(|| CoreError::Internal(format!("start node {:?} not found in graph", current_start)))?;
            let result = final_ctx
                .get_result(&start_name)
                .ok_or(CoreError::NoResultProduced)?;

            if let Some(mut status) = self.active_executions.get_mut(sop_id) {
                status.current_node = Some(start_name.clone());
                status.progress = (iteration + 1) as f32 / max_iterations as f32 * 100.0;
                status.last_updated = iter_now;
            }
            self.broadcast_event(sop_id, ExecutionEvent::NodeCompleted {
                node_name: start_name.clone(),
                output: result.clone(),
            });

            final_result = result.clone();

            // 3. Build per-node trace records
            let mut trace_lines: Vec<String> = Vec::with_capacity(current_ref.graph.node_count());
            for node in current_ref.graph.node_indices() {
                let Some(nw) = current_ref.graph.node_weight(node) else {
                    tracing::warn!(node_idx = ?node, "Skipping trace record for missing node");
                    continue;
                };
                let name = nw.name().to_string();
                let output = final_ctx
                    .get_result(&name)
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "no_result".into());
                let status = if output.contains("ResourceExhausted") {
                    "error"
                } else {
                    "ok"
                };
                let record = serde_json::json!({
                    "node": name,
                    "status": status,
                    "output": output,
                });
                trace_lines.push(record.to_string());
            }
            let trace = trace_lines.join("\n");

            // 4. Reflect
            if let Some(mut status) = self.active_executions.get_mut(sop_id) {
                status.status = "Reflecting".into();
                status.thought_logs.push(format!("Iteration {}: Reflecting on execution trace...", iteration));
                status.last_updated = iter_now;
            }
            self.broadcast_event(sop_id, ExecutionEvent::CognitiveStep {
                step: "Reflecting".into(),
                message: format!("Iteration {}: Reflecting on execution trace...", iteration),
            });

            let reflect_start = Instant::now();
            let reflection = self.intel.reflector.reflect(&trace, &prediction).await
                .map_err(|e| CoreError::Internal(e.to_string()))?;
            let reflect_duration = reflect_start.elapsed();
            tracing::debug!(
                deviation = reflection.deviation_score,
                suggestions = reflection.suggested_improvements.len(),
                duration_ms = reflect_duration.as_millis(),
                "Reflection completed"
            );

            // Persist history
            let history_key = format!("history:{}", sop_id);
            let mut history: sisyphus_types::ExecutionHistory = self
                .state_store
                .get_state_json(&history_key)
                .await
                .map_err(|e| CoreError::Internal(format!("MFS history load error: {}", e)))?
                .and_then(|val| serde_json::from_value(val).ok())
                .unwrap_or_else(|| sisyphus_types::ExecutionHistory {
                    sop_id: sop_id.to_string(),
                    records: Vec::new(),
                });

            history.records.push(sisyphus_types::HistoryRecord {
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                trace: trace.clone(),
                deviation_score: reflection.deviation_score,
                success: !trace.contains("ResourceExhausted"),
                cost: 10 * (trace.lines().count() as u64),
            });

            // Keep only the last 100 records to bound storage growth
            const MAX_HISTORY_RECORDS: usize = 100;
            if history.records.len() > MAX_HISTORY_RECORDS {
                let excess = history.records.len() - MAX_HISTORY_RECORDS;
                history.records.drain(0..excess);
            }

            if let Err(e) = self
                .state_store
                .set_state_json(&history_key, serde_json::to_value(&history).map_err(|e| CoreError::Internal(format!("Failed to serialize execution history: {e}")))?)
                .await
            {
                tracing::warn!(
                    sop_id = %sop_id,
                    error = %e,
                    "Failed to persist execution history"
                );
            }

            // 5. Evolve
            if let Some(mut status) = self.active_executions.get_mut(sop_id) {
                status.status = "Evolving".into();
                status.thought_logs.push(format!("Iteration {}: Evolving graph based on reflection...", iteration));
                status.last_updated = iter_now;
            }
            self.broadcast_event(sop_id, ExecutionEvent::CognitiveStep {
                step: "Evolving".into(),
                message: format!("Iteration {}: Evolving graph based on reflection...", iteration),
            });

            let evolve_start = Instant::now();
            let evolved = self
                .intel
                .evolver
                .evolve(current_ref, &reflection, &prediction)
                .ok();
            let evolve_duration = evolve_start.elapsed();
            let has_evolved = evolved.is_some();
            tracing::debug!(
                evolved = has_evolved,
                duration_ms = evolve_duration.as_millis(),
                "Evolution completed"
            );

            if let Some(ref eg) = evolved {
                if let Err(e) = self.save_sop(sop_id, eg).await {
                    tracing::warn!(sop_id = %sop_id, error = %e, "Failed to persist evolved graph");
                }
            }

            let iter_duration = iter_start.elapsed();
            iterations.push(CognitiveIteration {
                iteration,
                prediction: prediction.clone(),
                deviation_score: reflection.deviation_score,
                suggestions: reflection.suggested_improvements,
                evolved: has_evolved,
            });

            // Record cognitive iteration metric
            record_cognitive_iteration(
                sop_id,
                iteration,
                reflection.deviation_score,
                reflection.deviation_score < self.cognitive_loop_config.convergence_threshold,
                iter_duration.as_secs_f64(),
            );

            // Record iteration timing to benchmark
            let success = !trace.contains("ResourceExhausted");
            if let Ok(mut bm) = self.benchmark.lock() {
                let run = sisyphus_types::BenchmarkRun {
                    name: format!("cognitive_iter.{}.{}", sop_id, iteration),
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    duration_secs: iter_duration.as_secs_f64(),
                    success,
                    metrics: serde_json::json!({
                        "deviation": reflection.deviation_score,
                        "has_evolved": has_evolved,
                        "prediction_success_prob": prediction.success_probability,
                    }),
                };
                let _ = bm.record_result(run);
            }

            // Record iteration outcome to meta_learner
            if let Ok(mut ml) = self.meta_learner.lock() {
                let sample = sisyphus_types::MetaLearningSample {
                    context: serde_json::json!({
                        "sop_id": sop_id,
                        "iteration": iteration,
                        "deviation_score": reflection.deviation_score,
                        "graph_nodes": current_ref.graph.node_count(),
                        "estimated_cost": prediction.estimated_cost,
                    }),
                    strategy: if has_evolved { "evolve" } else { "keep" }.into(),
                    success,
                    duration_secs: iter_duration.as_secs_f64(),
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                };
                let _ = ml.record_outcome(sample);
            }

            // Check convergence
            if reflection.deviation_score < self.cognitive_loop_config.convergence_threshold || !has_evolved {
                final_evolved = evolved;
                break;
            }

            // Re-execute with evolved graph
            if let Some(eg) = evolved {
                current_start = eg.start_node;
                own_graph = Some(eg);
                // SAFETY: own_graph was just set to Some(eg) on the line above
                current_ref = own_graph.as_ref().ok_or_else(|| CoreError::Internal("own_graph should be Some after evolution".into()))?;
            } else {
                break;
            }
        }

        let converged = iterations
            .last()
            .map(|i| i.deviation_score < self.cognitive_loop_config.convergence_threshold)
            .unwrap_or(false);

        // Publish cognitive loop completion via AgentChannel
        let _ = self.channel.publish(
            "cognitive_events",
            AgentMessage {
                from: "kernel".into(),
                to: "*".into(),
                kind: "CognitiveLoopCompleted".into(),
                payload: serde_json::json!({
                    "sop_id": sop_id,
                    "iterations": iterations.len(),
                    "converged": converged,
                    "final_deviation": iterations.last().map(|i| i.deviation_score).unwrap_or(0.0),
                }),
                correlation_id: format!("{}-complete", sop_id),
            },
        );

        // Check benchmark baseline for regression
        if let Ok(bm) = self.benchmark.lock() {
            if let Some(comparison) = bm.compare_with_baseline(&format!(
                "cognitive_iter.{}.{}",
                sop_id,
                iterations.len().saturating_sub(1)
            )) {
                if comparison.is_significant {
                    tracing::warn!(
                        sop_id = %sop_id,
                        regression_pct = comparison.regression_pct,
                        "Performance regression detected in cognitive iteration"
                    );
                }
            }
        }

        // Learn from this execution and persist insight
        if let Ok(ml) = self.meta_learner.lock() {
            let insight = ml.learn();
            let _ = ml.save_insight(&insight);
            // Publish insight summary via AgentChannel
            let _ = self.channel.publish(
                "cognitive_events",
                AgentMessage {
                    from: "kernel".into(),
                    to: "*".into(),
                    kind: "LearningInsight".into(),
                    payload: serde_json::json!({
                        "sop_id": sop_id,
                        "total_samples": insight.total_samples,
                        "overall_success_rate": insight.overall_success_rate,
                        "trend": insight.trend,
                        "strategies": insight.strategies.iter().map(|s| serde_json::json!({
                            "strategy": s.strategy,
                            "success_rate": s.success_rate,
                            "total_uses": s.total_uses,
                        })).collect::<Vec<_>>(),
                    }),
                    correlation_id: format!("{}-insight", sop_id),
                },
            );
        }

        let iteration_details: Vec<String> = iterations
            .iter()
            .map(|i| {
                format!(
                    "iter[{}]: deviation={:.2}, evolved={}, suggestions={:?}",
                    i.iteration, i.deviation_score, i.evolved, i.suggestions
                )
            })
            .collect();

        let report = format!(
            "CognitiveLoop: {} iterations, converged={} | {}",
            iterations.len(),
            converged,
            iteration_details.join(" -> ")
        );

        tracing::info!(
            iterations = iterations.len(),
            converged,
            final_deviation = iterations.last().map(|i| i.deviation_score).unwrap_or(0.0),
            "Cognitive loop completed"
        );

        if let Some(mut status) = self.active_executions.get_mut(sop_id) {
            status.status = "Completed".into();
            status.progress = 100.0;
            status.thought_logs.push("Cognitive loop converged and completed.".into());
            status.last_updated = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs();
        }
        self.broadcast_event(sop_id, ExecutionEvent::CognitiveComplete {
            report: report.clone(),
        });

        Ok((final_result, report, final_evolved))
    }
}
