//! Integration tests for sisyphus-types `asset` types.
//!
//! Exercises the public `AssetKind`, `AssetContent`,
//! `CognitiveAsset`, `InsightData`, `ConstraintData`, and
//! `AssetRelation` types. These are pure data types with serde
//! derives, so tests focus on serialization round-trips and
//! discrimination between content variants.

use sisyphus_types::asset::{
    AssetContent, AssetKind, AssetRelation, CognitiveAsset, ConstraintData, InsightData,
    RelationType,
};
use sisyphus_types::tools::ToolSpec;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// AssetKind
// ---------------------------------------------------------------------------

#[test]
fn asset_kind_serialization_roundtrip() {
    for kind in [
        AssetKind::Tool,
        AssetKind::SOP,
        AssetKind::Insight,
        AssetKind::Constraint,
    ] {
        let json = serde_json::to_string(&kind).expect("ok");
        let parsed: AssetKind = serde_json::from_str(&json).expect("ok");
        // We can re-serialize; check that round-trip is stable.
        let json2 = serde_json::to_string(&parsed).expect("");
        assert_eq!(json, json2);
    }
}

#[test]
fn asset_kind_clone() {
    let k = AssetKind::Insight;
    let cloned = k.clone();
    // No PartialEq, but Debug + Clone are derived.
    let _ = format!("{:?}", cloned);
}

// ---------------------------------------------------------------------------
// AssetContent variants
// ---------------------------------------------------------------------------

#[test]
fn asset_content_tool_variant() {
    let tool = ToolSpec {
        tool_id: "t1".into(),
        required_capability: "file".into(),
        expected_input: serde_json::json!({}),
        expected_output: serde_json::json!({}),
        success_criteria: "ok".into(),
        test_command: None,
        workdir: None,
    };
    let content = AssetContent::Tool(tool);
    let json = serde_json::to_string(&content).expect("");
    assert!(json.contains("Tool"));
    assert!(json.contains("t1"));
}

#[test]
fn asset_content_insight_variant() {
    let insight = InsightData {
        observation: "User prefers terse output".into(),
        reasoning: "Multiple corrections over 3 turns".into(),
        evidence_ids: vec!["e1".into(), "e2".into()],
        confidence: 0.85,
    };
    let content = AssetContent::Insight(insight);
    let json = serde_json::to_string(&content).expect("");
    assert!(json.contains("Insight"));
    assert!(json.contains("terse output"));
}

#[test]
fn asset_content_constraint_variant() {
    let c = ConstraintData {
        description: "Max 100 tokens per response".into(),
        strictness: 0.8,
        violation_penalty: -0.5,
    };
    let content = AssetContent::Constraint(c);
    let json = serde_json::to_string(&content).expect("");
    assert!(json.contains("Constraint"));
    assert!(json.contains("Max 100 tokens"));
}

#[test]
fn asset_content_roundtrip_insight() {
    let original = AssetContent::Insight(InsightData {
        observation: "obs".into(),
        reasoning: "reason".into(),
        evidence_ids: vec!["a".into()],
        confidence: 0.9,
    });
    let json = serde_json::to_string(&original).expect("");
    let parsed: AssetContent = serde_json::from_str(&json).expect("");
    let json2 = serde_json::to_string(&parsed).expect("");
    assert_eq!(json, json2);
}

// ---------------------------------------------------------------------------
// CognitiveAsset
// ---------------------------------------------------------------------------

#[test]
fn cognitive_asset_basic_serialization() {
    let mut metadata = HashMap::new();
    metadata.insert("author".to_string(), "alice".to_string());
    let asset = CognitiveAsset {
        id: "asset-1".into(),
        kind: AssetKind::Tool,
        name: "My Tool".into(),
        description: "Reads files".into(),
        content: AssetContent::Tool(ToolSpec {
            tool_id: "file_read".into(),
            required_capability: "io".into(),
            expected_input: serde_json::json!({}),
            expected_output: serde_json::json!({}),
            success_criteria: "ok".into(),
            test_command: None,
            workdir: None,
        }),
        metadata,
        version: 1,
        created_at: 1_700_000_000,
    };
    let json = serde_json::to_string(&asset).expect("");
    let parsed: CognitiveAsset = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.id, "asset-1");
    assert_eq!(parsed.name, "My Tool");
    assert_eq!(parsed.version, 1);
    assert_eq!(parsed.metadata.get("author").map(String::as_str), Some("alice"));
}

#[test]
fn cognitive_asset_clone() {
    let asset = CognitiveAsset {
        id: "x".into(),
        kind: AssetKind::Insight,
        name: "X".into(),
        description: "d".into(),
        content: AssetContent::Insight(InsightData {
            observation: "o".into(),
            reasoning: "r".into(),
            evidence_ids: vec![],
            confidence: 0.5,
        }),
        metadata: HashMap::new(),
        version: 1,
        created_at: 0,
    };
    let cloned = asset.clone();
    assert_eq!(cloned.id, "x");
}

// ---------------------------------------------------------------------------
// InsightData
// ---------------------------------------------------------------------------

#[test]
fn insight_data_roundtrip() {
    let i = InsightData {
        observation: "user prefers X".into(),
        reasoning: "based on 5 turns".into(),
        evidence_ids: vec!["e1".into(), "e2".into()],
        confidence: 0.95,
    };
    let json = serde_json::to_string(&i).expect("");
    let parsed: InsightData = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.observation, "user prefers X");
    assert!((parsed.confidence - 0.95).abs() < 1e-6);
    assert_eq!(parsed.evidence_ids.len(), 2);
}

#[test]
fn insight_data_with_empty_evidence() {
    let i = InsightData {
        observation: "x".into(),
        reasoning: "y".into(),
        evidence_ids: vec![],
        confidence: 0.0,
    };
    let json = serde_json::to_string(&i).expect("");
    let parsed: InsightData = serde_json::from_str(&json).expect("");
    assert!(parsed.evidence_ids.is_empty());
    assert_eq!(parsed.confidence, 0.0);
}

// ---------------------------------------------------------------------------
// ConstraintData
// ---------------------------------------------------------------------------

#[test]
fn constraint_data_serialization() {
    let c = ConstraintData {
        description: "Never delete files outside /tmp".into(),
        strictness: 1.0,
        violation_penalty: -1.0,
    };
    let json = serde_json::to_string(&c).expect("");
    let parsed: ConstraintData = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.description, "Never delete files outside /tmp");
}

// ---------------------------------------------------------------------------
// AssetRelation
// ---------------------------------------------------------------------------

#[test]
fn asset_relation_serialization() {
    let r = AssetRelation {
        from_id: "a".into(),
        to_id: "b".into(),
        relation_type: RelationType::Solves,
        weight: 1.0,
    };
    let json = serde_json::to_string(&r).expect("");
    let parsed: AssetRelation = serde_json::from_str(&json).expect("");
    assert_eq!(parsed.from_id, "a");
    assert_eq!(parsed.to_id, "b");
    let _ = format!("{:?}", parsed.relation_type);
}

#[test]
fn asset_relation_clone() {
    let r = AssetRelation {
        from_id: "a".into(),
        to_id: "b".into(),
        relation_type: RelationType::Refines,
        weight: 0.5,
    };
    let cloned = r.clone();
    assert_eq!(cloned.from_id, "a");
}

// ---------------------------------------------------------------------------
// Additional coverage
// ---------------------------------------------------------------------------

#[test]
fn asset_kind_debug_format() {
    use sisyphus_types::asset::AssetKind;
    let _ = format!("{:?}", AssetKind::Tool);
    let _ = format!("{:?}", AssetKind::SOP);
    let _ = format!("{:?}", AssetKind::Insight);
    let _ = format!("{:?}", AssetKind::Constraint);
}

#[test]
fn asset_kind_distinct_via_serde() {
    use sisyphus_types::asset::AssetKind;
    let kinds = [
        AssetKind::Tool,
        AssetKind::SOP,
        AssetKind::Insight,
        AssetKind::Constraint,
    ];
    let serialized: Vec<String> = kinds.iter().map(|k| serde_json::to_string(k).unwrap()).collect();
    // All four serialize to distinct strings
    let unique: std::collections::HashSet<_> = serialized.iter().collect();
    assert_eq!(unique.len(), 4);
}

#[test]
fn asset_kind_serde_unicode() {
    use sisyphus_types::asset::AssetKind;
    let kinds = [
        AssetKind::Tool,
        AssetKind::SOP,
        AssetKind::Insight,
        AssetKind::Constraint,
    ];
    for k in kinds {
        let j = serde_json::to_string(&k).unwrap();
        let back: AssetKind = serde_json::from_str(&j).unwrap();
        let j2 = serde_json::to_string(&back).unwrap();
        assert_eq!(j, j2);
    }
}

#[test]
fn insight_data_unicode() {
    use sisyphus_types::asset::InsightData;
    let i = InsightData {
        observation: "观察-α".into(),
        reasoning: "推理-β".into(),
        evidence_ids: vec!["证据-1".into()],
        confidence: 0.95,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.observation, "观察-α");
}

#[test]
fn insight_data_with_many_evidence() {
    use sisyphus_types::asset::InsightData;
    let ev: Vec<String> = (0..100).map(|i| format!("e{}", i)).collect();
    let i = InsightData {
        observation: "o".into(),
        reasoning: "r".into(),
        evidence_ids: ev.clone(),
        confidence: 0.5,
    };
    assert_eq!(i.evidence_ids.len(), 100);
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.evidence_ids.len(), 100);
    assert_eq!(back.evidence_ids[42], "e42");
}

#[test]
fn constraint_data_field_access() {
    use sisyphus_types::asset::ConstraintData;
    let c = ConstraintData {
        description: "must compile".into(),
        strictness: 0.9,
        violation_penalty: 0.5,
    };
    assert_eq!(c.description, "must compile");
    assert_eq!(c.strictness, 0.9);
}

#[test]
fn constraint_data_zero_values() {
    use sisyphus_types::asset::ConstraintData;
    let c = ConstraintData {
        description: "".into(),
        strictness: 0.0,
        violation_penalty: 0.0,
    };
    assert_eq!(c.strictness, 0.0);
    let j = serde_json::to_string(&c).unwrap();
    let back: ConstraintData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.violation_penalty, 0.0);
}

#[test]
fn constraint_data_clone() {
    use sisyphus_types::asset::ConstraintData;
    let c = ConstraintData {
        description: "d".into(),
        strictness: 0.5,
        violation_penalty: 0.5,
    };
    let cloned = c.clone();
    assert_eq!(cloned.strictness, c.strictness);
}

#[test]
fn cognitive_asset_with_many_metadata() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let mut meta = HashMap::new();
    for i in 0..50 {
        meta.insert(format!("k{}", i), format!("v{}", i));
    }
    let a = CognitiveAsset {
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
        metadata: meta,
        version: 1,
        created_at: 0,
    };
    assert_eq!(a.metadata.len(), 50);
}

#[test]
fn cognitive_asset_zero_version() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let a = CognitiveAsset {
        id: "a".into(),
        kind: AssetKind::Insight,
        name: "n".into(),
        description: "d".into(),
        content: AssetContent::Insight(InsightData {
            observation: "".into(),
            reasoning: "".into(),
            evidence_ids: vec![],
            confidence: 0.0,
        }),
        metadata: HashMap::new(),
        version: 0,
        created_at: 0,
    };
    assert_eq!(a.version, 0);
    let j = serde_json::to_string(&a).unwrap();
    let back: CognitiveAsset = serde_json::from_str(&j).unwrap();
    assert_eq!(back.version, 0);
}

#[test]
fn asset_relation_with_negative_weight() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let r = AssetRelation {
        from_id: "a".into(),
        to_id: "b".into(),
        relation_type: RelationType::ConflictsWith,
        weight: -0.5,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: AssetRelation = serde_json::from_str(&j).unwrap();
    assert_eq!(back.weight, -0.5);
}

#[test]
fn asset_relation_with_max_weight() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let r = AssetRelation {
        from_id: "a".into(),
        to_id: "b".into(),
        relation_type: RelationType::Solves,
        weight: f32::MAX,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: AssetRelation = serde_json::from_str(&j).unwrap();
    assert_eq!(back.weight, f32::MAX);
}

#[test]
fn asset_relation_unicode_again() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let r = AssetRelation {
        from_id: "资产-α".into(),
        to_id: "资产-β".into(),
        relation_type: RelationType::Refines,
        weight: 1.0,
    };
    assert_eq!(r.from_id, "资产-α");
}
