use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use sisyphus_types::SOPGraph;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
/// Subgraph Pattern.
pub struct SubgraphPattern {
    pub nodes: Vec<(String, String)>, // (node_name, tool_id)
    pub edges: Vec<(usize, usize)>,   // Indices into the nodes vector
}

/// Pattern Analyzer.
pub struct PatternAnalyzer {
    // 将模式映射到其在不同图谱中的出现频率
    pub pattern_frequency: HashMap<SubgraphPattern, usize>,
}

impl Default for PatternAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl PatternAnalyzer {
    /// New.
    pub fn new() -> Self {
        Self {
            pattern_frequency: HashMap::new(),
        }
    }

    /// 分析一组成功图谱以发现重复模式。
    pub fn analyze_graphs(&mut self, graphs: &[Arc<SOPGraph>]) {
        for graph in graphs {
            let patterns = self.extract_patterns(graph);
            for pattern in patterns {
                *self.pattern_frequency.entry(pattern).or_insert(0) += 1;
            }
        }
    }

    /// 提取大小为 2 到 4 的所有子图。
    fn extract_patterns(&self, graph: &SOPGraph) -> HashSet<SubgraphPattern> {
        let mut patterns = HashSet::new();
        let nodes = &graph.graph;

        for start_node in nodes.node_indices() {
            // 使用简单 BFS/DFS 从该节点开始探索子图
            self.explore_subgraphs(graph, start_node, 4, &mut patterns);
        }

        patterns
    }

    fn explore_subgraphs(
        &self,
        graph: &SOPGraph,
        current_node: NodeIndex,
        max_size: usize,
        patterns: &mut HashSet<SubgraphPattern>,
    ) {
        let mut visited = Vec::new();
        self.recursive_explore(graph, current_node, max_size, &mut visited, patterns);
    }

    fn recursive_explore(
        &self,
        graph: &SOPGraph,
        current_node: NodeIndex,
        max_size: usize,
        visited: &mut Vec<NodeIndex>,
        patterns: &mut HashSet<SubgraphPattern>,
    ) {
        visited.push(current_node);

        if visited.len() >= 2 {
            if let Some(pattern) = self.canonicalize_subgraph(graph, visited) {
                patterns.insert(pattern);
            }
        }

        if visited.len() < max_size {
            for edge in graph.graph.edges(current_node) {
                let next_node = edge.target();
                if !visited.contains(&next_node) {
                    self.recursive_explore(graph, next_node, max_size, visited, patterns);
                }
            }
        }

        visited.pop();
    }

    fn canonicalize_subgraph(
        &self,
        graph: &SOPGraph,
        nodes: &[NodeIndex],
    ) -> Option<SubgraphPattern> {
        let mut pattern_nodes = Vec::new();
        let mut node_to_idx = HashMap::new();

        for (i, &node_idx) in nodes.iter().enumerate() {
            let node = graph.graph[node_idx].as_ref();
            // 我们关注模式中的 tool_id，名称次要但为清晰起见保留
            let tool_id = match node.to_kind() {
                sisyphus_types::NodeKind::Wasm { ref tool_id, .. } => tool_id.clone(),
                _ => "unknown".to_string(),
            };
            let name = node.name().to_string();
            pattern_nodes.push((name, tool_id));
            node_to_idx.insert(node_idx, i);
        }

        let mut pattern_edges = Vec::new();
        for &u in nodes {
            for edge in graph.graph.edges(u) {
                let v = edge.target();
                if let Some(&v_idx) = node_to_idx.get(&v) {
                    if let Some(&u_idx) = node_to_idx.get(&u) {
                        pattern_edges.push((u_idx, v_idx));
                    }
                }
            }
        }

        // 对边排序使模式规范化
        pattern_edges.sort();

        Some(SubgraphPattern {
            nodes: pattern_nodes,
            edges: pattern_edges,
        })
    }

    /// 返回出现超过 `min_frequency` 次的模式。
    pub fn get_frequent_patterns(&self, min_frequency: usize) -> Vec<SubgraphPattern> {
        let frequent: Vec<_> = self
            .pattern_frequency
            .iter()
            .filter(|(_, &count)| count >= min_frequency)
            .map(|(pattern, _)| pattern.clone())
            .collect();

        // 按频率降序排序（需在结果中存储频率）
        frequent
    }
}

#[derive(Debug, Clone)]
/// Cluster Analysis.
pub struct ClusterAnalysis {
    pub goal_cluster: String,
    pub common_failure_mode: String,
    pub evidence_count: usize,
    pub suggested_shift: String,
}

/// Semantic Pattern Analyzer.
pub struct SemanticPatternAnalyzer {
    pub min_cluster_size: usize,
}

impl SemanticPatternAnalyzer {
    /// New.
    pub fn new(min_cluster_size: usize) -> Self {
        Self { min_cluster_size }
    }

    /// 分析经验库中具有相似语义目标的失败簇，
    /// 以及共同根因（从洞察中提取）。
    pub async fn analyze_failure_clusters(
        &self,
        bank: &dyn sisyphus_types::experience::ExperienceBank,
        current_goal: &str,
    ) -> Result<Option<ClusterAnalysis>, String> {
        // 1. 检索与当前目标相关的大量经验
        let query = sisyphus_types::experience::ExperienceQuery {
            query_text: current_goal.to_string(),
            context_hash: "".to_string(), // Broad search
            project_id: None,
            action_id: None,
            visual_embedding: None,
            limit: 50,
        };

        let experiences = bank.query_similar(query).await.map_err(|e| e.to_string())?;
        let failures: Vec<_> = experiences
            .into_iter()
            .filter(|e| !e.outcome_success)
            .collect();

        if failures.len() < self.min_cluster_size {
            return Ok(None);
        }

        // 2. 按共同洞察主题对失败分组（简单关键词匹配或 LLM）
        let mut theme_counts: HashMap<String, Vec<String>> = HashMap::new();
        for fail in &failures {
            if let Some(ref insight) = fail.insight {
                let theme = insight.split('.').next().unwrap_or("Unknown").to_string();
                theme_counts
                    .entry(theme)
                    .or_default()
                    .push(fail.context_text.clone());
            }
        }

        // 3. 找到最主要的失败模式
        let dominant_theme = theme_counts
            .iter()
            .max_by_key(|(_, goals)| goals.len())
            .map(|(theme, goals)| (theme.clone(), goals.len(), goals.clone()));

        if let Some((theme, count, goals)) = dominant_theme {
            if count >= self.min_cluster_size {
                return Ok(Some(ClusterAnalysis {
                    goal_cluster: goals.join(", "),
                    common_failure_mode: theme.clone(),
                    evidence_count: count,
                    suggested_shift: format!("The system consistently fails at '{}'. Consider a strategic shift in tool selection or SOP architecture.", theme),
                }));
            }
        }

        Ok(None)
    }
}
