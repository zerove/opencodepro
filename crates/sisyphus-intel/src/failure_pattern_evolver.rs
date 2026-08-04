//! 失败模式演进器 — 基于失败模式库的高级 SOP 演进。
//! 分析历史失败并应用针对性变异以防止复发。

use crate::reflect::ReflectionReport;
use crate::topological_evolver::TopologicalSOPEvolver;
use serde::{Deserialize, Serialize};
use sisyphus_types::EvolutionOp;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

/// 失败模式的类别
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum FailureCategory {
    /// 节点超出燃料预算
    FuelExhaustion,
    /// 节点产生错误输出类型
    OutputMismatch,
    /// 依赖未满足
    DependencyMissing,
    /// 节点超时
    Timeout,
    /// 外部工具返回错误
    ToolError,
    /// 节点处理中的逻辑错误
    LogicError,
    /// 资源不可用
    ResourceUnavailable,
}

/// 带关联演进策略的已记录失败模式
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailurePattern {
    pub id: String,
    pub category: FailureCategory,
    pub description: String,
    pub affected_node: String,
    pub occurrence_count: usize,
    pub last_seen: u64,
    pub evolution_ops: Vec<EvolutionOp>,
    pub success_rate_after_fix: f64,
}

/// 跟踪并从过去失败中学习的失败模式库
pub struct FailurePatternLibrary {
    patterns: Vec<FailurePattern>,
    node_failure_counts: HashMap<String, HashMap<FailureCategory, usize>>,
}

impl FailurePatternLibrary {
    pub fn new() -> Self {
        Self {
            patterns: Vec::new(),
            node_failure_counts: HashMap::new(),
        }
    }

    /// 记录新的失败观测
    pub fn record_failure(
        &mut self,
        node_name: &str,
        category: FailureCategory,
        description: &str,
    ) {
        // 更新计数
        self.node_failure_counts
            .entry(node_name.to_string())
            .or_default()
            .entry(category)
            .and_modify(|c| *c += 1)
            .or_insert(1);

        // 检查模式是否已存在
        let existing = self
            .patterns
            .iter_mut()
            .find(|p| p.affected_node == node_name && p.category == category);

        if let Some(pattern) = existing {
            pattern.occurrence_count += 1;
            pattern.last_seen = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
        } else {
            let pattern = FailurePattern {
                id: format!("fp_{}_{}", node_name, category as u8),
                category,
                description: description.to_string(),
                affected_node: node_name.to_string(),
                occurrence_count: 1,
                last_seen: SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                evolution_ops: Vec::new(),
                success_rate_after_fix: 0.0,
            };
            self.patterns.push(pattern);
        }
    }

    /// 获取特定节点的模式
    pub fn get_node_patterns(&self, node_name: &str) -> Vec<&FailurePattern> {
        self.patterns
            .iter()
            .filter(|p| p.affected_node == node_name)
            .collect()
    }

    /// 获取节点最常见的失败类别
    pub fn most_common_failure(&self, node_name: &str) -> Option<FailureCategory> {
        self.node_failure_counts.get(node_name).and_then(|counts| {
            counts
                .iter()
                .max_by_key(|(_, count)| *count)
                .map(|(cat, _)| *cat)
        })
    }

    /// 获取按出现次数排序的所有模式
    pub fn get_hotspots(&self) -> Vec<&FailurePattern> {
        let mut patterns: Vec<_> = self.patterns.iter().collect();
        patterns.sort_by_key(|b| std::cmp::Reverse(b.occurrence_count));
        patterns
    }
}

impl Default for FailurePatternLibrary {
    fn default() -> Self {
        Self::new()
    }
}

/// 使用失败模式生成针对性变异的高级演进器
pub struct FailurePatternEvolver {
    pattern_library: Arc<parking_lot::RwLock<FailurePatternLibrary>>,
    #[expect(dead_code)]
    base_evolver: TopologicalSOPEvolver,
}

impl FailurePatternEvolver {
    pub fn new(pattern_library: Arc<parking_lot::RwLock<FailurePatternLibrary>>) -> Self {
        Self {
            pattern_library,
            base_evolver: TopologicalSOPEvolver::new(),
        }
    }

    /// 分析反思报告并建议针对性变异
    pub fn analyze_and_suggest(&self, report: &ReflectionReport) -> Vec<MutationSuggestion> {
        let mut suggestions = Vec::new();
        let library = self.pattern_library.read();

        for record in &report.records {
            if record.status == "failed" || record.status == "error" {
                let category = Self::classify_failure(&record.output);

                // 检查此前是否见过该模式
                if let Some(existing_pattern) = library
                    .get_node_patterns(&record.node)
                    .iter()
                    .find(|p| p.category == category)
                    .copied()
                {
                    if existing_pattern.occurrence_count >= 3 {
                        suggestions.push(MutationSuggestion {
                            node_name: record.node.clone(),
                            category,
                            description: format!(
                                "Node '{}' has failed {} times with {:?}. Recommend: {}",
                                record.node,
                                existing_pattern.occurrence_count,
                                category,
                                Self::suggest_mutation_for_category(category)
                            ),
                            confidence: (existing_pattern.occurrence_count as f64 / 10.0).min(0.95),
                        });
                    }
                } else {
                    // 首次见到该模式 — 仍建议基础缓解措施
                    suggestions.push(MutationSuggestion {
                        node_name: record.node.clone(),
                        category,
                        description: format!(
                            "New failure pattern detected for node '{}': {:?} - {}",
                            record.node, category, record.output
                        ),
                        confidence: 0.3,
                    });
                }
            }
        }

        suggestions.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        suggestions
    }

    /// 基于失败模式生成演进操作
    pub fn generate_evolution_ops(&self, report: &ReflectionReport) -> Vec<EvolutionOp> {
        let suggestions = self.analyze_and_suggest(report);
        let mut ops = Vec::new();

        for suggestion in &suggestions {
            if suggestion.confidence > 0.5 {
                match suggestion.category {
                    FailureCategory::FuelExhaustion => {
                        ops.push(EvolutionOp::AdjustFuel {
                            node_name: suggestion.node_name.clone(),
                            delta: 2000,
                        });
                    }
                    FailureCategory::Timeout => {
                        ops.push(EvolutionOp::AdjustFuel {
                            node_name: suggestion.node_name.clone(),
                            delta: 5000,
                        });
                        ops.push(EvolutionOp::AddRetry {
                            node_name: suggestion.node_name.clone(),
                            max_retries: 3,
                        });
                    }
                    FailureCategory::ToolError | FailureCategory::LogicError => {
                        ops.push(EvolutionOp::AddRetry {
                            node_name: suggestion.node_name.clone(),
                            max_retries: 2,
                        });
                    }
                    _ => {
                        ops.push(EvolutionOp::AddRetry {
                            node_name: suggestion.node_name.clone(),
                            max_retries: 2,
                        });
                    }
                }
            }
        }

        ops
    }

    fn classify_failure(output: &str) -> FailureCategory {
        let lower = output.to_lowercase();
        if lower.contains("fuel") || lower.contains("budget") {
            FailureCategory::FuelExhaustion
        } else if lower.contains("timeout") || lower.contains("timed out") {
            FailureCategory::Timeout
        } else if lower.contains("not found") || lower.contains("missing") {
            FailureCategory::DependencyMissing
        } else if lower.contains("tool error") || lower.contains("tool failed") {
            FailureCategory::ToolError
        } else if lower.contains("type mismatch") || lower.contains("output") {
            FailureCategory::OutputMismatch
        } else {
            FailureCategory::LogicError
        }
    }

    fn suggest_mutation_for_category(category: FailureCategory) -> &'static str {
        match category {
            FailureCategory::FuelExhaustion => "Increase fuel limit by 50%",
            FailureCategory::Timeout => "Add retry with exponential backoff",
            FailureCategory::OutputMismatch => "Add type validation node before this node",
            FailureCategory::DependencyMissing => "Add dependency check and fallback",
            FailureCategory::ToolError => "Add retry with alternative tool",
            FailureCategory::LogicError => "Refactor node logic with guard clauses",
            FailureCategory::ResourceUnavailable => "Add resource availability check",
        }
    }
}

/// 带置信度分数的变异建议
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationSuggestion {
    pub node_name: String,
    pub category: FailureCategory,
    pub description: String,
    pub confidence: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pattern_library() {
        let mut lib = FailurePatternLibrary::new();
        lib.record_failure("node_a", FailureCategory::FuelExhaustion, "out of fuel");
        lib.record_failure("node_a", FailureCategory::FuelExhaustion, "fuel exhausted");
        lib.record_failure("node_b", FailureCategory::Timeout, "timed out");

        assert_eq!(lib.get_node_patterns("node_a").len(), 1);
        assert_eq!(
            lib.most_common_failure("node_a"),
            Some(FailureCategory::FuelExhaustion)
        );
        assert_eq!(lib.get_hotspots().len(), 2);
    }

    #[test]
    fn test_classify_failure() {
        assert_eq!(
            FailurePatternEvolver::classify_failure("fuel budget exceeded"),
            FailureCategory::FuelExhaustion
        );
        assert_eq!(
            FailurePatternEvolver::classify_failure("operation timed out"),
            FailureCategory::Timeout
        );
        assert_eq!(
            FailurePatternEvolver::classify_failure("dependency not found"),
            FailureCategory::DependencyMissing
        );
    }
}
