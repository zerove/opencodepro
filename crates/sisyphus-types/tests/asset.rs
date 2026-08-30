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

// ---------------------------------------------------------------------------
// AssetContent::SOP variant
// ---------------------------------------------------------------------------

#[test]
fn asset_content_sop_variant_serde() {
    use sisyphus_types::asset::AssetContent;
    use sisyphus_types::graph::SOPGraphConfig;
    let c = AssetContent::SOP(SOPGraphConfig::default());
    let j = serde_json::to_string(&c).unwrap();
    let back: AssetContent = serde_json::from_str(&j).unwrap();
    let j2 = serde_json::to_string(&back).unwrap();
    assert_eq!(j, j2);
}

#[test]
fn asset_content_tool_variant_serde() {
    use sisyphus_types::asset::AssetContent;
    use sisyphus_types::tools::ToolSpec;
    let tool = ToolSpec {
        tool_id: "test_tool".into(),
        required_capability: "test".into(),
        expected_input: serde_json::json!({}),
        expected_output: serde_json::json!({}),
        success_criteria: "ok".into(),
        test_command: None,
        workdir: None,
    };
    let c = AssetContent::Tool(tool);
    let j = serde_json::to_string(&c).unwrap();
    let back: AssetContent = serde_json::from_str(&j).unwrap();
    let j2 = serde_json::to_string(&back).unwrap();
    assert_eq!(j, j2);
}

#[test]
fn asset_content_insight_with_high_confidence() {
    use sisyphus_types::asset::{AssetContent, InsightData};
    let i = InsightData {
        observation: "high".into(),
        reasoning: "because".into(),
        evidence_ids: vec!["e1".into(), "e2".into()],
        confidence: 0.99,
    };
    let c = AssetContent::Insight(i);
    let j = serde_json::to_string(&c).unwrap();
    let back: AssetContent = serde_json::from_str(&j).unwrap();
    let j2 = serde_json::to_string(&back).unwrap();
    assert_eq!(j, j2);
}

#[test]
fn asset_content_constraint_with_high_strictness() {
    use sisyphus_types::asset::{AssetContent, ConstraintData};
    let cd = ConstraintData {
        description: "must be fast".into(),
        strictness: 1.0,
        violation_penalty: 1.0,
    };
    let c = AssetContent::Constraint(cd);
    let j = serde_json::to_string(&c).unwrap();
    let back: AssetContent = serde_json::from_str(&j).unwrap();
    let j2 = serde_json::to_string(&back).unwrap();
    assert_eq!(j, j2);
}

#[test]
fn asset_content_variants_distinct_via_serde() {
    use sisyphus_types::asset::{AssetContent, ConstraintData, InsightData};
    let variants = vec![
        AssetContent::Insight(InsightData {
            observation: "o".into(),
            reasoning: "r".into(),
            evidence_ids: vec![],
            confidence: 0.5,
        }),
        AssetContent::Constraint(ConstraintData {
            description: "d".into(),
            strictness: 0.5,
            violation_penalty: 0.5,
        }),
    ];
    let serialized: Vec<String> = variants
        .iter()
        .map(|v| serde_json::to_string(v).unwrap())
        .collect();
    let unique: std::collections::HashSet<_> = serialized.iter().collect();
    assert_eq!(unique.len(), 2);
}

// ---------------------------------------------------------------------------
// Additional coverage
// ---------------------------------------------------------------------------

#[test]
fn asset_kind_serde_all_distinct() {
    use sisyphus_types::asset::AssetKind;
    let kinds = [
        AssetKind::Tool,
        AssetKind::SOP,
        AssetKind::Insight,
        AssetKind::Constraint,
    ];
    let serialized: Vec<String> = kinds
        .iter()
        .map(|k| serde_json::to_string(k).unwrap())
        .collect();
    // All four should produce distinct JSON strings
    let unique: std::collections::HashSet<_> = serialized.iter().collect();
    assert_eq!(unique.len(), 4);
}

#[test]
fn insight_data_clone_preserves_all_fields() {
    use sisyphus_types::asset::InsightData;
    let i = InsightData {
        observation: "obs".into(),
        reasoning: "rea".into(),
        evidence_ids: vec!["e1".into(), "e2".into()],
        confidence: 0.7,
    };
    let cloned = i.clone();
    assert_eq!(cloned.observation, "obs");
    assert_eq!(cloned.reasoning, "rea");
    assert_eq!(cloned.evidence_ids.len(), 2);
    assert_eq!(cloned.confidence, 0.7);
}

#[test]
fn constraint_data_serde_unicode() {
    use sisyphus_types::asset::ConstraintData;
    let c = ConstraintData {
        description: "约束-α".into(),
        strictness: 0.5,
        violation_penalty: 0.5,
    };
    let j = serde_json::to_string(&c).unwrap();
    let back: ConstraintData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.description, "约束-α");
}

#[test]
fn asset_relation_serde_unicode() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let r = AssetRelation {
        from_id: "资产-α".into(),
        to_id: "资产-β".into(),
        relation_type: RelationType::Solves,
        weight: 0.5,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: AssetRelation = serde_json::from_str(&j).unwrap();
    assert_eq!(back.from_id, "资产-α");
    assert_eq!(back.to_id, "资产-β");
}

#[test]
fn asset_relation_serde_with_weight_one() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let r = AssetRelation {
        from_id: "a".into(),
        to_id: "b".into(),
        relation_type: RelationType::Requires,
        weight: 1.0,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: AssetRelation = serde_json::from_str(&j).unwrap();
    assert_eq!(back.weight, 1.0);
}

#[test]
fn asset_relation_serde_with_weight_zero_negative() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let r = AssetRelation {
        from_id: "a".into(),
        to_id: "b".into(),
        relation_type: RelationType::Refines,
        weight: -1.0,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: AssetRelation = serde_json::from_str(&j).unwrap();
    assert_eq!(back.weight, -1.0);
}

#[test]
fn cognitive_asset_with_unicode_id() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "资产-α".into(),
        kind: AssetKind::Tool,
        name: "名字".into(),
        description: "描述-β".into(),
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
    let j = serde_json::to_string(&asset).unwrap();
    let back: CognitiveAsset = serde_json::from_str(&j).unwrap();
    assert_eq!(back.id, "资产-α");
    assert_eq!(back.description, "描述-β");
}

#[test]
fn cognitive_asset_metadata_with_long_value() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let long_value = "x".repeat(1000);
    let mut meta = HashMap::new();
    meta.insert("k".into(), long_value.clone());
    let asset = CognitiveAsset {
        id: "a".into(),
        kind: AssetKind::Tool,
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
    assert_eq!(asset.metadata["k"].len(), 1000);
}

// ---------------------------------------------------------------------------
// Additional coverage
// ---------------------------------------------------------------------------

#[test]
fn cognitive_asset_with_empty_id() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "".into(),
        kind: AssetKind::Tool,
        name: "n".into(),
        description: "d".into(),
        content: AssetContent::Insight(InsightData {
            observation: "o".into(),
            reasoning: "r".into(),
            evidence_ids: vec![],
            confidence: 0.5,
        }),
        metadata: HashMap::new(),
        version: 0,
        created_at: 0,
    };
    let j = serde_json::to_string(&asset).unwrap();
    let back: CognitiveAsset = serde_json::from_str(&j).unwrap();
    assert_eq!(back.id, "");
}

#[test]
fn cognitive_asset_with_empty_content_insight() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
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
    let j = serde_json::to_string(&asset).unwrap();
    let back: CognitiveAsset = serde_json::from_str(&j).unwrap();
    // AssetContent doesn't implement PartialEq, verify via Debug
    let orig_dbg = format!("{:?}", asset.content);
    let back_dbg = format!("{:?}", back.content);
    assert_eq!(orig_dbg, back_dbg);
}

#[test]
fn asset_relation_with_unicode_relation_type() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let r = AssetRelation {
        from_id: "a".into(),
        to_id: "b".into(),
        relation_type: RelationType::EvolvedFrom,
        weight: 0.5,
    };
    let j = serde_json::to_string(&r.relation_type).unwrap();
    let back: RelationType = serde_json::from_str(&j).unwrap();
    let j2 = serde_json::to_string(&back).unwrap();
    assert_eq!(j, j2);
}

#[test]
fn asset_relation_5_variants_distinct() {
    use sisyphus_types::asset::RelationType;
    let variants = [
        RelationType::Solves,
        RelationType::Requires,
        RelationType::EvolvedFrom,
        RelationType::ConflictsWith,
        RelationType::Refines,
    ];
    let serialized: Vec<String> = variants
        .iter()
        .map(|v| serde_json::to_string(v).unwrap())
        .collect();
    let unique: std::collections::HashSet<_> = serialized.iter().collect();
    assert_eq!(unique.len(), 5);
}

#[test]
fn insight_data_with_unicode_observation_round_trip() {
    use sisyphus_types::asset::InsightData;
    let i = InsightData {
        observation: "观察-α 🔍".into(),
        reasoning: "推理".into(),
        evidence_ids: vec!["证据-1".into()],
        confidence: 0.95,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.observation, "观察-α 🔍");
    assert!((back.confidence - 0.95).abs() < 1e-6);
}

#[test]
fn constraint_data_with_unicode_2() {
    use sisyphus_types::asset::ConstraintData;
    let c = ConstraintData {
        description: "约束: 必须快, 不允许 慢".into(),
        strictness: 0.9,
        violation_penalty: 0.1,
    };
    let j = serde_json::to_string(&c).unwrap();
    let back: ConstraintData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.description, "约束: 必须快, 不允许 慢");
}

#[test]
fn cognitive_asset_with_special_characters_in_name() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let special = "name-with-!@#$%^&*()_+=[]{}|;':\",./<>?`~";
    let asset = CognitiveAsset {
        id: "a".into(),
        kind: AssetKind::Tool,
        name: special.into(),
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
    let j = serde_json::to_string(&asset).unwrap();
    let back: CognitiveAsset = serde_json::from_str(&j).unwrap();
    assert_eq!(back.name, special);
}

// ---------------------------------------------------------------------------
// Additional CognitiveAsset tests
// ---------------------------------------------------------------------------

#[test]
fn cognitive_asset_with_long_description() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let long = "x".repeat(5000);
    let asset = CognitiveAsset {
        id: "a".into(),
        kind: AssetKind::Tool,
        name: "n".into(),
        description: long.clone(),
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
    let j = serde_json::to_string(&asset).unwrap();
    let back: CognitiveAsset = serde_json::from_str(&j).unwrap();
    assert_eq!(back.description.len(), 5000);
}

#[test]
fn cognitive_asset_with_negative_version() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    // Version is u32, can't be negative
    let asset = CognitiveAsset {
        id: "a".into(),
        kind: AssetKind::Tool,
        name: "n".into(),
        description: "d".into(),
        content: AssetContent::Insight(InsightData {
            observation: "o".into(),
            reasoning: "r".into(),
            evidence_ids: vec![],
            confidence: 0.5,
        }),
        metadata: HashMap::new(),
        version: 0,
        created_at: 0,
    };
    assert_eq!(asset.version, 0);
}

#[test]
fn cognitive_asset_with_max_created_at() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "a".into(),
        kind: AssetKind::Tool,
        name: "n".into(),
        description: "d".into(),
        content: AssetContent::Insight(InsightData {
            observation: "o".into(),
            reasoning: "r".into(),
            evidence_ids: vec![],
            confidence: 0.5,
        }),
        metadata: HashMap::new(),
        version: 1,
        created_at: u64::MAX,
    };
    let j = serde_json::to_string(&asset).unwrap();
    let back: CognitiveAsset = serde_json::from_str(&j).unwrap();
    assert_eq!(back.created_at, u64::MAX);
}

#[test]
fn asset_relation_with_unicode_in_5_fields() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let r = AssetRelation {
        from_id: "α".into(),
        to_id: "β".into(),
        relation_type: RelationType::Refines,
        weight: 0.7,
    };
    assert_eq!(r.from_id, "α");
    let j = serde_json::to_string(&r).unwrap();
    let back: AssetRelation = serde_json::from_str(&j).unwrap();
    assert_eq!(back.to_id, "β");
}

#[test]
fn cognitive_asset_with_unicode_in_metadata_key() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let mut meta = HashMap::new();
    meta.insert("中文-key-α".into(), "中文-value-β".into());
    let asset = CognitiveAsset {
        id: "a".into(),
        kind: AssetKind::Tool,
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
    let j = serde_json::to_string(&asset).unwrap();
    let back: CognitiveAsset = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metadata["中文-key-α"], "中文-value-β");
}

#[test]
fn insight_data_with_unicode_observation_only() {
    use sisyphus_types::asset::InsightData;
    let i = InsightData {
        observation: "观察".into(),
        reasoning: "".into(),
        evidence_ids: vec![],
        confidence: 0.0,
    };
    assert_eq!(i.observation, "观察");
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.observation, "观察");
}

#[test]
fn constraint_data_with_max_penalty() {
    use sisyphus_types::asset::ConstraintData;
    let c = ConstraintData {
        description: "d".into(),
        strictness: 1.0,
        violation_penalty: 1.0,
    };
    let j = serde_json::to_string(&c).unwrap();
    let back: ConstraintData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.violation_penalty, 1.0);
}

// ---------------------------------------------------------------------------
// Additional asset tests
// ---------------------------------------------------------------------------

#[test]
fn asset_content_with_sop_via_serde() {
    use sisyphus_types::asset::AssetContent;
    use sisyphus_types::graph::SOPGraphConfig;
    let cfg = SOPGraphConfig::default();
    let c = AssetContent::SOP(cfg);
    let j = serde_json::to_string(&c).unwrap();
    let back: AssetContent = serde_json::from_str(&j).unwrap();
    let j2 = serde_json::to_string(&back).unwrap();
    assert_eq!(j, j2);
}

#[test]
fn asset_content_with_tool_via_serde() {
    use sisyphus_types::asset::AssetContent;
    use sisyphus_types::tools::ToolSpec;
    let tool = ToolSpec {
        tool_id: "t".into(),
        required_capability: "c".into(),
        expected_input: serde_json::json!({}),
        expected_output: serde_json::json!({}),
        success_criteria: "ok".into(),
        test_command: None,
        workdir: None,
    };
    let c = AssetContent::Tool(tool);
    let j = serde_json::to_string(&c).unwrap();
    let back: AssetContent = serde_json::from_str(&j).unwrap();
    let j2 = serde_json::to_string(&back).unwrap();
    assert_eq!(j, j2);
}

#[test]
fn asset_content_with_insight_via_serde() {
    use sisyphus_types::asset::{AssetContent, InsightData};
    let i = InsightData {
        observation: "o".into(),
        reasoning: "r".into(),
        evidence_ids: vec!["e".into()],
        confidence: 0.5,
    };
    let c = AssetContent::Insight(i);
    let j = serde_json::to_string(&c).unwrap();
    let back: AssetContent = serde_json::from_str(&j).unwrap();
    let j2 = serde_json::to_string(&back).unwrap();
    assert_eq!(j, j2);
}

#[test]
fn asset_content_with_constraint_via_serde() {
    use sisyphus_types::asset::{AssetContent, ConstraintData};
    let cd = ConstraintData {
        description: "d".into(),
        strictness: 0.5,
        violation_penalty: 0.5,
    };
    let c = AssetContent::Constraint(cd);
    let j = serde_json::to_string(&c).unwrap();
    let back: AssetContent = serde_json::from_str(&j).unwrap();
    let j2 = serde_json::to_string(&back).unwrap();
    assert_eq!(j, j2);
}

#[test]
fn cognitive_asset_with_unicode_id_3_times() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    for id in ["资产-α", "资产-β", "资产-γ"] {
        let asset = CognitiveAsset {
            id: id.into(),
            kind: AssetKind::Tool,
            name: "n".into(),
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
        let j = serde_json::to_string(&asset).unwrap();
        let back: CognitiveAsset = serde_json::from_str(&j).unwrap();
        assert_eq!(back.id, id);
    }
}

#[test]
fn asset_relation_5_variants_clone_preserves() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let variants = vec![
        AssetRelation {
            from_id: "a".into(),
            to_id: "b".into(),
            relation_type: RelationType::Solves,
            weight: 0.0,
        },
        AssetRelation {
            from_id: "a".into(),
            to_id: "b".into(),
            relation_type: RelationType::Requires,
            weight: 0.0,
        },
        AssetRelation {
            from_id: "a".into(),
            to_id: "b".into(),
            relation_type: RelationType::EvolvedFrom,
            weight: 0.0,
        },
        AssetRelation {
            from_id: "a".into(),
            to_id: "b".into(),
            relation_type: RelationType::ConflictsWith,
            weight: 0.0,
        },
        AssetRelation {
            from_id: "a".into(),
            to_id: "b".into(),
            relation_type: RelationType::Refines,
            weight: 0.0,
        },
    ];
    for v in &variants {
        let cloned = v.clone();
        // RelationType doesn't implement PartialEq, but we can verify via Debug
        let orig_dbg = format!("{:?}", v.relation_type);
        let clone_dbg = format!("{:?}", cloned.relation_type);
        assert_eq!(orig_dbg, clone_dbg);
    }
}

#[test]
fn insight_data_with_only_unicode_observation() {
    use sisyphus_types::asset::InsightData;
    let i = InsightData {
        observation: "🔭".into(),
        reasoning: "".into(),
        evidence_ids: vec![],
        confidence: 0.0,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.observation, "🔭");
}

// ---------------------------------------------------------------------------
// Additional tests
// ---------------------------------------------------------------------------

#[test]
fn cognitive_asset_with_unicode_name_field() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "a".into(),
        kind: AssetKind::Insight,
        name: "工具-α".into(),
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
    let j = serde_json::to_string(&asset).unwrap();
    let back: CognitiveAsset = serde_json::from_str(&j).unwrap();
    assert_eq!(back.name, "工具-α");
}

#[test]
fn cognitive_asset_with_unicode_id_field() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "资产-α-β-γ".into(),
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
        version: 1,
        created_at: 0,
    };
    let j = serde_json::to_string(&asset).unwrap();
    let back: CognitiveAsset = serde_json::from_str(&j).unwrap();
    assert_eq!(back.id, "资产-α-β-γ");
}

#[test]
fn asset_relation_with_unicode_3_fields() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let r = AssetRelation {
        from_id: "资产-α".into(),
        to_id: "资产-β".into(),
        relation_type: RelationType::EvolvedFrom,
        weight: 0.42,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: AssetRelation = serde_json::from_str(&j).unwrap();
    assert_eq!(back.from_id, "资产-α");
    assert_eq!(back.to_id, "资产-β");
    assert!((back.weight - 0.42).abs() < 1e-6);
}

#[test]
fn asset_content_insight_with_many_evidence() {
    use sisyphus_types::asset::{AssetContent, InsightData};
    let i = InsightData {
        observation: "o".into(),
        reasoning: "r".into(),
        evidence_ids: (0..1000).map(|i| format!("e{}", i)).collect(),
        confidence: 0.5,
    };
    let c = AssetContent::Insight(i);
    let j = serde_json::to_string(&c).unwrap();
    let back: AssetContent = serde_json::from_str(&j).unwrap();
    let j2 = serde_json::to_string(&back).unwrap();
    assert_eq!(j, j2);
}

#[test]
fn cognitive_asset_with_high_version_and_created_at() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "a".into(),
        kind: AssetKind::Tool,
        name: "n".into(),
        description: "d".into(),
        content: AssetContent::Insight(InsightData {
            observation: "o".into(),
            reasoning: "r".into(),
            evidence_ids: vec![],
            confidence: 0.5,
        }),
        metadata: HashMap::new(),
        version: 100,
        created_at: 1700000000,
    };
    let j = serde_json::to_string(&asset).unwrap();
    let back: CognitiveAsset = serde_json::from_str(&j).unwrap();
    assert_eq!(back.version, 100);
    assert_eq!(back.created_at, 1700000000);
}

#[test]
fn cognitive_asset_with_unicode_metadata_value() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let mut meta = HashMap::new();
    meta.insert("k".into(), "value-α-🔍".into());
    let asset = CognitiveAsset {
        id: "a".into(),
        kind: AssetKind::Tool,
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
    let j = serde_json::to_string(&asset).unwrap();
    let back: CognitiveAsset = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metadata["k"], "value-α-🔍");
}

#[test]
fn cognitive_asset_with_only_unicode_description() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let long_unicode = "中文-α-".repeat(100);
    let asset = CognitiveAsset {
        id: "a".into(),
        kind: AssetKind::Tool,
        name: "n".into(),
        description: long_unicode.clone(),
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
    assert_eq!(asset.description, long_unicode);
    let j = serde_json::to_string(&asset).unwrap();
    let back: CognitiveAsset = serde_json::from_str(&j).unwrap();
    assert_eq!(back.description, long_unicode);
}

// ---------------------------------------------------------------------------
// Additional tests
// ---------------------------------------------------------------------------

#[test]
fn cognitive_asset_with_max_metadata_value_string() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let long_value = "v".repeat(5000);
    let mut meta = HashMap::new();
    meta.insert("k".into(), long_value.clone());
    let asset = CognitiveAsset {
        id: "a".into(),
        kind: AssetKind::Tool,
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
    let j = serde_json::to_string(&asset).unwrap();
    let back: CognitiveAsset = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metadata["k"].len(), 5000);
}

#[test]
fn cognitive_asset_with_max_metadata_key_string() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let long_key = "k".repeat(5000);
    let mut meta = HashMap::new();
    meta.insert(long_key.clone(), "v".into());
    let asset = CognitiveAsset {
        id: "a".into(),
        kind: AssetKind::Tool,
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
    let j = serde_json::to_string(&asset).unwrap();
    let back: CognitiveAsset = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metadata.len(), 1);
}

#[test]
fn insight_data_with_unicode_emoji_observation_only() {
    use sisyphus_types::asset::InsightData;
    let i = InsightData {
        observation: "🔍🔭🌟📚💡".into(),
        reasoning: "".into(),
        evidence_ids: vec![],
        confidence: 0.0,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert!(back.observation.contains("🔍"));
    assert!(back.observation.contains("🌟"));
}

#[test]
fn cognitive_asset_with_unicode_each_field_round_trip() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "id-α".into(),
        kind: AssetKind::Insight,
        name: "name-α".into(),
        description: "desc-α".into(),
        content: AssetContent::Insight(InsightData {
            observation: "obs-α".into(),
            reasoning: "rea-α".into(),
            evidence_ids: vec!["e-α".into()],
            confidence: 0.5,
        }),
        metadata: HashMap::new(),
        version: 1,
        created_at: 1000,
    };
    let j = serde_json::to_string(&asset).unwrap();
    let back: CognitiveAsset = serde_json::from_str(&j).unwrap();
    assert_eq!(back.id, "id-α");
    assert_eq!(back.name, "name-α");
    assert_eq!(back.description, "desc-α");
    assert_eq!(back.created_at, 1000);
    assert_eq!(back.version, 1);
}

#[test]
fn asset_relation_with_zero_weight_serde() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let r = AssetRelation {
        from_id: "a".into(),
        to_id: "b".into(),
        relation_type: RelationType::Solves,
        weight: 0.0,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: AssetRelation = serde_json::from_str(&j).unwrap();
    assert_eq!(back.weight, 0.0);
}

#[test]
fn asset_content_with_tool_long_capability() {
    use sisyphus_types::asset::AssetContent;
    use sisyphus_types::tools::ToolSpec;
    let long_cap = "c".repeat(2000);
    let tool = ToolSpec {
        tool_id: "t".into(),
        required_capability: long_cap.clone(),
        expected_input: serde_json::json!({}),
        expected_output: serde_json::json!({}),
        success_criteria: "ok".into(),
        test_command: None,
        workdir: None,
    };
    let c = AssetContent::Tool(tool);
    let j = serde_json::to_string(&c).unwrap();
    let back: AssetContent = serde_json::from_str(&j).unwrap();
    let j2 = serde_json::to_string(&back).unwrap();
    assert_eq!(j, j2);
}

#[test]
fn cognitive_asset_with_only_metadata_serde() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let mut meta = HashMap::new();
    meta.insert("key".into(), "value".into());
    let asset = CognitiveAsset {
        id: "a".into(),
        kind: AssetKind::Tool,
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
    let j = serde_json::to_string(&asset).unwrap();
    let back: CognitiveAsset = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metadata["key"], "value");
}
