//! Integration tests for sisyphus-types `asset` module.
//!
//! Exercises the pure-data surfaces of [`AssetKind`], [`AssetContent`],
//! [`InsightData`], [`ConstraintData`], [`AssetRelation`], and
//! [`RelationType`].

use sisyphus_types::asset::{
    AssetContent, AssetKind, AssetRelation, CognitiveAsset, ConstraintData, InsightData,
    RelationType,
};
use sisyphus_types::tools::ToolSpec;

// ---------------------------------------------------------------------------
// AssetKind
// ---------------------------------------------------------------------------

#[test]
fn asset_kind_serde_round_trip() {
    for k in [
        AssetKind::Tool,
        AssetKind::SOP,
        AssetKind::Insight,
        AssetKind::Constraint,
    ] {
        let j = serde_json::to_string(&k).unwrap();
        let back: AssetKind = serde_json::from_str(&j).unwrap();
        // AssetKind has no PartialEq; compare via Debug.
        assert_eq!(format!("{back:?}"), format!("{k:?}"));
    }
}

// ---------------------------------------------------------------------------
// InsightData
// ---------------------------------------------------------------------------

#[test]
fn insight_data_field_assignment() {
    let d = InsightData {
        observation: "x".into(),
        reasoning: "because y".into(),
        evidence_ids: vec!["e1".into(), "e2".into()],
        confidence: 0.85,
    };
    assert_eq!(d.observation, "x");
    assert_eq!(d.evidence_ids.len(), 2);
    assert!((d.confidence - 0.85).abs() < 1e-6);
}

#[test]
fn insight_data_serde_round_trip() {
    let d = InsightData {
        observation: "o".into(),
        reasoning: "r".into(),
        evidence_ids: vec![],
        confidence: 0.5,
    };
    let j = serde_json::to_string(&d).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.observation, "o");
    assert!((back.confidence - 0.5).abs() < 1e-6);
}

// ---------------------------------------------------------------------------
// ConstraintData
// ---------------------------------------------------------------------------

#[test]
fn constraint_data_field_assignment() {
    let c = ConstraintData {
        description: "must compile".into(),
        strictness: 0.9,
        violation_penalty: 10.0,
    };
    assert!((c.strictness - 0.9).abs() < 1e-6);
    assert!((c.violation_penalty - 10.0).abs() < 1e-6);
}

#[test]
fn constraint_data_serde_round_trip() {
    let c = ConstraintData {
        description: "d".into(),
        strictness: 0.1,
        violation_penalty: 1.0,
    };
    let j = serde_json::to_string(&c).unwrap();
    let back: ConstraintData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.description, "d");
}

// ---------------------------------------------------------------------------
// AssetContent
// ---------------------------------------------------------------------------

#[test]
fn asset_content_tool_variant() {
    let tool = ToolSpec {
        tool_id: "t1".into(),
        required_capability: "read".into(),
        expected_input: serde_json::json!({}),
        expected_output: serde_json::json!({}),
        success_criteria: "x".into(),
        test_command: None,
        workdir: None,
    };
    let content = AssetContent::Tool(tool.clone());
    let j = serde_json::to_string(&content).unwrap();
    let back: AssetContent = serde_json::from_str(&j).unwrap();
    match back {
        AssetContent::Tool(t) => assert_eq!(t.tool_id, "t1"),
        _ => panic!("expected Tool"),
    }
}

#[test]
fn asset_content_insight_variant() {
    let insight = InsightData {
        observation: "o".into(),
        reasoning: "r".into(),
        evidence_ids: vec![],
        confidence: 0.7,
    };
    let content = AssetContent::Insight(insight);
    let j = serde_json::to_string(&content).unwrap();
    let back: AssetContent = serde_json::from_str(&j).unwrap();
    match back {
        AssetContent::Insight(i) => assert!((i.confidence - 0.7).abs() < 1e-6),
        _ => panic!("expected Insight"),
    }
}

#[test]
fn asset_content_constraint_variant() {
    let c = ConstraintData {
        description: "x".into(),
        strictness: 0.5,
        violation_penalty: 1.0,
    };
    let content = AssetContent::Constraint(c);
    let j = serde_json::to_string(&content).unwrap();
    let back: AssetContent = serde_json::from_str(&j).unwrap();
    match back {
        AssetContent::Constraint(c) => assert_eq!(c.description, "x"),
        _ => panic!("expected Constraint"),
    }
}

// ---------------------------------------------------------------------------
// RelationType
// ---------------------------------------------------------------------------

#[test]
fn relation_type_variants_distinct() {
    let variants = [
        RelationType::Solves,
        RelationType::Requires,
        RelationType::EvolvedFrom,
        RelationType::ConflictsWith,
        RelationType::Refines,
    ];
    // Use Debug for comparison (RelationType does not derive PartialEq).
    let mut seen = std::collections::HashSet::new();
    for v in &variants {
        let s = format!("{v:?}");
        assert!(seen.insert(s.clone()), "duplicate debug string: {s}");
    }
}

#[test]
fn relation_type_serde() {
    for r in [
        RelationType::Solves,
        RelationType::Requires,
        RelationType::EvolvedFrom,
        RelationType::ConflictsWith,
        RelationType::Refines,
    ] {
        let j = serde_json::to_string(&r).unwrap();
        let back: RelationType = serde_json::from_str(&j).unwrap();
        // Compare via Debug strings.
        assert_eq!(format!("{back:?}"), format!("{r:?}"));
    }
}

// ---------------------------------------------------------------------------
// AssetRelation
// ---------------------------------------------------------------------------

#[test]
fn asset_relation_field_assignment() {
    let r = AssetRelation {
        from_id: "a1".into(),
        to_id: "a2".into(),
        relation_type: RelationType::Solves,
        weight: 0.8,
    };
    assert_eq!(r.from_id, "a1");
    assert_eq!(r.to_id, "a2");
    assert_eq!(format!("{}", format!("{:?}", r.relation_type)), "Solves");
    assert!((r.weight - 0.8).abs() < 1e-6);
}

#[test]
fn asset_relation_serde_round_trip() {
    let r = AssetRelation {
        from_id: "a".into(),
        to_id: "b".into(),
        relation_type: RelationType::Requires,
        weight: 0.5,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: AssetRelation = serde_json::from_str(&j).unwrap();
    assert_eq!(back.from_id, "a");
    assert_eq!(format!("{:?}", back.relation_type), "Requires");
}

// ---------------------------------------------------------------------------
// CognitiveAsset
// ---------------------------------------------------------------------------

#[test]
fn cognitive_asset_field_assignment() {
    use std::collections::HashMap;
    let mut meta = HashMap::new();
    meta.insert("k".to_string(), "v".to_string());
    let asset = CognitiveAsset {
        id: "asset-1".into(),
        kind: AssetKind::Insight,
        name: "n".into(),
        description: "d".into(),
        content: AssetContent::Insight(InsightData {
            observation: "o".into(),
            reasoning: "r".into(),
            evidence_ids: vec![],
            confidence: 0.9,
        }),
        metadata: meta,
        version: 1,
        created_at: 12345,
    };
    assert_eq!(asset.id, "asset-1");
    assert_eq!(asset.version, 1);
    assert_eq!(asset.created_at, 12345);
    assert_eq!(asset.metadata.get("k"), Some(&"v".to_string()));
}

#[test]
fn asset_kind_variants_distinct() {
    // AssetKind does not derive PartialEq; compare via Debug.
    let k1 = AssetKind::Tool;
    let k2 = AssetKind::SOP;
    let k3 = AssetKind::Insight;
    let k4 = AssetKind::Constraint;
    let s1 = format!("{k1:?}");
    let s2 = format!("{k2:?}");
    let s3 = format!("{k3:?}");
    let s4 = format!("{k4:?}");
    let all = vec![s1.clone(), s2.clone(), s3.clone(), s4.clone()];
    let unique: std::collections::HashSet<_> = all.into_iter().collect();
    assert_eq!(unique.len(), 4);
}
