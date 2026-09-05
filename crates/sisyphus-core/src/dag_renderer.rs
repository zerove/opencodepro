//! 用于 TUI 展示的视觉化 DAG 渲染。
//!
//! 将 SOP 图谱执行状态渲染为 ASCII 艺术图。

use std::collections::HashMap;

/// 用于视觉渲染的节点执行状态。
#[derive(Debug, Clone, PartialEq)]
pub enum NodeStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
}

/// DAG 可视化中的一个已渲染节点。
#[derive(Debug, Clone)]
pub struct RenderedNode {
    pub id: String,
    pub label: String,
    pub status: NodeStatus,
    pub row: usize,
    pub col: usize,
}

/// DAG 可视化中的一条已渲染边。
#[derive(Debug, Clone)]
pub struct RenderedEdge {
    pub from: String,
    pub to: String,
}

/// 完整的 DAG 可视化。
#[derive(Debug, Clone)]
pub struct DagVisualization {
    pub nodes: Vec<RenderedNode>,
    pub edges: Vec<RenderedEdge>,
    pub width: usize,
    pub height: usize,
}

/// ASCII 艺术 DAG 渲染器。
pub struct DagRenderer {
    node_width: usize,
    node_height: usize,
    h_gap: usize,
    v_gap: usize,
}

impl DagRenderer {
    /// New.
    pub fn new() -> Self {
        Self {
            node_width: 20,
            node_height: 3,
            h_gap: 3,
            v_gap: 2,
        }
    }

    /// 从邻接表与节点状态渲染 DAG。
    pub fn render(
        &self,
        nodes: &[(String, NodeStatus)],
        edges: &[(String, String)],
    ) -> DagVisualization {
        let _node_map: HashMap<&str, &NodeStatus> =
            nodes.iter().map(|(id, status)| (id.as_str(), status)).collect();

        // 拓扑布局
        let layout = self.compute_layout(nodes, edges);

        let mut rendered_nodes = Vec::new();
        for (id, status) in nodes {
            if let Some(&(row, col)) = layout.get(id.as_str()) {
                rendered_nodes.push(RenderedNode {
                    id: id.clone(),
                    label: self.truncate_label(id),
                    status: status.clone(),
                    row,
                    col,
                });
            }
        }

        let rendered_edges: Vec<RenderedEdge> = edges
            .iter()
            .map(|(from, to)| RenderedEdge {
                from: from.clone(),
                to: to.clone(),
            })
            .collect();

        let width = rendered_nodes.iter().map(|n| n.col + self.node_width).max().unwrap_or(0);

        let height = rendered_nodes.iter().map(|n| n.row + self.node_height).max().unwrap_or(0);

        DagVisualization {
            nodes: rendered_nodes,
            edges: rendered_edges,
            width,
            height,
        }
    }

    /// 使用拓扑排序计算每个节点的行列位置。
    fn compute_layout(
        &self,
        nodes: &[(String, NodeStatus)],
        edges: &[(String, String)],
    ) -> HashMap<String, (usize, usize)> {
        let mut layout = HashMap::new();
        let mut adj: HashMap<String, Vec<String>> = HashMap::new();
        let mut in_degree: HashMap<String, usize> = HashMap::new();

        for (id, _) in nodes {
            adj.entry(id.clone()).or_default();
            in_degree.entry(id.clone()).or_insert(0);
        }

        for (from, to) in edges {
            adj.entry(from.clone()).or_default().push(to.clone());
            *in_degree.entry(to.clone()).or_insert(0) += 1;
        }

        // 拓扑排序并分配层级
        let mut queue: Vec<String> =
            in_degree.iter().filter(|(_, &d)| d == 0).map(|(id, _)| id.clone()).collect();

        let mut levels: HashMap<String, usize> = HashMap::new();
        let mut col_counters: HashMap<usize, usize> = HashMap::new();

        // 优化（TODO 8.1.1）：sort_unstable 更快（不保证等值元素的相对顺序，
        // 但节点 ID 唯一性保证输出仍确定）；同时按子节点已确定的 level
        // 排序，避免后续二次遍历。
        while !queue.is_empty() {
            queue.sort_unstable();
            // 每轮取队首（最小字典序），与原算法语义等价
            let current = queue.remove(0);
            let level = levels.get(&current).copied().unwrap_or(0);

            let col = col_counters.entry(level).or_insert(0);
            let position = *col;
            *col += 1;

            layout.insert(current.clone(), (level, position));

            if let Some(children) = adj.get(&current) {
                for child in children {
                    let child_level = level + 1;
                    let current_child_level = levels.get(child).copied().unwrap_or(0);
                    levels.insert(child.clone(), child_level.max(current_child_level));

                    if let Some(degree) = in_degree.get_mut(child) {
                        *degree -= 1;
                        if *degree == 0 {
                            queue.push(child.clone());
                        }
                    }
                }
            }
        }

        // 将层级/位置转换为行列
        let mut result = HashMap::new();
        for (id, (level, position)) in layout {
            let row = level * (self.node_height + self.v_gap);
            let col = position * (self.node_width + self.h_gap);
            result.insert(id, (row, col));
        }

        result
    }

    fn truncate_label(&self, label: &str) -> String {
        if label.len() > self.node_width - 2 {
            format!("{}...", &label[..self.node_width - 5])
        } else {
            label.to_string()
        }
    }

    /// 渲染为 ASCII 字符串。
    pub fn render_to_string(
        &self,
        nodes: &[(String, NodeStatus)],
        edges: &[(String, String)],
    ) -> String {
        let viz = self.render(nodes, edges);
        if viz.width == 0 || viz.height == 0 {
            return "Empty DAG".to_string();
        }

        let mut canvas: Vec<Vec<char>> = vec![vec![' '; viz.width + 1]; viz.height + 1];

        // 先绘制边（简单的垂直/水平线）
        for edge in &viz.edges {
            if let (Some(from_node), Some(to_node)) = (
                viz.nodes.iter().find(|n| n.id == edge.from),
                viz.nodes.iter().find(|n| n.id == edge.to),
            ) {
                let from_center_x = from_node.col + self.node_width / 2;
                let from_bottom_y = from_node.row + self.node_height;
                let to_center_x = to_node.col + self.node_width / 2;
                let to_top_y = to_node.row;

                // 从源节点底部绘制垂直线
                let mid_y = (from_bottom_y + to_top_y) / 2;
                for y in from_bottom_y..=mid_y {
                    if y < canvas.len() && from_center_x < canvas[y].len() {
                        canvas[y][from_center_x] = '|';
                    }
                }

                // 绘制水平线
                let (start_x, end_x) = if from_center_x <= to_center_x {
                    (from_center_x, to_center_x)
                } else {
                    (to_center_x, from_center_x)
                };
                for x in start_x..=end_x {
                    if mid_y < canvas.len() && x < canvas[mid_y].len() {
                        canvas[mid_y][x] = if x == from_center_x || x == to_center_x {
                            '+'
                        } else {
                            '-'
                        };
                    }
                }

                // 绘制到目标节点顶部的垂直线
                for y in mid_y..=to_top_y {
                    if y < canvas.len() && to_center_x < canvas[y].len() {
                        canvas[y][to_center_x] = '|';
                    }
                }
            }
        }

        // 绘制节点
        for node in &viz.nodes {
            let (status_char, status_symbol) = match node.status {
                NodeStatus::Pending => ('-', "P"),
                NodeStatus::Running => ('~', "*"),
                NodeStatus::Completed => ('=', "+"),
                NodeStatus::Failed => ('!', "X"),
                NodeStatus::Skipped => ('.', "o"),
            };

            // 顶部边框
            let border: String = std::iter::repeat_n(status_char, self.node_width).collect();
            if node.row < canvas.len() && node.col + self.node_width <= canvas[node.row].len() {
                for (i, c) in border.chars().enumerate() {
                    canvas[node.row][node.col + i] = c;
                }
            }

            // 内容
            let content_row = node.row + 1;
            if content_row < canvas.len() {
                let padding = self.node_width.saturating_sub(node.label.len() + 4);
                let left_pad = padding / 2;
                let right_pad = padding - left_pad;
                let content = format!(
                    "{}{} {} {}{}",
                    status_symbol,
                    " ".repeat(left_pad),
                    node.label,
                    " ".repeat(right_pad),
                    status_symbol
                );
                for (i, c) in content.chars().enumerate() {
                    if node.col + i < canvas[content_row].len() {
                        canvas[content_row][node.col + i] = c;
                    }
                }
            }

            // 底部边框
            let bottom_row = node.row + 2;
            if bottom_row < canvas.len() && node.col + self.node_width <= canvas[bottom_row].len() {
                for (i, c) in border.chars().enumerate() {
                    canvas[bottom_row][node.col + i] = c;
                }
            }
        }

        // 将画布转换为字符串
        canvas
            .iter()
            .map(|row| row.iter().collect::<String>().trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Default for DagRenderer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_empty_dag() {
        let renderer = DagRenderer::new();
        let output = renderer.render_to_string(&[], &[]);
        assert_eq!(output, "Empty DAG");
    }

    #[test]
    fn test_render_single_node() {
        let renderer = DagRenderer::new();
        let nodes = vec![("node1".to_string(), NodeStatus::Pending)];
        let output = renderer.render_to_string(&nodes, &[]);
        assert!(output.contains("node1"));
    }

    #[test]
    fn test_render_two_nodes_with_edge() {
        let renderer = DagRenderer::new();
        let nodes = vec![
            ("start".to_string(), NodeStatus::Completed),
            ("end".to_string(), NodeStatus::Pending),
        ];
        let edges = vec![("start".to_string(), "end".to_string())];
        let output = renderer.render_to_string(&nodes, &edges);
        assert!(output.contains("start"));
        assert!(output.contains("end"));
    }

    #[test]
    fn test_render_with_statuses() {
        let renderer = DagRenderer::new();
        let nodes = vec![
            ("a".to_string(), NodeStatus::Completed),
            ("b".to_string(), NodeStatus::Running),
            ("c".to_string(), NodeStatus::Failed),
        ];
        let edges = vec![
            ("a".to_string(), "b".to_string()),
            ("b".to_string(), "c".to_string()),
        ];
        let output = renderer.render_to_string(&nodes, &edges);
        assert!(output.contains("a"));
        assert!(output.contains("b"));
        assert!(output.contains("c"));
    }
}
