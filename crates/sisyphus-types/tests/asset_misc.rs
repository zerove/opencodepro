//! Integration tests for sisyphus-types `asset` module's
//! pure data types: `AssetKind`, `AssetContent`, `InsightData`,
//! `CognitiveAsset`.

use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
use sisyphus_types::graph::SOPGraphConfig;
use sisyphus_types::tools::ToolSpec;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// AssetKind
// ---------------------------------------------------------------------------

#[test]
fn asset_kind_variants_distinct() {
    let variants = vec![
        AssetKind::Tool,
        AssetKind::SOP,
        AssetKind::Insight,
        AssetKind::Constraint,
    ];
    let mut seen = std::collections::HashSet::new();
    for v in &variants {
        let s = format!("{v:?}");
        assert!(seen.insert(s));
    }
    assert_eq!(seen.len(), 4);
}

#[test]
fn asset_kind_serde() {
    let kinds = [
        AssetKind::Tool,
        AssetKind::SOP,
        AssetKind::Insight,
        AssetKind::Constraint,
    ];
    for k in &kinds {
        let j = serde_json::to_string(k).unwrap();
        let back: AssetKind = serde_json::from_str(&j).unwrap();
        // Compare via Debug since AssetKind doesn't derive PartialEq
        assert_eq!(format!("{back:?}"), format!("{k:?}"));
    }
}

#[test]
fn asset_kind_clone() {
    let a = AssetKind::Insight;
    let b = a.clone();
    assert_eq!(format!("{a:?}"), format!("{b:?}"));
}

// ---------------------------------------------------------------------------
// InsightData
// ---------------------------------------------------------------------------

#[test]
fn insight_data_field_access() {
    let i = InsightData {
        observation: "Pattern observed".into(),
        reasoning: "Because of X".into(),
        evidence_ids: vec!["ev1".into(), "ev2".into()],
        confidence: 0.85,
    };
    assert_eq!(i.observation, "Pattern observed");
    assert_eq!(i.reasoning, "Because of X");
    assert_eq!(i.evidence_ids.len(), 2);
    assert!((i.confidence - 0.85).abs() < 1e-6);
}

#[test]
fn insight_data_no_evidence() {
    let i = InsightData {
        observation: "x".into(),
        reasoning: "y".into(),
        evidence_ids: vec![],
        confidence: 0.0,
    };
    assert!(i.evidence_ids.is_empty());
    assert_eq!(i.confidence, 0.0);
}

#[test]
fn insight_data_clone() {
    let a = InsightData {
        observation: "o".into(),
        reasoning: "r".into(),
        evidence_ids: vec!["e".into()],
        confidence: 0.5,
    };
    let b = a.clone();
    assert_eq!(a.observation, b.observation);
    assert_eq!(a.evidence_ids, b.evidence_ids);
}

#[test]
fn insight_data_serde() {
    let i = InsightData {
        observation: "obs".into(),
        reasoning: "reason".into(),
        evidence_ids: vec!["a".into()],
        confidence: 0.7,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.observation, "obs");
    assert_eq!(back.confidence, 0.7);
}

// ---------------------------------------------------------------------------
// AssetContent
// ---------------------------------------------------------------------------

#[test]
fn asset_content_tool_variant() {
    let c = AssetContent::Tool(ToolSpec {
        tool_id: "my_tool".into(),
        required_capability: "file_read".into(),
        expected_input: serde_json::json!({}),
        expected_output: serde_json::json!({}),
        success_criteria: "x".into(),
        test_command: None,
        workdir: None,
    });
    match c {
        AssetContent::Tool(t) => assert_eq!(t.tool_id, "my_tool"),
        _ => panic!("expected Tool"),
    }
}

#[test]
fn asset_content_sop_variant() {
    let c = AssetContent::SOP(SOPGraphConfig::default());
    match c {
        AssetContent::SOP(_) => {}
        _ => panic!("expected SOP"),
    }
}

#[test]
fn asset_content_insight_variant() {
    let c = AssetContent::Insight(InsightData {
        observation: "o".into(),
        reasoning: "r".into(),
        evidence_ids: vec![],
        confidence: 0.5,
    });
    match c {
        AssetContent::Insight(i) => assert_eq!(i.observation, "o"),
        _ => panic!("expected Insight"),
    }
}

#[test]
fn asset_content_constraint_variant() {
    let c = AssetContent::Constraint(sisyphus_types::asset::ConstraintData {
        description: "d".into(),
        strictness: 0.8,
        violation_penalty: 100.0,
    });
    match c {
        AssetContent::Constraint(c) => {
            assert_eq!(c.description, "d");
            assert_eq!(c.strictness, 0.8);
        }
        _ => panic!("expected Constraint"),
    }
}

#[test]
fn asset_content_variants_distinct() {
    let v = vec![
        AssetContent::Insight(InsightData {
            observation: "".into(),
            reasoning: "".into(),
            evidence_ids: vec![],
            confidence: 0.0,
        }),
        AssetContent::SOP(SOPGraphConfig::default()),
    ];
    let mut seen = std::collections::HashSet::new();
    for c in &v {
        let s = format!("{c:?}");
        assert!(seen.insert(s));
    }
}

// ---------------------------------------------------------------------------
// CognitiveAsset
// ---------------------------------------------------------------------------

#[test]
fn cognitive_asset_field_access() {
    let a = CognitiveAsset {
        id: "asset1".into(),
        kind: AssetKind::Insight,
        name: "My Insight".into(),
        description: "An important observation".into(),
        content: AssetContent::Insight(InsightData {
            observation: "o".into(),
            reasoning: "r".into(),
            evidence_ids: vec![],
            confidence: 0.9,
        }),
        metadata: HashMap::new(),
        version: 1,
        created_at: 1234,
    };
    assert_eq!(a.id, "asset1");
    assert_eq!(a.name, "My Insight");
    assert_eq!(a.version, 1);
    assert_eq!(a.created_at, 1234);
}

#[test]
fn cognitive_asset_with_metadata() {
    let mut metadata = HashMap::new();
    metadata.insert("author".into(), "alice".into());
    metadata.insert("tag".into(), "test".into());
    let a = CognitiveAsset {
        id: "x".into(),
        kind: AssetKind::Tool,
        name: "n".into(),
        description: "d".into(),
        content: AssetContent::SOP(SOPGraphConfig::default()),
        metadata: metadata.clone(),
        version: 1,
        created_at: 0,
    };
    assert_eq!(a.metadata.get("author").unwrap(), "alice");
    assert_eq!(a.metadata.len(), 2);
}

#[test]
fn cognitive_asset_clone() {
    let a = CognitiveAsset {
        id: "x".into(),
        kind: AssetKind::SOP,
        name: "n".into(),
        description: "d".into(),
        content: AssetContent::SOP(SOPGraphConfig::default()),
        metadata: HashMap::new(),
        version: 5,
        created_at: 1000,
    };
    let b = a.clone();
    assert_eq!(a.id, b.id);
    assert_eq!(a.version, b.version);
}

#[test]
fn cognitive_asset_serde_round_trip() {
    let a = CognitiveAsset {
        id: "x".into(),
        kind: AssetKind::Insight,
        name: "n".into(),
        description: "d".into(),
        content: AssetContent::Insight(InsightData {
            observation: "o".into(),
            reasoning: "r".into(),
            evidence_ids: vec!["ev".into()],
            confidence: 0.5,
        }),
        metadata: HashMap::new(),
        version: 1,
        created_at: 100,
    };
    let j = serde_json::to_string(&a).unwrap();
    let back: CognitiveAsset = serde_json::from_str(&j).unwrap();
    assert_eq!(back.id, "x");
    assert_eq!(back.version, 1);
}

// ---------------------------------------------------------------------------
// ConstraintData additional coverage
// ---------------------------------------------------------------------------

#[test]
fn constraint_data_field_access() {
    let c = sisyphus_types::asset::ConstraintData {
        description: "must be fast".into(),
        strictness: 0.8,
        violation_penalty: 0.5,
    };
    assert_eq!(c.description, "must be fast");
    assert_eq!(c.strictness, 0.8);
}

#[test]
fn constraint_data_serde() {
    let c = sisyphus_types::asset::ConstraintData {
        description: "d".into(),
        strictness: 0.0,
        violation_penalty: 1.0,
    };
    let j = serde_json::to_string(&c).unwrap();
    let back: sisyphus_types::asset::ConstraintData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.violation_penalty, 1.0);
}

#[test]
fn constraint_data_clone() {
    let c = sisyphus_types::asset::ConstraintData {
        description: "d".into(),
        strictness: 0.5,
        violation_penalty: 0.5,
    };
    let cloned = c.clone();
    assert_eq!(cloned.strictness, c.strictness);
}

#[test]
fn relation_type_serde() {
    use sisyphus_types::asset::RelationType;
    for v in [
        RelationType::Solves,
        RelationType::Requires,
        RelationType::EvolvedFrom,
        RelationType::ConflictsWith,
        RelationType::Refines,
    ] {
        let j = serde_json::to_string(&v).unwrap();
        let back: RelationType = serde_json::from_str(&j).unwrap();
        // RelationType doesn't implement PartialEq, but we can verify it round-trips
        // by re-serializing and comparing strings
        let back_j = serde_json::to_string(&back).unwrap();
        assert_eq!(back_j, j);
    }
}

#[test]
fn asset_relation_serde() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let r = AssetRelation {
        from_id: "a".into(),
        to_id: "b".into(),
        relation_type: RelationType::Solves,
        weight: 0.5,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: AssetRelation = serde_json::from_str(&j).unwrap();
    assert_eq!(back.from_id, "a");
    // Verify relation_type round-trips via Debug equality on string repr
    let j2 = serde_json::to_string(&back.relation_type).unwrap();
    let orig = serde_json::to_string(&RelationType::Solves).unwrap();
    assert_eq!(j2, orig);
}

#[test]
fn asset_relation_with_zero_weight() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let r = AssetRelation {
        from_id: "a".into(),
        to_id: "b".into(),
        relation_type: RelationType::Requires,
        weight: 0.0,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: AssetRelation = serde_json::from_str(&j).unwrap();
    assert_eq!(back.weight, 0.0);
}

#[test]
fn asset_relation_unicode() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let r = AssetRelation {
        from_id: "资产-α".into(),
        to_id: "资产-β".into(),
        relation_type: RelationType::Refines,
        weight: 1.0,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: AssetRelation = serde_json::from_str(&j).unwrap();
    assert_eq!(back.from_id, "资产-α");
}

#[test]
fn insight_data_with_unicode_evidence() {
    let i = InsightData {
        observation: "观察-α".into(),
        reasoning: "推理-β".into(),
        evidence_ids: vec!["证据-1".into(), "证据-2".into()],
        confidence: 0.95,
    };
    assert_eq!(i.evidence_ids.len(), 2);
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.evidence_ids[0], "证据-1");
}

#[test]
fn insight_data_with_zero_confidence() {
    let i = InsightData {
        observation: "".into(),
        reasoning: "".into(),
        evidence_ids: vec![],
        confidence: 0.0,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.confidence, 0.0);
    assert!(back.evidence_ids.is_empty());
}

#[test]
fn cognitive_asset_with_max_version() {
    let asset = CognitiveAsset {
        id: "a".into(),
        kind: AssetKind::Insight,
        name: "n".into(),
        description: "d".into(),
        content: AssetContent::Insight(InsightData {
            observation: "o".into(),
            reasoning: "r".into(),
            evidence_ids: vec![],
            confidence: 0.5,
        }),
        metadata: HashMap::new(),
        version: u32::MAX,
        created_at: 0,
    };
    assert_eq!(asset.version, u32::MAX);
}
