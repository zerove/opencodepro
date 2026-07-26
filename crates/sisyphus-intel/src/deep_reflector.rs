//! Deep Reflector — multi-dimensional deviation analysis and cognitive corrections.
//! Provides deeper insight into execution deviations than the standard reflector.

use crate::reflect::{RecordedNode, ReflectionReport};
use crate::simulation::SimulationResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Dimension of analysis for deviation scoring
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum AnalysisDimension {
    OutputQuality,
    ResourceEfficiency,
    TemporalAccuracy,
    LogicCorrectness,
    ToolReliability,
}

/// Deviation score for a single dimension
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DimensionScore {
    pub dimension: AnalysisDimension,
    pub score: f64, // 0.0 = perfect, 1.0 = maximum deviation
    pub weight: f64,
    pub explanation: String,
}

/// Multi-dimensional deviation analysis result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepDeviationReport {
    /// Overall weighted deviation score (0.0 = perfect)
    pub overall_score: f64,
    /// Per-dimension scores
    pub dimensions: Vec<DimensionScore>,
    /// Node-level breakdown
    pub node_reports: Vec<NodeDeviationReport>,
    /// Cognitive corrections suggested
    pub corrections: Vec<CognitiveCorrection>,
    /// Pattern detected across iterations
    pub patterns: Vec<DeviationPattern>,
}

/// Node-level deviation report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeDeviationReport {
    pub node_name: String,
    pub deviation_score: f64,
    pub primary_dimension: AnalysisDimension,
    pub root_cause: String,
    pub suggestions: Vec<String>,
}

/// A suggested cognitive correction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CognitiveCorrection {
    pub target_node: String,
    pub correction_type: CorrectionType,
    pub description: String,
    pub expected_improvement: f64,
    pub priority: CorrectionPriority,
}

/// Type of cognitive correction
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum CorrectionType {
    IncreaseFuel,
    AddRetry,
    ModifyLogic,
    ChangeTool,
    AddValidation,
    ReorderNodes,
    AddFallback,
    SplitNode,
}

/// Priority of correction
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum CorrectionPriority {
    Critical,
    High,
    Medium,
    Low,
}

/// A detected deviation pattern
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviationPattern {
    pub pattern_type: String,
    pub description: String,
    pub frequency: usize,
    pub affected_nodes: Vec<String>,
    pub recommended_action: String,
}

/// Deep reflector that performs multi-dimensional deviation analysis
pub struct DeepReflector {
    /// Weight configuration for each analysis dimension
    dimension_weights: HashMap<AnalysisDimension, f64>,
    /// History of deviations for pattern detection
    deviation_history: Vec<DeepDeviationReport>,
}

impl DeepReflector {
    pub fn new() -> Self {
        let mut dimension_weights = HashMap::new();
        dimension_weights.insert(AnalysisDimension::OutputQuality, 0.30);
        dimension_weights.insert(AnalysisDimension::ResourceEfficiency, 0.20);
        dimension_weights.insert(AnalysisDimension::TemporalAccuracy, 0.15);
        dimension_weights.insert(AnalysisDimension::LogicCorrectness, 0.25);
        dimension_weights.insert(AnalysisDimension::ToolReliability, 0.10);

        Self {
            dimension_weights,
            deviation_history: Vec::new(),
        }
    }

    pub fn with_weights(weights: HashMap<AnalysisDimension, f64>) -> Self {
        Self {
            dimension_weights: weights,
            deviation_history: Vec::new(),
        }
    }

    /// Perform deep deviation analysis on a reflection report
    pub fn analyze(
        &mut self,
        report: &ReflectionReport,
        prediction: &SimulationResult,
    ) -> DeepDeviationReport {
        let node_reports = self.analyze_nodes(report);
        let dimensions = self.compute_dimension_scores(report, prediction);
        let overall_score = self.compute_overall_score(&dimensions);
        let corrections = self.generate_corrections(&node_reports, &dimensions);
        let patterns = self.detect_patterns();

        let deep_report = DeepDeviationReport {
            overall_score,
            dimensions,
            node_reports,
            corrections,
            patterns,
        };

        self.deviation_history.push(deep_report.clone());
        deep_report
    }

    fn analyze_nodes(&self, report: &ReflectionReport) -> Vec<NodeDeviationReport> {
        report
            .records
            .iter()
            .map(|record| {
                let (score, dimension, root_cause) = self.score_node(record);
                let suggestions = self.suggest_node_improvements(record, score, dimension);

                NodeDeviationReport {
                    node_name: record.node.clone(),
                    deviation_score: score,
                    primary_dimension: dimension,
                    root_cause,
                    suggestions,
                }
            })
            .collect()
    }

    fn score_node(&self, record: &RecordedNode) -> (f64, AnalysisDimension, String) {
        match record.status.as_str() {
            "success" => {
                // Check output quality
                let quality_score = self.score_output_quality(&record.output);
                if quality_score > 0.3 {
                    (
                        quality_score,
                        AnalysisDimension::OutputQuality,
                        format!("Output quality suboptimal: {}", record.output),
                    )
                } else {
                    (
                        quality_score,
                        AnalysisDimension::OutputQuality,
                        "Minor output quality issues".to_string(),
                    )
                }
            }
            "failed" | "error" => {
                let lower = record.output.to_lowercase();
                if lower.contains("timeout") {
                    (
                        0.9,
                        AnalysisDimension::TemporalAccuracy,
                        format!("Node timed out: {}", record.output),
                    )
                } else if lower.contains("fuel") {
                    (
                        0.85,
                        AnalysisDimension::ResourceEfficiency,
                        format!("Resource exhaustion: {}", record.output),
                    )
                } else if lower.contains("tool") {
                    (
                        0.7,
                        AnalysisDimension::ToolReliability,
                        format!("Tool failure: {}", record.output),
                    )
                } else {
                    (
                        0.8,
                        AnalysisDimension::LogicCorrectness,
                        format!("Logic error: {}", record.output),
                    )
                }
            }
            _ => (
                0.5,
                AnalysisDimension::LogicCorrectness,
                format!("Unexpected status: {}", record.status),
            ),
        }
    }

    fn score_output_quality(&self, output: &str) -> f64 {
        if output.is_empty() {
            return 0.7;
        }
        // Heuristic: check for error indicators in output
        let lower = output.to_lowercase();
        if lower.contains("error") || lower.contains("fail") {
            0.6
        } else if lower.contains("warning") || lower.contains("partial") {
            0.3
        } else {
            0.1
        }
    }

    fn compute_dimension_scores(
        &self,
        report: &ReflectionReport,
        prediction: &SimulationResult,
    ) -> Vec<DimensionScore> {
        let mut scores = Vec::new();

        // Output quality
        let output_failures = report
            .records
            .iter()
            .filter(|r| r.status == "failed" || r.status == "error")
            .count();
        let output_score = output_failures as f64 / report.records.len().max(1) as f64;
        scores.push(DimensionScore {
            dimension: AnalysisDimension::OutputQuality,
            score: output_score,
            weight: *self
                .dimension_weights
                .get(&AnalysisDimension::OutputQuality)
                .unwrap_or(&0.3),
            explanation: format!("{}/{} nodes failed", output_failures, report.records.len()),
        });

        // Resource efficiency
        let resource_score = (1.0 - prediction.predicted_stability).clamp(0.0, 1.0);
        scores.push(DimensionScore {
            dimension: AnalysisDimension::ResourceEfficiency,
            score: resource_score,
            weight: *self
                .dimension_weights
                .get(&AnalysisDimension::ResourceEfficiency)
                .unwrap_or(&0.2),
            explanation: format!("Predicted stability: {:.2}", prediction.predicted_stability),
        });

        // Temporal accuracy
        let temporal_score = if prediction.predicted_effort > 0 {
            (prediction.estimated_cost as f64 / prediction.predicted_effort as f64 - 1.0)
                .abs()
                .min(1.0)
        } else {
            0.5
        };
        scores.push(DimensionScore {
            dimension: AnalysisDimension::TemporalAccuracy,
            score: temporal_score,
            weight: *self
                .dimension_weights
                .get(&AnalysisDimension::TemporalAccuracy)
                .unwrap_or(&0.15),
            explanation: format!("Cost/effort ratio: {:.2}", temporal_score),
        });

        // Logic correctness
        let logic_score = report.deviation_score.clamp(0.0, 1.0);
        scores.push(DimensionScore {
            dimension: AnalysisDimension::LogicCorrectness,
            score: logic_score,
            weight: *self
                .dimension_weights
                .get(&AnalysisDimension::LogicCorrectness)
                .unwrap_or(&0.25),
            explanation: format!("Deviation score: {:.4}", report.deviation_score),
        });

        // Tool reliability
        let tool_failures = report
            .records
            .iter()
            .filter(|r| r.output.to_lowercase().contains("tool"))
            .count();
        let tool_score = tool_failures as f64 / report.records.len().max(1) as f64;
        scores.push(DimensionScore {
            dimension: AnalysisDimension::ToolReliability,
            score: tool_score,
            weight: *self
                .dimension_weights
                .get(&AnalysisDimension::ToolReliability)
                .unwrap_or(&0.1),
            explanation: format!(
                "{}/{} tool-related failures",
                tool_failures,
                report.records.len()
            ),
        });

        scores
    }

    fn compute_overall_score(&self, dimensions: &[DimensionScore]) -> f64 {
        let total_weight: f64 = dimensions.iter().map(|d| d.weight).sum();
        if total_weight == 0.0 {
            return 0.0;
        }
        dimensions.iter().map(|d| d.score * d.weight).sum::<f64>() / total_weight
    }

    fn generate_corrections(
        &self,
        node_reports: &[NodeDeviationReport],
        _dimensions: &[DimensionScore],
    ) -> Vec<CognitiveCorrection> {
        let mut corrections = Vec::new();

        for node in node_reports {
            if node.deviation_score > 0.5 {
                let (corr_type, priority) = match node.primary_dimension {
                    AnalysisDimension::ResourceEfficiency => {
                        (CorrectionType::IncreaseFuel, CorrectionPriority::High)
                    }
                    AnalysisDimension::TemporalAccuracy => {
                        (CorrectionType::AddRetry, CorrectionPriority::High)
                    }
                    AnalysisDimension::ToolReliability => {
                        (CorrectionType::ChangeTool, CorrectionPriority::Medium)
                    }
                    AnalysisDimension::LogicCorrectness => {
                        (CorrectionType::ModifyLogic, CorrectionPriority::Critical)
                    }
                    AnalysisDimension::OutputQuality => {
                        (CorrectionType::AddValidation, CorrectionPriority::Medium)
                    }
                };

                corrections.push(CognitiveCorrection {
                    target_node: node.node_name.clone(),
                    correction_type: corr_type,
                    description: node.root_cause.clone(),
                    expected_improvement: (1.0 - node.deviation_score) * 0.3,
                    priority,
                });
            }
        }

        corrections.sort_by(|a, b| a.priority.cmp(&b.priority));
        corrections
    }

    fn suggest_node_improvements(
        &self,
        _record: &RecordedNode,
        score: f64,
        dimension: AnalysisDimension,
    ) -> Vec<String> {
        let mut suggestions = Vec::new();
        if score > 0.5 {
            match dimension {
                AnalysisDimension::ResourceEfficiency => {
                    suggestions.push(
                        "Consider increasing fuel limit or optimizing resource usage".to_string(),
                    );
                }
                AnalysisDimension::TemporalAccuracy => {
                    suggestions
                        .push("Node may need timeout increase or parallel execution".to_string());
                }
                AnalysisDimension::LogicCorrectness => {
                    suggestions.push("Review node logic for correctness".to_string());
                }
                AnalysisDimension::ToolReliability => {
                    suggestions
                        .push("Consider adding tool fallback or retry mechanism".to_string());
                }
                AnalysisDimension::OutputQuality => {
                    suggestions.push("Add output validation step after this node".to_string());
                }
            }
        }
        suggestions
    }

    fn detect_patterns(&self) -> Vec<DeviationPattern> {
        let mut patterns = Vec::new();

        if self.deviation_history.len() < 2 {
            return patterns;
        }

        // Detect recurring node failures
        let mut node_failures: HashMap<String, usize> = HashMap::new();
        for report in &self.deviation_history {
            for node in &report.node_reports {
                if node.deviation_score > 0.5 {
                    *node_failures.entry(node.node_name.clone()).or_insert(0) += 1;
                }
            }
        }

        for (node, count) in &node_failures {
            if *count >= 2 {
                patterns.push(DeviationPattern {
                    pattern_type: "recurring_failure".to_string(),
                    description: format!(
                        "Node '{}' has deviated in {} consecutive iterations",
                        node, count
                    ),
                    frequency: *count,
                    affected_nodes: vec![node.clone()],
                    recommended_action: "Consider restructuring or replacing this node".to_string(),
                });
            }
        }

        // Detect increasing deviation trend
        if self.deviation_history.len() >= 3 {
            let recent: Vec<f64> = self
                .deviation_history
                .iter()
                .rev()
                .take(3)
                .map(|r| r.overall_score)
                .collect();
            if recent[0] > recent[1] && recent[1] > recent[2] {
                patterns.push(DeviationPattern {
                    pattern_type: "increasing_deviation".to_string(),
                    description: "Deviation is increasing over iterations".to_string(),
                    frequency: 3,
                    affected_nodes: vec![],
                    recommended_action: "Consider fundamental strategy change".to_string(),
                });
            }
        }

        patterns
    }
}

impl Default for DeepReflector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_report() -> ReflectionReport {
        ReflectionReport {
            original_sop_id: "test_sop".to_string(),
            deviation_score: 0.35,
            suggested_improvements: vec!["Add validation".to_string()],
            evolved_sop_structure: None,
            records: vec![
                RecordedNode {
                    node: "step1".to_string(),
                    status: "success".to_string(),
                    output: "ok".to_string(),
                },
                RecordedNode {
                    node: "step2".to_string(),
                    status: "failed".to_string(),
                    output: "timeout error".to_string(),
                },
            ],
        }
    }

    fn test_prediction() -> SimulationResult {
        SimulationResult {
            path_id: "test".to_string(),
            success_probability: 0.75,
            estimated_cost: 2000,
            predicted_stability: 0.8,
            predicted_effort: 1500,
            predicted_outcome: "Test".to_string(),
        }
    }

    #[test]
    fn test_deep_analysis() {
        let mut reflector = DeepReflector::new();
        let report = reflector.analyze(&test_report(), &test_prediction());
        assert!(report.overall_score >= 0.0 && report.overall_score <= 1.0);
        assert_eq!(report.dimensions.len(), 5);
        assert!(!report.corrections.is_empty());
    }

    #[test]
    fn test_pattern_detection() {
        let mut reflector = DeepReflector::new();
        // Run multiple analyses to trigger pattern detection
        for _ in 0..3 {
            reflector.analyze(&test_report(), &test_prediction());
        }
        let report = reflector.analyze(&test_report(), &test_prediction());
        // Should detect recurring failure pattern for step2
        assert!(report
            .patterns
            .iter()
            .any(|p| p.pattern_type == "recurring_failure"));
    }
}
