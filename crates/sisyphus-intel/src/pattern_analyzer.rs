use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use sisyphus_types::{SOPGraph, SOPNode};
use petgraph::visit::EdgeRef;
use petgraph::graph::NodeIndex;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SubgraphPattern {
    pub nodes: Vec<(String, String)>, // (node_name, tool_id)
    pub edges: Vec<(usize, usize)>,   // Indices into the nodes vector
}

pub struct PatternAnalyzer {
    // Maps a pattern to its frequency across different graphs
    pub pattern_frequency: HashMap<SubgraphPattern, usize>,
}

impl PatternAnalyzer {
    pub fn new() -> Self {
        Self {
            pattern_frequency: HashMap::new(),
        }
    }

    /// Analyzes a set of successful graphs to find recurring patterns.
    pub fn analyze_graphs(&mut self, graphs: &[Arc<SOPGraph>]) {
        for graph in graphs {
            let patterns = self.extract_patterns(graph);
            for pattern in patterns {
                *self.pattern_frequency.entry(pattern).or_insert(0) += 1;
            }
        }
    }

    /// Extracts all subgraphs of size 2 to 4.
    fn extract_patterns(&self, graph: &SOPGraph) -> HashSet<SubgraphPattern> {
        let mut patterns = HashSet::new();
        let nodes = &graph.graph;

        for start_node in nodes.node_indices() {
            // Explore subgraphs starting from this node using simple BFS/DFS
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

    fn canonicalize_subgraph(&self, graph: &SOPGraph, nodes: &[NodeIndex]) -> Option<SubgraphPattern> {
        let mut pattern_nodes = Vec::new();
        let mut node_to_idx = HashMap::new();

        for (i, &node_idx) in nodes.iter().enumerate() {
            let node = graph.graph[node_idx].as_ref();
            // We care about the tool_id for the pattern, name is secondary but kept for clarity
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
                    let u_idx = *node_to_idx.get(&u).unwrap();
                    pattern_edges.push((u_idx, v_idx));
                }
            }
        }

        // Sort edges to make the pattern canonical
        pattern_edges.sort();

        Some(SubgraphPattern {
            nodes: pattern_nodes,
            edges: pattern_edges,
        })
    }

    /// Returns patterns that appear more than `min_frequency` times.
    pub fn get_frequent_patterns(&self, min_frequency: usize) -> Vec<SubgraphPattern> {
        let mut frequent: Vec<_> = self.pattern_frequency
            .iter()
            .filter(|(_, &count)| count >= min_frequency)
            .map(|(pattern, _)| pattern.clone())
            .collect();
        
        // Sort by frequency descending (would need to store frequency in result)
        frequent
    }
}

#[derive(Debug, Clone)]
pub struct ClusterAnalysis {
    pub goal_cluster: String,
    pub common_failure_mode: String,
    pub evidence_count: usize,
    pub suggested_shift: String,
}

pub struct SemanticPatternAnalyzer {
    pub min_cluster_size: usize,
}

impl SemanticPatternAnalyzer {
    pub fn new(min_cluster_size: usize) -> Self {
        Self { min_cluster_size }
    }

    /// Analyzes the experience bank for clusters of failures with similar semantic goals
    /// and common root causes (extracted from insights).
    pub async fn analyze_failure_clusters(
        &self,
        bank: &dyn sisyphus_types::experience::ExperienceBank,
        current_goal: &str,
    ) -> Result<Option<ClusterAnalysis>, String> {
        // 1. Retrieve a broad set of experiences related to the current goal
        let query = sisyphus_types::experience::ExperienceQuery {
            query_text: current_goal.to_string(),
            context_hash: "".to_string(), // Broad search
            project_id: None,
            action_id: None,
            visual_embedding: None,
            limit: 50,
        };
        
        let experiences = bank.query_similar(query).await?;
        let failures: Vec<_> = experiences.into_iter().filter(|e| !e.outcome_success).collect();
        
        if failures.len() < self.min_cluster_size {
            return Ok(None);
        }

        // 2. Group failures by common insight themes (simple keyword matching or LLM)
        let mut theme_counts: HashMap<String, Vec<String>> = HashMap::new();
        for fail in &failures {
            if let Some(ref insight) = fail.insight {
                let theme = insight.split('.').next().unwrap_or("Unknown").to_string();
                theme_counts.entry(theme).or_default().push(fail.context_text.clone());
            }
        }

        // 3. Find the most dominant failure mode
        let dominant_theme = theme_counts.iter()
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
