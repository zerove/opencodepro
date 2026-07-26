//! Failure Pattern Evolver — advanced SOP evolution based on failure pattern library.
//! Analyzes historical failures and applies targeted mutations to prevent recurrence.

use crate::reflect::ReflectionReport;
use crate::topological_evolver::TopologicalSOPEvolver;
use serde::{Deserialize, Serialize};
use sisyphus_types::EvolutionOp;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

/// Category of failure pattern
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum FailureCategory {
    /// Node exceeded fuel budget
    FuelExhaustion,
    /// Node produced wrong output type
    OutputMismatch,
    /// Dependency was not satisfied
    DependencyMissing,
    /// Node timed out
    Timeout,
    /// External tool returned error
    ToolError,
    /// Logic error in node processing
    LogicError,
    /// Resource unavailable
    ResourceUnavailable,
}

/// A recorded failure pattern with associated evolution strategy
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

/// Failure pattern library for tracking and learning from past failures
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

    /// Record a new failure observation
    pub fn record_failure(
        &mut self,
        node_name: &str,
        category: FailureCategory,
        description: &str,
    ) {
        // Update count
        self.node_failure_counts
            .entry(node_name.to_string())
            .or_default()
            .entry(category)
            .and_modify(|c| *c += 1)
            .or_insert(1);

        // Check if pattern already exists
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

    /// Get patterns for a specific node
    pub fn get_node_patterns(&self, node_name: &str) -> Vec<&FailurePattern> {
        self.patterns
            .iter()
            .filter(|p| p.affected_node == node_name)
            .collect()
    }

    /// Get the most common failure category for a node
    pub fn most_common_failure(&self, node_name: &str) -> Option<FailureCategory> {
        self.node_failure_counts.get(node_name).and_then(|counts| {
            counts
                .iter()
                .max_by_key(|(_, count)| *count)
                .map(|(cat, _)| *cat)
        })
    }

    /// Get all patterns sorted by occurrence count
    pub fn get_hotspots(&self) -> Vec<&FailurePattern> {
        let mut patterns: Vec<_> = self.patterns.iter().collect();
        patterns.sort_by(|a, b| b.occurrence_count.cmp(&a.occurrence_count));
        patterns
    }
}

impl Default for FailurePatternLibrary {
    fn default() -> Self {
        Self::new()
    }
}

/// Advanced evolver that uses failure patterns to generate targeted mutations
pub struct FailurePatternEvolver {
    pattern_library: Arc<parking_lot::RwLock<FailurePatternLibrary>>,
    base_evolver: TopologicalSOPEvolver,
}

impl FailurePatternEvolver {
    pub fn new(pattern_library: Arc<parking_lot::RwLock<FailurePatternLibrary>>) -> Self {
        Self {
            pattern_library,
            base_evolver: TopologicalSOPEvolver::new(),
        }
    }

    /// Analyze a reflection report and suggest targeted mutations
    pub fn analyze_and_suggest(&self, report: &ReflectionReport) -> Vec<MutationSuggestion> {
        let mut suggestions = Vec::new();
        let library = self.pattern_library.read();

        for record in &report.records {
            if record.status == "failed" || record.status == "error" {
                let category = Self::classify_failure(&record.output);

                // Check if we've seen this pattern before
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
                    // First time seeing this pattern - still suggest basic mitigation
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

    /// Generate evolution operations based on failure patterns
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

/// A mutation suggestion with confidence score
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
