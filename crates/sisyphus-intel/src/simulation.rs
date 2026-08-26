use async_trait::async_trait;
use petgraph::visit::EdgeRef;
use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use sisyphus_metrics::record_simulation;
use sisyphus_types::{ExecutionContext, SOPGraph};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

// ---------------------------------------------------------------------------
// 仿真类型
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogicalFlaw {
    pub node_name: String,
    pub description: String,
    pub severity: FlawSeverity,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum FlawSeverity {
    Critical, // Must fix before execution
    Warning,  // Potential issue, but may succeed
    Info,     // Suggestion for improvement
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogicalSimulationResult {
    pub is_valid: bool,
    pub flaws: Vec<LogicalFlaw>,
    pub predicted_path: Vec<String>,
    pub confidence: f64,
}

#[async_trait]
pub trait LogicalSimulator: Send + Sync {
    async fn simulate_logic(
        &self,
        graph: Arc<SOPGraph>,
        context: &ExecutionContext,
        sop_id: &str,
        goal_text: Option<&str>,
    ) -> Result<LogicalSimulationResult, sisyphus_types::IntelError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationResult {
    pub path_id: String,
    pub success_probability: f64,
    pub estimated_cost: u64,
    pub predicted_stability: f64,
    pub predicted_effort: u64,
    pub predicted_outcome: String,
}

#[async_trait]
pub trait Simulator: Send + Sync {
    async fn simulate(
        &self,
        graph: Arc<SOPGraph>,
        context: &ExecutionContext,
        project_id: &str,
        sop_id: &str,
        goal_text: Option<&str>,
    ) -> Result<SimulationResult, sisyphus_types::IntelError>;

    async fn set_experience_bank(
        &self,
        _bank: Arc<dyn sisyphus_types::experience::ExperienceBank>,
    ) -> Result<(), String> {
        Err("Experience bank not supported by this simulator".into())
    }
}

// ---------------------------------------------------------------------------
// MonteCarloSimulator — 经验加权随机游走仿真
// ---------------------------------------------------------------------------
//
// 结合蒙特卡洛采样与历史经验：
// - state_store 可用时，查询过去执行记录以偏置
//   边转移概率与成本估算。
// - 无历史时，回退到均匀随机游走。
// - 使用 Rayon 并行迭代。

#[derive(Clone)]
pub struct LlmLogicalSimulator {
    pub llm_client: Arc<dyn sisyphus_tools_llm::LlmClient>,
    /// 逻辑审计阶段 LLM 调用的超时（评审 B3：原硬编码 2s）。
    pub timeout: std::time::Duration,
    pub confidence_threshold: f64,
}

impl LlmLogicalSimulator {
    /// 测试友好构造：使用默认的 2 秒超时。
    pub fn new(llm_client: Arc<dyn sisyphus_tools_llm::LlmClient>) -> Self {
        Self::with_timeout(llm_client, std::time::Duration::from_secs(2))
    }

    /// 生产构造：显式传入统一配置的阶段超时。
    pub fn with_timeout(
        llm_client: Arc<dyn sisyphus_tools_llm::LlmClient>,
        timeout: std::time::Duration,
    ) -> Self {
        Self {
            llm_client,
            timeout,
            confidence_threshold: 0.7,
        }
    }
}

#[async_trait]
impl LogicalSimulator for LlmLogicalSimulator {
    async fn simulate_logic(
        &self,
        graph: Arc<SOPGraph>,
        _context: &ExecutionContext,
        sop_id: &str,
        goal_text: Option<&str>,
    ) -> Result<LogicalSimulationResult, sisyphus_types::IntelError> {
        let mut virtual_context: HashMap<String, String> = HashMap::new();
        let mut flaws = Vec::new();
        let mut predicted_path = Vec::new();
        // LLM 一旦失败（超时/错误），我们停止调用并回退到
        // 对剩余节点的启发式预测，使不可达的 LLM
        // 永远不会拖慢认知循环。
        let mut llm_available = true;

        // 按拓扑顺序处理节点
        let node_indices: Vec<_> = graph.graph.node_indices().collect();
        // 注意：真实实现中应使用正确的拓扑排序。
        // 目前假定索引大致有序，或使用 petgraph 的 topo_sort。

        for idx in node_indices {
            let node_weight = graph
                .graph
                .node_weight(idx)
                .ok_or_else(|| "Node weight missing".to_string())?;
            let node_name = node_weight.name().to_string();

            // 从前驱节点收集输入
            let mut inputs = HashMap::new();
            for edge in graph
                .graph
                .edges_directed(idx, petgraph::Direction::Incoming)
            {
                let source_idx = edge.source();
                if let Some(source_weight) = graph.graph.node_weight(source_idx) {
                    let source_name = source_weight.name();
                    if let Some(out) = virtual_context.get(source_name) {
                        inputs.insert(source_name.to_string(), out.clone());
                    }
                }
            }

            // 提示 LLM 预测输出并检查缺陷
            let prompt = format!(
                "Goal: {}\nSOP ID: {}\nNode: {}\nInputs: {:?}\n\nPredict the output of this node and check for any logical flaws (e.g. missing required input, type mismatch). \
                Respond in JSON format: {{\"output\": \"...\", \"flaw\": \"...\", \"severity\": \"Critical|Warning|Info\", \"confidence\": 0.0-1.0}}",
                goal_text.unwrap_or(sop_id),
                sop_id,
                node_name,
                inputs
            );

            let response = if llm_available {
                match tokio::time::timeout(
                    self.timeout,
                    self.llm_client.chat(
                        "You are a logical auditor for a cognitive engine. Analyze the provided SOP node and predict its output while checking for logical flaws.",
                        &prompt,
                        true,
                    ),
                )
                .await
                {
                    Ok(Ok(resp)) => Some(resp),
                    Ok(Err(e)) => {
                        tracing::warn!(node = %node_name, "LLM logical audit failed, degrading to heuristic: {e}");
                        llm_available = false;
                        None
                    }
                    Err(_) => {
                        tracing::warn!(
                            node = %node_name,
                            timeout_ms = self.timeout.as_millis(),
                            "LLM logical audit timed out, degrading to heuristic"
                        );
                        llm_available = false;
                        None
                    }
                }
            } else {
                None
            };

            // LLM 不可用时的启发式回退：假定节点成功，
            // 并记录一个信息性缺陷，使降级可在
            // 仿真结果中观测到。
            let response = match response {
                Some(resp) => resp,
                None => {
                    flaws.push(LogicalFlaw {
                        node_name: node_name.clone(),
                        description: "LLM audit unavailable; heuristic prediction used".into(),
                        severity: FlawSeverity::Info,
                    });
                    virtual_context.insert(node_name.clone(), "ok".to_string());
                    predicted_path.push(node_name.clone());
                    continue;
                }
            };

            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&response) {
                let output = val
                    .get("output")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let flaw_desc = val.get("flaw").and_then(|v| v.as_str());
                let severity_str = val
                    .get("severity")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Info");
                let confidence = val
                    .get("confidence")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.5);

                if let Some(desc) = flaw_desc {
                    let severity = match severity_str {
                        "Critical" => FlawSeverity::Critical,
                        "Warning" => FlawSeverity::Warning,
                        _ => FlawSeverity::Info,
                    };
                    flaws.push(LogicalFlaw {
                        node_name: node_name.clone(),
                        description: desc.to_string(),
                        severity,
                    });

                    if severity == FlawSeverity::Critical {
                        return Ok(LogicalSimulationResult {
                            is_valid: false,
                            flaws,
                            predicted_path,
                            confidence,
                        });
                    }
                }

                virtual_context.insert(node_name.clone(), output);
                predicted_path.push(node_name);
            }
        }

        let is_valid = !flaws
            .iter()
            .any(|f| matches!(f.severity, FlawSeverity::Critical));

        Ok(LogicalSimulationResult {
            is_valid,
            flaws,
            predicted_path,
            confidence: 0.8, // Simplified
        })
    }
}

#[derive(Debug)]
struct MctsNode {
    visit_count: f64,
    total_reward: f64,
    total_cost: f64,
    children:
        std::collections::HashMap<petgraph::graph::NodeIndex, Arc<std::sync::Mutex<MctsNode>>>,
}

impl MctsNode {
    fn new() -> Self {
        Self {
            visit_count: 0.0,
            total_reward: 0.0,
            total_cost: 0.0,
            children: std::collections::HashMap::new(),
        }
    }

    fn ucb1(&self, parent_visit_count: f64) -> f64 {
        if self.visit_count == 0.0 {
            return f64::INFINITY;
        }
        let success_rate = self.total_reward / self.visit_count;
        let avg_cost = self.total_cost / self.visit_count;
        // 偏好成功率更高、平均成本更低的路径
        let exploitation = success_rate / (avg_cost.max(1.0) / 1000.0).max(0.1);
        let exploration = (2.0 * parent_visit_count.ln()).sqrt() / self.visit_count.sqrt();
        exploitation + 1.41 * exploration
    }
}

#[derive(Clone)]
pub struct MonteCarloSimulator {
    pub iterations: usize,
    pub state_store: Option<Arc<dyn sisyphus_types::StateStore>>,
    pub experience_bank: Option<Arc<dyn sisyphus_types::experience::ExperienceBank>>,
}

impl MonteCarloSimulator {
    pub fn new(iterations: usize) -> Self {
        Self {
            iterations,
            state_store: None,
            experience_bank: None,
        }
    }

    pub fn with_state_store(iterations: usize, store: Arc<dyn sisyphus_types::StateStore>) -> Self {
        Self {
            iterations,
            state_store: Some(store),
            experience_bank: None,
        }
    }

    pub fn with_experience_bank(
        iterations: usize,
        bank: Arc<dyn sisyphus_types::experience::ExperienceBank>,
    ) -> Self {
        Self {
            iterations,
            state_store: None,
            experience_bank: Some(bank),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn simulate_one_path(
        &self,
        graph: Arc<SOPGraph>,
        root: Arc<std::sync::Mutex<MctsNode>>,
        project_id: &str,
        sop_id: Arc<String>,
        goal_text: Option<String>,
        num_nodes: usize,
        hist_success_rate: f64,
        rng: &mut SmallRng,
    ) -> Result<
        (
            Vec<(petgraph::graph::NodeIndex, Arc<std::sync::Mutex<MctsNode>>)>,
            f64,
            f64,
        ),
        String,
    > {
        let mut path = Vec::new();
        let mut current_node_arc = Arc::clone(&root);
        let mut current_idx = graph.start_node;

        // 1. 选择
        loop {
            let (next_idx, next_node_arc) = {
                let probs = self
                    .get_transition_probabilities(
                        &graph,
                        current_idx,
                        project_id,
                        &sop_id,
                        goal_text.as_deref(),
                    )
                    .await;
                if probs.is_empty() {
                    break;
                }

                let mut node = current_node_arc
                    .lock()
                    .map_err(|_| "Lock poisoned".to_string())?;
                let best_neighbor = probs
                    .iter()
                    .max_by(|(a_idx, _a_prob), (b_idx, _b_prob)| {
                        let val_a = node
                            .children
                            .get(a_idx)
                            .map(|n| {
                                n.lock()
                                    .map_err(|_| "Lock poisoned".to_string())
                                    .map(|ln| ln.ucb1(node.visit_count))
                            })
                            .unwrap_or(Ok(f64::INFINITY));
                        let val_b = node
                            .children
                            .get(b_idx)
                            .map(|n| {
                                n.lock()
                                    .map_err(|_| "Lock poisoned".to_string())
                                    .map(|ln| ln.ucb1(node.visit_count))
                            })
                            .unwrap_or(Ok(f64::INFINITY));

                        match (val_a, val_b) {
                            (Ok(a), Ok(b)) => {
                                a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
                            }
                            (Ok(_), Err(_)) => std::cmp::Ordering::Greater,
                            (Err(_), Ok(_)) => std::cmp::Ordering::Less,
                            (Err(_), Err(_)) => std::cmp::Ordering::Equal,
                        }
                    })
                    .map(|(idx, _)| *idx)
                    .ok_or_else(|| "No best neighbor found".to_string())?;

                let next_node = node
                    .children
                    .entry(best_neighbor)
                    .or_insert_with(|| Arc::new(std::sync::Mutex::new(MctsNode::new())))
                    .clone();
                (best_neighbor, next_node)
            };
            path.push((current_idx, Arc::clone(&current_node_arc)));
            current_idx = next_idx;
            current_node_arc = next_node_arc;
        }

        // 2. 仿真（随机游走）
        let mut sim_prob = 1.0;
        let mut sim_cost = 0u64;
        let mut sim_current = current_idx;

        for _ in 0..num_nodes {
            let probs = self
                .get_transition_probabilities(
                    &graph,
                    sim_current,
                    project_id,
                    &sop_id,
                    goal_text.as_deref(),
                )
                .await;
            if probs.is_empty() {
                break;
            }

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

            let node_weight = graph
                .graph
                .node_weight(sim_current)
                .ok_or("Node weight not found")?;
            let cost = if let Some(wasm) = node_weight
                .as_any()
                .downcast_ref::<sisyphus_types::WasmSOPNode>()
            {
                wasm.fuel_limit
            } else {
                10
            };

            sim_cost += cost;
            sim_prob *= 0.9 * hist_success_rate + 0.1 * 0.9;
        }
        let reward = sim_prob / (sim_cost as f64).max(1.0);
        Ok((path, reward, sim_cost as f64))
    }

    /// 基于节点类型与历史计算转移概率。
    async fn get_transition_probabilities(
        &self,
        graph: &SOPGraph,
        node_idx: petgraph::graph::NodeIndex,
        project_id: &str,
        sop_id: &str,
        goal_text: Option<&str>,
    ) -> std::collections::HashMap<petgraph::graph::NodeIndex, f64> {
        let mut probs = std::collections::HashMap::new();
        let node_weight = match graph.graph.node_weight(node_idx) {
            Some(w) => w,
            None => return probs,
        };

        if let Some(branch_node) = node_weight
            .as_any()
            .downcast_ref::<sisyphus_types::BranchSOPNode>()
        {
            let mut total_weight = 0.0;
            let mut weights = std::collections::HashMap::new();

            if let Some(ref store) = self.state_store {
                if let Ok(Some(val)) = store
                    .get_state_json(
                        project_id,
                        &format!("history:node:{}:{:?}", sop_id, node_idx),
                    )
                    .await
                {
                    if let Ok(dist) =
                        serde_json::from_value::<std::collections::HashMap<String, f64>>(val)
                    {
                        for (output, prob) in dist {
                            if let Some(&next_idx_usize) = branch_node.branches.get(&output) {
                                let next_idx = petgraph::graph::NodeIndex::new(next_idx_usize);
                                weights.insert(next_idx, prob);
                                total_weight += prob;
                            }
                        }
                        let default_prob = (1.0 - total_weight).max(0.0);
                        weights.insert(
                            petgraph::graph::NodeIndex::new(branch_node.default_next),
                            default_prob,
                        );
                        total_weight += default_prob;

                        // 基于经验库中的历史经验偏置权重
                        if let Some(ref bank) = self.experience_bank {
                            let query_text = goal_text.unwrap_or(sop_id);
                            let context_hash = format!("{:x}", md5::compute(query_text));
                            let query = sisyphus_types::experience::ExperienceQuery {
                                query_text: query_text.to_string(),
                                context_hash,
                                project_id: None,
                                action_id: Some(format!("{:?}", node_idx)),
                                visual_embedding: None,
                                limit: 10,
                            };
                            if let Ok(experiences) = bank.query_similar(query).await {
                                let total_exp = experiences.len() as f64;
                                let successes =
                                    experiences.iter().filter(|e| e.outcome_success).count() as f64;
                                let rate = successes / total_exp.max(1.0);

                                for weight in weights.values_mut() {
                                    *weight *= 0.5 + rate;
                                }
                            }
                        }
                    }
                }
            }

            if weights.is_empty() {
                let branch_count = branch_node.branches.len() + 1;
                let uniform_prob = 1.0 / branch_count as f64;
                for &next_idx_usize in branch_node.branches.values() {
                    weights.insert(
                        petgraph::graph::NodeIndex::new(next_idx_usize),
                        uniform_prob,
                    );
                }
                weights.insert(
                    petgraph::graph::NodeIndex::new(branch_node.default_next),
                    uniform_prob,
                );
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
        graph: Arc<SOPGraph>,
        _context: &ExecutionContext,
        project_id: &str,
        sop_id: &str,
        goal_text: Option<&str>,
    ) -> Result<SimulationResult, sisyphus_types::IntelError> {
        let project_id_owned = project_id.to_string();
        let span = tracing::info_span!("intel.simulate", sop_id = %sop_id, iterations = self.iterations, graph_nodes = graph.graph.node_count());
        let _enter = span.enter();

        let start = Instant::now();
        let num_nodes = graph.graph.node_count();
        if num_nodes == 0 {
            return Ok(SimulationResult {
                path_id: "empty".into(),
                success_probability: 0.0,
                estimated_cost: 0,
                predicted_stability: 1.0,
                predicted_effort: 0,
                predicted_outcome: "Empty graph".into(),
            });
        }

        let start_node_idx = graph.start_node;
        let graph_arc = Arc::clone(&graph);
        let sop_id_arc = Arc::new(sop_id.to_string());

        // --- 经验先验 ---
        let (hist_success_rate, _hist_avg_cost) = if let Some(ref store) = self.state_store {
            let history_key = format!("history:{}", sop_id);
            match store.get_state_json(project_id, &history_key).await {
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

        let root = Arc::new(std::sync::Mutex::new(MctsNode::new()));

        // --- 并行批量仿真 ---
        let batch_size = 100;
        let num_batches = self.iterations.div_ceil(batch_size);
        let mut batch_handles = Vec::new();

        for b in 0..num_batches {
            let root_clone = Arc::clone(&root);
            let graph_clone = Arc::clone(&graph_arc);
            let sop_id_clone = Arc::clone(&sop_id_arc);
            let goal_text_clone = goal_text.map(|s| s.to_string());
            let iterations_in_batch = if b == num_batches - 1 {
                self.iterations % batch_size.max(1)
            } else {
                batch_size
            };
            if iterations_in_batch == 0 && self.iterations > 0 {
                continue;
            }
            let iterations_in_batch = if iterations_in_batch == 0 {
                batch_size
            } else {
                iterations_in_batch
            };

            let simulator_ref = self.clone();

            let p_id = project_id_owned.clone();
            batch_handles.push(tokio::spawn(async move {
                let mut local_rng = SmallRng::from_entropy();
                let mut batch_results = Vec::new();

                for _ in 0..iterations_in_batch {
                    if let Ok(res) = simulator_ref
                        .simulate_one_path(
                            Arc::clone(&graph_clone),
                            Arc::clone(&root_clone),
                            &p_id,
                            Arc::clone(&sop_id_clone),
                            goal_text_clone.clone(),
                            num_nodes,
                            hist_success_rate,
                            &mut local_rng,
                        )
                        .await
                    {
                        batch_results.push(res);
                    }
                }
                batch_results
            }));
        }

        for handle in batch_handles {
            if let Ok(batch_results) = handle.await {
                for (path, reward, cost) in batch_results {
                    for (_, node_arc) in path.into_iter().rev() {
                        let mut node = node_arc.lock().unwrap_or_else(|e| e.into_inner());
                        node.visit_count += 1.0;
                        node.total_reward += reward;
                        node.total_cost += cost;
                    }
                    let mut root_node = root.lock().unwrap_or_else(|e| e.into_inner());
                    root_node.visit_count += 1.0;
                    root_node.total_reward += reward;
                    root_node.total_cost += cost;
                }
            }
        }

        // 提取最优路径（贪心）
        let mut best_path = Vec::new();
        let mut curr_node_arc = Arc::clone(&root);
        let mut curr_idx = start_node_idx;
        let mut total_cost = 0u64;

        loop {
            let next = {
                let node = curr_node_arc.lock().unwrap_or_else(|e| e.into_inner());
                node.children
                    .iter()
                    .max_by(|a, b| {
                        a.1.lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .visit_count
                            .partial_cmp(&b.1.lock().unwrap_or_else(|e| e.into_inner()).visit_count)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .map(|(idx, node)| (*idx, Arc::clone(node)))
            };
            if let Some((idx, node)) = next {
                best_path.push(
                    graph
                        .graph
                        .node_weight(curr_idx)
                        .map(|n| n.name().to_string())
                        .unwrap_or_default(),
                );
                curr_idx = idx;
                curr_node_arc = node;
                let node_weight = graph.graph.node_weight(curr_idx);
                total_cost += if let Some(weight) = node_weight {
                    if let Some(wasm) = weight
                        .as_any()
                        .downcast_ref::<sisyphus_types::WasmSOPNode>()
                    {
                        wasm.fuel_limit
                    } else {
                        10
                    }
                } else {
                    10
                };
            } else {
                best_path.push(
                    graph
                        .graph
                        .node_weight(curr_idx)
                        .map(|n| n.name().to_string())
                        .unwrap_or_default(),
                );
                break;
            }
        }

        let best_path_str = best_path.join(" -> ");
        let max_score = {
            let root_node = root.lock().unwrap_or_else(|e| e.into_inner());
            if root_node.visit_count > 0.0 {
                root_node.total_reward / root_node.visit_count
            } else {
                0.0
            }
        };

        let duration = start.elapsed();
        tracing::info!(path_id = %best_path_str, success_probability = max_score, estimated_cost = total_cost, duration_ms = duration.as_millis(), "Parallel MCTS Simulation completed");

        record_simulation(sop_id, max_score > 0.0, duration.as_secs_f64(), total_cost);

        Ok(SimulationResult {
            path_id: best_path_str,
            success_probability: max_score.max(0.0),
            estimated_cost: total_cost,
            predicted_stability: 1.0 - (max_score * 0.1), // Simple heuristic: higher success usually means higher stability
            predicted_effort: total_cost / 100,           // Simplified effort metric
            predicted_outcome: format!(
                "Parallel MCTS: {} iterations, batches={}",
                self.iterations, num_batches
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
        let graph = Arc::new(SOPGraph::new());
        let sim = MonteCarloSimulator::new(10);
        let ctx = ExecutionContext::new();
        let result = sim
            .simulate(graph, &ctx, "test-project", "empty", None)
            .await
            .unwrap();
        assert_eq!(result.success_probability, 0.0);
        assert_eq!(result.estimated_cost, 0);
    }

    #[tokio::test]
    async fn test_monte_carlo_branching_graph() {
        struct DummyExecutor;
        #[async_trait::async_trait]
        impl sisyphus_types::ToolExecutor for DummyExecutor {
            async fn execute(
                &self,
                _: &str,
                _: Vec<u8>,
                _: u64,
            ) -> Result<Vec<u8>, sisyphus_types::TypesError> {
                Ok(vec![])
            }
        }
        #[async_trait::async_trait]
        impl sisyphus_types::TestExecutor for DummyExecutor {
            async fn run_test(
                &self,
                _: &str,
                _: Option<&str>,
            ) -> Result<sisyphus_types::TestResult, sisyphus_types::TypesError> {
                Ok(sisyphus_types::TestResult {
                    success: true,
                    stdout: "".to_string(),
                    stderr: "".to_string(),
                    exit_code: 0,
                    duration_ms: 0,
                    error_summary: None,
                })
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
        let dummy_exec = Arc::new(DummyExecutor);
        let graph = SOPGraph::from_config(
            &config,
            Arc::new(sisyphus_types::ExecutorRouter::new(dummy_exec.clone())),
            dummy_exec,
        )
        .unwrap();

        let sim = MonteCarloSimulator::new(100);
        let ctx = ExecutionContext::new();
        let result = sim
            .simulate(graph.clone(), &ctx, "test-project", "branch-test", None)
            .await
            .unwrap();
        assert!(result.success_probability >= 0.0 && result.success_probability <= 1.0);
    }
}
