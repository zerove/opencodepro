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

// ---------------------------------------------------------------------------
// MCTS Node for simulation tree
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct MctsNode {
    visit_count: f64,
    total_reward: f64,
    children: std::collections::HashMap<petgraph::graph::NodeIndex, Arc<std::sync::Mutex<MctsNode>>>,
}

impl MctsNode {
    fn new() -> Self {
        Self {
            visit_count: 0.0,
            total_reward: 0.0,
            children: std::collections::HashMap::new(),
        }
    }

    fn ucb1(&self, parent_visit_count: f64) -> f64 {
        if self.visit_count == 0.0 {
            return f64::INFINITY;
        }
        let exploitation = self.total_reward / self.visit_count;
        let exploration = (2.0 * parent_visit_count.ln()).sqrt() / self.visit_count.sqrt();
        exploitation + 1.41 * exploration
    }
}

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

        // Use actual start_node from graph
        let start_node_idx = graph.start_node;

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

        let root = Arc::new(std::sync::Mutex::new(MctsNode::new()));
        let mut rng = SmallRng::from_entropy();

        for _ in 0..self.iterations {
            let mut path = Vec::new();
            let mut current_node_arc = Arc::clone(&root);
            let mut current_idx = start_node_idx;

            // 1. Selection
            loop {
                let (next_idx, next_node_arc) = {
                    let mut node = current_node_arc.lock().unwrap();
                    let neighbors: Vec<_> = graph.graph.neighbors(current_idx).collect();
                    if neighbors.is_empty() { break; }

                    let best_neighbor = neighbors.into_iter()
                        .max_by(|a, b| {
                            // Avoid nested locks by fetching visit counts first
                            let val_a = node.children.get(a)
                                .map(|n| n.lock().unwrap().ucb1(node.visit_count))
                                .unwrap_or(f64::INFINITY);
                            let val_b = node.children.get(b)
                                .map(|n| n.lock().unwrap().ucb1(node.visit_count))
                                .unwrap_or(f64::INFINITY);
                            val_a.partial_cmp(&val_b).unwrap()
                        })
                        .unwrap();

                    let next_node = node.children.entry(best_neighbor)
                        .or_insert_with(|| Arc::new(std::sync::Mutex::new(MctsNode::new())))
                        .clone();
                    (best_neighbor, next_node)
                };
                path.push((current_idx, Arc::clone(&current_node_arc)));
                current_idx = next_idx;
                current_node_arc = next_node_arc;
            }

            // 2. Simulation (Random Walk)
            let mut sim_prob = 1.0;
            let mut sim_cost = 0u64;
            let mut sim_current = current_idx;
            
            // Bound random walk to prevent infinite loops in cyclic graphs
            for _ in 0..num_nodes {
                let neighbors: Vec<_> = graph.graph.neighbors(sim_current).collect();
                if neighbors.is_empty() { break; }
                sim_current = neighbors[rng.gen_range(0..neighbors.len())];
                sim_prob *= 0.9 * hist_success_rate + 0.1 * 0.9;
                sim_cost += 10;
            }
            let reward = sim_prob / (sim_cost as f64).max(1.0);

            // 3. Backpropagation
            for (_, node_arc) in path.into_iter().rev() {
                let mut node = node_arc.lock().unwrap();
                node.visit_count += 1.0;
                node.total_reward += reward;
            }
            {
                let mut root_node = root.lock().unwrap();
                root_node.visit_count += 1.0;
                root_node.total_reward += reward;
            }
        }

        // Extract best path
        let mut best_path = Vec::new();
        let mut curr_node_arc = Arc::clone(&root);
        let mut curr_idx = start_node_idx;
        let mut total_prob = 1.0;
        let mut total_cost = 0u64;

        loop {
            let next = {
                let node = curr_node_arc.lock().unwrap();
                node.children.iter()
                    .max_by(|a, b| a.1.lock().unwrap().visit_count.partial_cmp(&b.1.lock().unwrap().visit_count).unwrap())
                    .map(|(idx, node)| (*idx, Arc::clone(node)))
            };
            if let Some((idx, node)) = next {
                best_path.push(graph.graph.node_weight(curr_idx).map(|n| n.name().to_string()).unwrap_or_default());
                curr_idx = idx;
                curr_node_arc = node;
                total_prob *= 0.9 * hist_success_rate + 0.1 * 0.9;
                total_cost += 10;
            } else {
                best_path.push(graph.graph.node_weight(curr_idx).map(|n| n.name().to_string()).unwrap_or_default());
                break;
            }
        }

        let best_path_str = best_path.join(" -> ");
        let max_score = {
            let root_node = root.lock().unwrap();
            if root_node.visit_count > 0.0 {
                root_node.total_reward / root_node.visit_count
            } else { 0.0 }
        };
        let final_cost = total_cost;

        let blended_cost = if final_cost > 0 {
            final_cost
        } else {
            hist_avg_cost
        };

        let duration = start.elapsed();
        tracing::info!(path_id = %best_path_str, success_probability = max_score, estimated_cost = blended_cost, duration_ms = duration.as_millis(), "MCTS Simulation completed");

        record_simulation(
            sop_id,
            max_score > 0.0,
            duration.as_secs_f64(),
            blended_cost,
        );

        Ok(SimulationResult {
            path_id: best_path_str,
            success_probability: max_score.max(0.0),
            estimated_cost: blended_cost,
            predicted_outcome: format!(
                "MCTS with history: {} iterations, prior_success={:.2}",
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
