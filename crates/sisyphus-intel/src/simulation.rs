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

    /// Calculate transition probabilities for a node based on its type and history.
    async fn get_transition_probabilities(
        &self,
        graph: &SOPGraph,
        node_idx: petgraph::graph::NodeIndex,
        sop_id: &str,
    ) -> std::collections::HashMap<petgraph::graph::NodeIndex, f64> {
        let mut probs = std::collections::HashMap::new();
        let node_weight = match graph.graph.node_weight(node_idx) {
            Some(w) => w,
            None => return probs,
        };

        if let Some(branch_node) = node_weight.as_any().downcast_ref::<sisyphus_types::BranchSOPNode>() {
            let mut total_weight = 0.0;
            let mut weights = std::collections::HashMap::new();

            if let Some(ref store) = self.state_store {
                if let Ok(Some(val)) = store.get_state_json(&format!("history:node:{}:{:?}", sop_id, node_idx)).await {
                    if let Ok(dist) = serde_json::from_value::<std::collections::HashMap<String, f64>>(val) {
                        for (output, prob) in dist {
                            if let Some(&next_idx_usize) = branch_node.branches.get(&output) {
                                let next_idx = petgraph::graph::NodeIndex::new(next_idx_usize);
                                weights.insert(next_idx, prob);
                                total_weight += prob;
                            }
                        }
                        let default_prob = (1.0 - total_weight).max(0.0);
                        weights.insert(petgraph::graph::NodeIndex::new(branch_node.default_next), default_prob);
                        total_weight += default_prob;
                    }
                }
            }

            if weights.is_empty() {
                let branch_count = branch_node.branches.len() + 1;
                let uniform_prob = 1.0 / branch_count as f64;
                for &next_idx_usize in branch_node.branches.values() {
                    weights.insert(petgraph::graph::NodeIndex::new(next_idx_usize), uniform_prob);
                }
                weights.insert(petgraph::graph::NodeIndex::new(branch_node.default_next), uniform_prob);
                total_weight = 1.0;
            }

            for (idx, weight) in weights {
                probs.insert(idx, weight / total_weight.max(1e-6));
            }
        } else {
            let neighbors: Vec<_> = graph.graph.neighbors(node_idx).collect();
            if !neighbors.is_empty() {
                let prob = 1.0 / neighbors.len() as f64;
                for n in neighbors {
                    probs.insert(n, prob);
                }
            }
        }
        probs
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
                    let probs = self.get_transition_probabilities(graph, current_idx, sop_id).await;
                    if probs.is_empty() { break; }

                    let mut node = current_node_arc.lock().unwrap();
                    let best_neighbor = probs.iter()
                        .max_by(|(a_idx, a_prob), (b_idx, b_prob)| {
                            let val_a = node.children.get(a_idx)
                                .map(|n| n.lock().unwrap().ucb1(node.visit_count))
                                .unwrap_or(f64::INFINITY) * *a_prob;
                            let val_b = node.children.get(b_idx)
                                .map(|n| n.lock().unwrap().ucb1(node.visit_count))
                                .unwrap_or(f64::INFINITY) * *b_prob;
                            val_a.partial_cmp(&val_b).unwrap()
                        })
                        .map(|(idx, _)| *idx)
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
            
            for _ in 0..num_nodes {
                let probs = self.get_transition_probabilities(graph, sim_current, sop_id).await;
                if probs.is_empty() { break; }
                
                // Weighted random sampling
                let mut r = rng.gen::<f64>();
                let mut next_node = None;
                for (idx, prob) in &probs {
                    if r < *prob {
                        next_node = Some(*idx);
                        break;
                    }
                    r -= prob;
                }
                sim_current = next_node.unwrap_or(sim_current);
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

    #[tokio::test]
    async fn test_monte_carlo_branching_graph() {
        struct DummyExecutor;
        #[async_trait::async_trait]
        impl sisyphus_types::ToolExecutor for DummyExecutor {
            async fn execute(&self, _: &str, _: Vec<u8>, _: u64) -> Result<Vec<u8>, sisyphus_types::TypesError> {
                Ok(vec![])
            }
        }

        let config_val = serde_json::json!({
            "start_node": 0,
            "nodes": [
                { "name": "Start", "kind": { "Wasm": { "tool_id": "t1", "fuel_limit": 1000 } } },
                { "name": "Branch", "kind": { "Branch": { "branches": { "ok": 2, "err": 3 }, "default_next": 3 } } },
                { "name": "Success", "kind": { "Wasm": { "tool_id": "t2", "fuel_limit": 1000 } } },
                { "name": "Failure", "kind": { "Wasm": { "tool_id": "t3", "fuel_limit": 1000 } } }
            ],
            "edges": [ [0, 1], [1, 2], [1, 3] ]
        });
        let config: sisyphus_types::SOPGraphConfig = serde_json::from_value(config_val).unwrap();
        let graph = SOPGraph::from_config(&config, Arc::new(sisyphus_types::ExecutorRouter::new(Arc::new(DummyExecutor)))).unwrap();
        
        let sim = MonteCarloSimulator::new(100);
        let ctx = ExecutionContext::new();
        let result = sim.simulate(&graph, &ctx, "branch-test").await.unwrap();
        assert!(result.success_probability >= 0.0 && result.success_probability <= 1.0);
    }
}
