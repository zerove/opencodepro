//! 集成测试：intel deep_reflector 偏差评分数据类型
//!
//! 覆盖 `sisyphus_intel::deep_reflector` 中的纯数据模块：
//! - `AnalysisDimension` 5 个变体 + serde
//! - `DimensionScore` 字段
//! - `DeepDeviationReport` 字段
//! - 其他结构（`NodeDeviationReport`/`CognitiveCorrection`/`DeviationPattern`）

use sisyphus_intel::deep_reflector::{
    AnalysisDimension, CognitiveCorrection, CorrectionPriority, CorrectionType,
    DeepDeviationReport, DeviationPattern, DimensionScore, NodeDeviationReport,
};

// ---------------------------------------------------------------------------
// AnalysisDimension
// ---------------------------------------------------------------------------

#[test]
fn analysis_dimension_serde_round_trip() {
    for d in [
        AnalysisDimension::OutputQuality,
        AnalysisDimension::ResourceEfficiency,
        AnalysisDimension::TemporalAccuracy,
        AnalysisDimension::LogicCorrectness,
        AnalysisDimension::ToolReliability,
    ] {
        let json = serde_json::to_string(&d).unwrap();
        let back: AnalysisDimension = serde_json::from_str(&json).unwrap();
        assert_eq!(d, back);
    }
}

#[test]
fn analysis_dimension_distinct() {
    assert_ne!(
        AnalysisDimension::OutputQuality,
        AnalysisDimension::ResourceEfficiency
    );
    assert_ne!(
        AnalysisDimension::LogicCorrectness,
        AnalysisDimension::ToolReliability
    );
}

#[test]
fn analysis_dimension_copy() {
    let a = AnalysisDimension::TemporalAccuracy;
    let b = a;
    assert_eq!(a, b);
}

#[test]
fn analysis_dimension_debug() {
    let s = format!("{:?}", AnalysisDimension::OutputQuality);
    assert!(s.contains("OutputQuality"));
}

#[test]
fn analysis_dimension_hash() {
    use std::collections::HashSet;
    let mut s = HashSet::new();
    s.insert(AnalysisDimension::OutputQuality);
    s.insert(AnalysisDimension::OutputQuality); // dup
    s.insert(AnalysisDimension::ToolReliability);
    assert_eq!(s.len(), 2);
}

// ---------------------------------------------------------------------------
// DimensionScore
// ---------------------------------------------------------------------------

fn make_dim_score(d: AnalysisDimension, score: f64, weight: f64) -> DimensionScore {
    DimensionScore {
        dimension: d,
        score,
        weight,
        explanation: format!("Explanation for {d:?}"),
    }
}

#[test]
fn dim_score_field_access() {
    let s = make_dim_score(AnalysisDimension::OutputQuality, 0.3, 0.5);
    assert_eq!(s.dimension, AnalysisDimension::OutputQuality);
    assert!((s.score - 0.3).abs() < 1e-9);
    assert!((s.weight - 0.5).abs() < 1e-9);
    assert!(!s.explanation.is_empty());
}

#[test]
fn dim_score_clone() {
    let s = make_dim_score(AnalysisDimension::OutputQuality, 0.3, 0.5);
    let s2 = s.clone();
    assert_eq!(s.dimension, s2.dimension);
    assert_eq!(s.score, s2.score);
}

#[test]
fn dim_score_serde() {
    let s = make_dim_score(AnalysisDimension::LogicCorrectness, 0.0, 1.0);
    let json = serde_json::to_string(&s).unwrap();
    let back: DimensionScore = serde_json::from_str(&json).unwrap();
    assert_eq!(back.dimension, s.dimension);
    assert_eq!(back.score, s.score);
}

#[test]
fn dim_score_zero_score() {
    let s = make_dim_score(AnalysisDimension::OutputQuality, 0.0, 0.0);
    assert_eq!(s.score, 0.0);
    assert_eq!(s.weight, 0.0);
}

#[test]
fn dim_score_max_score() {
    let s = make_dim_score(AnalysisDimension::OutputQuality, 1.0, 1.0);
    assert_eq!(s.score, 1.0);
}

// ---------------------------------------------------------------------------
// DeepDeviationReport
// ---------------------------------------------------------------------------

fn make_report() -> DeepDeviationReport {
    DeepDeviationReport {
        overall_score: 0.5,
        dimensions: vec![
            make_dim_score(AnalysisDimension::OutputQuality, 0.3, 0.5),
            make_dim_score(AnalysisDimension::ResourceEfficiency, 0.7, 0.5),
        ],
        node_reports: vec![],
        corrections: vec![],
        patterns: vec![],
    }
}

#[test]
fn report_field_access() {
    let r = make_report();
    assert!((r.overall_score - 0.5).abs() < 1e-9);
    assert_eq!(r.dimensions.len(), 2);
    assert!(r.node_reports.is_empty());
    assert!(r.corrections.is_empty());
    assert!(r.patterns.is_empty());
}

#[test]
fn report_empty() {
    let r = DeepDeviationReport {
        overall_score: 0.0,
        dimensions: vec![],
        node_reports: vec![],
        corrections: vec![],
        patterns: vec![],
    };
    assert_eq!(r.overall_score, 0.0);
    assert!(r.dimensions.is_empty());
}

#[test]
fn report_clone() {
    let r = make_report();
    let r2 = r.clone();
    assert_eq!(r.overall_score, r2.overall_score);
    assert_eq!(r.dimensions.len(), r2.dimensions.len());
}

#[test]
fn report_serde() {
    let r = make_report();
    let json = serde_json::to_string(&r).unwrap();
    let back: DeepDeviationReport = serde_json::from_str(&json).unwrap();
    assert_eq!(back.overall_score, r.overall_score);
    assert_eq!(back.dimensions.len(), 2);
}

#[test]
fn report_debug() {
    let r = make_report();
    let s = format!("{r:?}");
    assert!(s.contains("DeepDeviationReport"));
    assert!(s.contains("overall_score"));
}

// ---------------------------------------------------------------------------
// NodeDeviationReport
// ---------------------------------------------------------------------------

#[test]
fn node_report_construction() {
    let n = NodeDeviationReport {
        node_name: "n1".to_string(),
        deviation_score: 0.5,
        primary_dimension: AnalysisDimension::OutputQuality,
        root_cause: "logic error".to_string(),
        suggestions: vec!["s1".to_string(), "s2".to_string()],
    };
    assert_eq!(n.node_name, "n1");
    assert!((n.deviation_score - 0.5).abs() < 1e-9);
    assert_eq!(n.primary_dimension, AnalysisDimension::OutputQuality);
    assert_eq!(n.root_cause, "logic error");
    assert_eq!(n.suggestions.len(), 2);
}

#[test]
fn node_report_clone() {
    let n = NodeDeviationReport {
        node_name: "n".to_string(),
        deviation_score: 0.1,
        primary_dimension: AnalysisDimension::OutputQuality,
        root_cause: "x".to_string(),
        suggestions: vec![],
    };
    let n2 = n.clone();
    assert_eq!(n.node_name, n2.node_name);
}

#[test]
fn node_report_serde() {
    let n = NodeDeviationReport {
        node_name: "n".to_string(),
        deviation_score: 0.0,
        primary_dimension: AnalysisDimension::OutputQuality,
        root_cause: "".to_string(),
        suggestions: vec![],
    };
    let json = serde_json::to_string(&n).unwrap();
    let back: NodeDeviationReport = serde_json::from_str(&json).unwrap();
    assert_eq!(back.node_name, "n");
}

// ---------------------------------------------------------------------------
// CognitiveCorrection
// ---------------------------------------------------------------------------

#[test]
fn cognitive_correction_construction() {
    let c = CognitiveCorrection {
        target_node: "n1".to_string(),
        correction_type: sisyphus_intel::deep_reflector::CorrectionType::IncreaseFuel,
        description: "Fix logic".to_string(),
        expected_improvement: 0.7,
        priority: CorrectionPriority::High,
    };
    assert_eq!(c.target_node, "n1");
    assert_eq!(c.priority, CorrectionPriority::High);
    assert!((c.expected_improvement - 0.7).abs() < 1e-9);
}

#[test]
fn cognitive_correction_clone() {
    let c = CognitiveCorrection {
        target_node: "n".to_string(),
        correction_type: sisyphus_intel::deep_reflector::CorrectionType::AddRetry,
        description: "d".to_string(),
        expected_improvement: 0.1,
        priority: CorrectionPriority::Low,
    };
    let c2 = c.clone();
    assert_eq!(c.target_node, c2.target_node);
}

#[test]
fn cognitive_correction_serde() {
    let c = CognitiveCorrection {
        target_node: "n".to_string(),
        correction_type: sisyphus_intel::deep_reflector::CorrectionType::SplitNode,
        description: "d".to_string(),
        expected_improvement: 0.5,
        priority: CorrectionPriority::Medium,
    };
    let json = serde_json::to_string(&c).unwrap();
    let back: CognitiveCorrection = serde_json::from_str(&json).unwrap();
    assert_eq!(back.target_node, "n");
}

// ---------------------------------------------------------------------------
// DeviationPattern
// ---------------------------------------------------------------------------

#[test]
fn deviation_pattern_construction() {
    let p = DeviationPattern {
        pattern_type: "circular".to_string(),
        description: "d".to_string(),
        frequency: 3,
        affected_nodes: vec!["a".to_string(), "b".to_string()],
        recommended_action: "restructure".to_string(),
    };
    assert_eq!(p.pattern_type, "circular");
    assert_eq!(p.frequency, 3);
    assert_eq!(p.affected_nodes.len(), 2);
}

#[test]
fn deviation_pattern_clone() {
    let p = DeviationPattern {
        pattern_type: "p".to_string(),
        description: "d".to_string(),
        frequency: 1,
        affected_nodes: vec![],
        recommended_action: "f".to_string(),
    };
    let p2 = p.clone();
    assert_eq!(p.pattern_type, p2.pattern_type);
}

#[test]
fn deviation_pattern_serde() {
    let p = DeviationPattern {
        pattern_type: "p".to_string(),
        description: "d".to_string(),
        frequency: 5,
        affected_nodes: vec!["a".to_string()],
        recommended_action: "fix".to_string(),
    };
    let json = serde_json::to_string(&p).unwrap();
    let back: DeviationPattern = serde_json::from_str(&json).unwrap();
    assert_eq!(back.frequency, 5);
}

// ---------------------------------------------------------------------------
// Additional enums
// ---------------------------------------------------------------------------

#[test]
fn correction_type_distinct_variants() {
    let variants = [
        CorrectionType::IncreaseFuel,
        CorrectionType::AddRetry,
        CorrectionType::ModifyLogic,
        CorrectionType::ChangeTool,
        CorrectionType::AddValidation,
        CorrectionType::ReorderNodes,
        CorrectionType::AddFallback,
        CorrectionType::SplitNode,
    ];
    // 8 distinct variants
    for (i, a) in variants.iter().enumerate() {
        for (j, b) in variants.iter().enumerate() {
            if i == j {
                assert_eq!(a, b);
            } else {
                assert_ne!(a, b);
            }
        }
    }
}

#[test]
fn correction_type_serde() {
    for v in [
        CorrectionType::IncreaseFuel,
        CorrectionType::AddRetry,
        CorrectionType::ModifyLogic,
        CorrectionType::ChangeTool,
    ] {
        let j = serde_json::to_string(&v).unwrap();
        let back: CorrectionType = serde_json::from_str(&j).unwrap();
        assert_eq!(back, v);
    }
}

#[test]
fn correction_type_clone() {
    let a = CorrectionType::SplitNode;
    let b = a;
    // rustc 1.98 clippy::clone_on_copy 修复：直接 Copy 复制
    let c = a;
    assert_eq!(b, c);
}

#[test]
fn correction_type_debug() {
    let _ = format!("{:?}", CorrectionType::AddFallback);
    let _ = format!("{:?}", CorrectionType::ReorderNodes);
}

#[test]
fn correction_priority_ordering() {
    // Derives Ord from variant declaration order: Critical < High < Medium < Low
    assert!(CorrectionPriority::Critical < CorrectionPriority::High);
    assert!(CorrectionPriority::High < CorrectionPriority::Medium);
    assert!(CorrectionPriority::Medium < CorrectionPriority::Low);
    assert!(CorrectionPriority::Low > CorrectionPriority::Critical);
}

#[test]
fn correction_priority_serde() {
    for v in [
        CorrectionPriority::Critical,
        CorrectionPriority::High,
        CorrectionPriority::Medium,
        CorrectionPriority::Low,
    ] {
        let j = serde_json::to_string(&v).unwrap();
        let back: CorrectionPriority = serde_json::from_str(&j).unwrap();
        assert_eq!(back, v);
    }
}

#[test]
fn correction_priority_distinct() {
    assert_ne!(CorrectionPriority::Critical, CorrectionPriority::Low);
    assert_eq!(CorrectionPriority::High, CorrectionPriority::High);
}

#[test]
fn correction_priority_clone_copy() {
    let a = CorrectionPriority::Critical;
    let b = a; // Copy
               // rustc 1.98 clippy::clone_on_copy 修复：直接 Copy 复制
    let c = a;
    assert_eq!(a, b);
    assert_eq!(a, c);
}

#[test]
fn analysis_dimension_serde_all_variants() {
    let variants = [
        AnalysisDimension::OutputQuality,
        AnalysisDimension::ResourceEfficiency,
        AnalysisDimension::TemporalAccuracy,
        AnalysisDimension::LogicCorrectness,
        AnalysisDimension::ToolReliability,
    ];
    for v in variants {
        let j = serde_json::to_string(&v).unwrap();
        let back: AnalysisDimension = serde_json::from_str(&j).unwrap();
        assert_eq!(back, v);
    }
}

#[test]
fn analysis_dimension_hash_distinct() {
    use std::collections::HashSet;
    let mut set: HashSet<AnalysisDimension> = HashSet::new();
    set.insert(AnalysisDimension::OutputQuality);
    set.insert(AnalysisDimension::ResourceEfficiency);
    set.insert(AnalysisDimension::TemporalAccuracy);
    set.insert(AnalysisDimension::LogicCorrectness);
    set.insert(AnalysisDimension::ToolReliability);
    assert_eq!(set.len(), 5);
}
