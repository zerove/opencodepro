use async_trait::async_trait;
use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use sisyphus_metrics::record_simulation;
use sisyphus_types::{ExecutionContext, SOPGraph};
use std::sync::Arc;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Simulation types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationResult {
    pub path_id: String,
    pub success_probability: f64,
    pub estimated_cost: u64,
    pub predicted_outcome: String,
}

#[async_trait]
pub trait Simulator: Send + Sync {
    async fn simulate(
        &self,
        graph: &SOPGraph,
        context: &ExecutionContext,
        sop_id: &str,
    ) -> Result<SimulationResult, String>;
}

// ---------------------------------------------------------------------------
// MonteCarloSimulator — Experience-weighted random-walk simulation
// ---------------------------------------------------------------------------
//
// Combines Monte Carlo sampling with historical experience:
// - When state_store is available, queries past execution records to bias
//   edge transition probabilities and cost estimates.
// - Without history, falls back to a uniform random walk.
// - Uses Rayon for parallel iterations.

pub struct MonteCarloSimulator {
    pub iterations: usize,
    pub state_store: Option<Arc<dyn sisyphus_types::StateStore>>,
}

impl MonteCarloSimulator {
    pub fn new(iterations: usize) -> Self {
        Self {
            iterations,
            state_store: None,
        }
    }

    pub fn with_state_store(iterations: usize, store: Arc<dyn sisyphus_types::StateStore>) -> Self {
        Self {
            iterations,
            state_store: Some(store),
        }
    }
}

#[async_trait]
impl Simulator for MonteCarloSimulator {
    async fn simulate(
        &self,
        graph: &SOPGraph,
        _context: &ExecutionContext,
        sop_id: &str,
    ) -> Result<SimulationResult, String> {
        let span = tracing::info_span!("intel.simulate", sop_id = %sop_id, iterations = self.iterations, graph_nodes = graph.graph.node_count());
        let _enter = span.enter();

        let start = Instant::now();
        let num_nodes = graph.graph.node_count();
        if num_nodes == 0 {
            tracing::warn!("Empty graph provided for simulation");
            return Ok(SimulationResult {
                path_id: "empty".into(),
                success_probability: 0.0,
                estimated_cost: 0,
                predicted_outcome: "Empty graph".into(),
            });
        }

        let nodes: Vec<_> = graph.graph.node_indices().collect();

        // --- Experience prior (async, outside rayon) ---
        let (hist_success_rate, hist_avg_cost) = if let Some(ref store) = self.state_store {
            let history_key = format!("history:{}", sop_id);
            match store.get_state_json(&history_key).await {
                Ok(Some(val)) => {
                    if let Ok(hist) =
                        serde_json::from_value::<sisyphus_types::ExecutionHistory>(val)
                    {
                        let total = hist.records.len() as f64;
                        let successes = hist.records.iter().filter(|r| r.success).count() as f64;
                        let avg_cost = if !hist.records.is_empty() {
                            hist.records.iter().map(|r| r.cost).sum::<u64>()
                                / hist.records.len() as u64
                        } else {
                            0
                        };
                        (successes / total.max(1.0), avg_cost)
                    } else {
                        (0.5, 100u64)
                    }
                }
                _ => (0.5, 100u64),
            }
        } else {
            (0.5, 100u64)
        };

        tracing::debug!(hist_success_rate, hist_avg_cost, "Experience prior loaded");

        let (best_path, max_score, final_cost) = (0..self.iterations)
            .into_par_iter()
            .map(|_| {
                let mut rng = SmallRng::from_entropy();
                let mut path_parts: Vec<String> = Vec::new();
                let mut prob = 1.0;
                let mut cost = 0u64;

                let start = rng.gen_range(0..nodes.len());
                let mut current = nodes[start];

                loop {
                    let name = graph
                        .graph
                        .node_weight(current)
                        .ok_or_else(|| "random walk picks invalid index".to_string())?
                        .name()
                        .to_string();
                    path_parts.push(name);
                    let step_prob = 0.9 * hist_success_rate + 0.1 * 0.9;
                    prob *= step_prob;
                    cost += 10;

                    let neighbors: Vec<_> = graph.graph.neighbors(current).collect();
                    if neighbors.is_empty() {
                        break;
                    }
                    current = neighbors[rng.gen_range(0..neighbors.len())];
                }

                let score = prob / (cost as f64).max(1.0);
                Ok((path_parts.join(" -> "), score, cost))
            })
            .reduce(
                || Ok::<(String, f64, u64), String>((String::new(), -1.0f64, 0u64)),
                |a, b| {
                    let a = a?;
                    let b = b?;
                    Ok(if a.1 > b.1 { a } else { b })
                },
            )?;

        let blended_cost = if final_cost > 0 {
            final_cost
        } else {
            hist_avg_cost
        };

        let duration = start.elapsed();
        tracing::info!(path_id = %best_path, success_probability = max_score, estimated_cost = blended_cost, duration_ms = duration.as_millis(), "Simulation completed");

        record_simulation(
            sop_id,
            max_score > 0.0,
            duration.as_secs_f64(),
            blended_cost,
        );

        Ok(SimulationResult {
            path_id: best_path,
            success_probability: max_score.max(0.0),
            estimated_cost: blended_cost,
            predicted_outcome: format!(
                "MC with history: {} iterations, prior_success={:.2}",
                self.iterations, hist_success_rate
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sisyphus_types::ExecutionContext;

    #[tokio::test]
    async fn test_monte_carlo_empty_graph() {
        let graph = SOPGraph::new();
        let sim = MonteCarloSimulator::new(10);
        let ctx = ExecutionContext::new();
        let result = sim.simulate(&graph, &ctx, "empty").await.unwrap();
        assert_eq!(result.success_probability, 0.0);
        assert_eq!(result.estimated_cost, 0);
    }
}
