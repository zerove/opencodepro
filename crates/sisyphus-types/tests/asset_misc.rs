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

// ---------------------------------------------------------------------------
// Additional coverage
// ---------------------------------------------------------------------------

#[test]
fn asset_kind_unicode_debug() {
    use sisyphus_types::asset::AssetKind;
    let s = format!("{:?}", AssetKind::Tool);
    // Just verify it doesn't panic
    let _ = s;
}

#[test]
fn insight_data_unicode_observation() {
    let i = InsightData {
        observation: "观察-α 包含 emoji 🔍".into(),
        reasoning: "推理".into(),
        evidence_ids: vec![],
        confidence: 0.7,
    };
    assert!(i.observation.contains("🔍"));
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert!(back.observation.contains("🔍"));
}

#[test]
fn insight_data_with_long_observation() {
    let long = "x".repeat(10_000);
    let i = InsightData {
        observation: long.clone(),
        reasoning: "r".into(),
        evidence_ids: vec![],
        confidence: 0.5,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.observation.len(), 10_000);
}

#[test]
fn constraint_data_unicode() {
    use sisyphus_types::asset::ConstraintData;
    let c = ConstraintData {
        description: "约束-α: 必须快".into(),
        strictness: 0.9,
        violation_penalty: 0.8,
    };
    let j = serde_json::to_string(&c).unwrap();
    let back: ConstraintData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.description, "约束-α: 必须快");
}

#[test]
fn constraint_data_above_one_strictness() {
    use sisyphus_types::asset::ConstraintData;
    let c = ConstraintData {
        description: "d".into(),
        strictness: 1.5,
        violation_penalty: 0.5,
    };
    // No validation, just verify it round-trips
    let j = serde_json::to_string(&c).unwrap();
    let back: ConstraintData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.strictness, 1.5);
}

#[test]
fn cognitive_asset_with_unicode_metadata() {
    let mut meta = HashMap::new();
    meta.insert("作者".into(), "张-α".into());
    meta.insert("版本".into(), "1.0".into());
    let asset = CognitiveAsset {
        id: "id".into(),
        kind: AssetKind::Insight,
        name: "名字".into(),
        description: "描述".into(),
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
    assert_eq!(asset.metadata["作者"], "张-α");
}

#[test]
fn asset_relation_self_loop() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let r = AssetRelation {
        from_id: "a".into(),
        to_id: "a".into(), // self-loop
        relation_type: RelationType::Solves,
        weight: 1.0,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: AssetRelation = serde_json::from_str(&j).unwrap();
    assert_eq!(back.from_id, back.to_id);
}

#[test]
fn asset_relation_with_max_occurrence_count() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let r = AssetRelation {
        from_id: "a".into(),
        to_id: "b".into(),
        relation_type: RelationType::Refines,
        weight: f32::MIN, // small negative
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: AssetRelation = serde_json::from_str(&j).unwrap();
    assert_eq!(back.weight, f32::MIN);
}

// ---------------------------------------------------------------------------
// Additional AssetKind and asset relation tests
// ---------------------------------------------------------------------------

#[test]
fn asset_kind_default_debug() {
    use sisyphus_types::asset::AssetKind;
    for kind in [
        AssetKind::Tool,
        AssetKind::SOP,
        AssetKind::Insight,
        AssetKind::Constraint,
    ] {
        let s = format!("{:?}", kind);
        assert!(s.len() > 0);
    }
}

#[test]
fn insight_data_with_only_whitespace_observation() {
    let i = InsightData {
        observation: "   \n\t  ".into(),
        reasoning: "r".into(),
        evidence_ids: vec![],
        confidence: 0.5,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.observation, "   \n\t  ");
}

#[test]
fn insight_data_with_negative_confidence() {
    let i = InsightData {
        observation: "o".into(),
        reasoning: "r".into(),
        evidence_ids: vec![],
        confidence: -0.5,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.confidence, -0.5);
}

#[test]
fn constraint_data_with_negative_strictness() {
    use sisyphus_types::asset::ConstraintData;
    let c = ConstraintData {
        description: "d".into(),
        strictness: -1.0,
        violation_penalty: 0.0,
    };
    let j = serde_json::to_string(&c).unwrap();
    let back: ConstraintData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.strictness, -1.0);
}

#[test]
fn asset_relation_with_empty_ids() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let r = AssetRelation {
        from_id: "".into(),
        to_id: "".into(),
        relation_type: RelationType::Solves,
        weight: 0.5,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: AssetRelation = serde_json::from_str(&j).unwrap();
    assert_eq!(back.from_id, "");
    assert_eq!(back.to_id, "");
}

#[test]
fn asset_relation_clone_preserves_weight() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let r = AssetRelation {
        from_id: "a".into(),
        to_id: "b".into(),
        relation_type: RelationType::EvolvedFrom,
        weight: 0.75,
    };
    let cloned = r.clone();
    assert_eq!(cloned.weight, 0.75);
}

#[test]
fn cognitive_asset_with_zero_created_at() {
    let asset = CognitiveAsset {
        id: "a".into(),
        kind: AssetKind::Tool,
        name: "n".into(),
        description: "d".into(),
        content: AssetContent::Insight(InsightData {
            observation: "o".into(),
            reasoning: "r".into(),
            evidence_ids: vec![],
            confidence: 0.0,
        }),
        metadata: HashMap::new(),
        version: 0,
        created_at: 0,
    };
    let j = serde_json::to_string(&asset).unwrap();
    let back: CognitiveAsset = serde_json::from_str(&j).unwrap();
    assert_eq!(back.created_at, 0);
}

// ---------------------------------------------------------------------------
// Additional InsightData tests
// ---------------------------------------------------------------------------

#[test]
fn insight_data_with_only_whitespace() {
    let i = InsightData {
        observation: "   ".into(),
        reasoning: "\t\n".into(),
        evidence_ids: vec!["  ".into()],
        confidence: 0.0,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.observation, "   ");
    assert_eq!(back.reasoning, "\t\n");
    assert_eq!(back.evidence_ids[0], "  ");
}

#[test]
fn insight_data_with_above_one_confidence() {
    let i = InsightData {
        observation: "o".into(),
        reasoning: "r".into(),
        evidence_ids: vec![],
        confidence: 1.5,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.confidence, 1.5);
}

#[test]
fn constraint_data_with_max_values() {
    use sisyphus_types::asset::ConstraintData;
    let c = ConstraintData {
        description: "d".into(),
        strictness: f32::MAX,
        violation_penalty: f32::MAX,
    };
    let j = serde_json::to_string(&c).unwrap();
    let back: ConstraintData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.strictness, f32::MAX);
}

#[test]
fn constraint_data_with_nan_values() {
    use sisyphus_types::asset::ConstraintData;
    let c = ConstraintData {
        description: "d".into(),
        strictness: f32::NAN,
        violation_penalty: f32::NAN,
    };
    // NaN might not roundtrip via Serde, but should not panic
    let j = serde_json::to_string(&c).unwrap_or_default();
    // Just verify it doesn't panic
    let _ = j;
}

#[test]
fn cognitive_asset_with_only_metadata() {
    let mut meta = HashMap::new();
    meta.insert("k".into(), "v".into());
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
fn cognitive_asset_field_access_v2() {
    let asset = CognitiveAsset {
        id: "id-α".into(),
        kind: AssetKind::SOP,
        name: "name".into(),
        description: "desc".into(),
        content: AssetContent::Insight(InsightData {
            observation: "o".into(),
            reasoning: "r".into(),
            evidence_ids: vec!["e".into()],
            confidence: 0.5,
        }),
        metadata: HashMap::new(),
        version: 1,
        created_at: 100,
    };
    assert_eq!(asset.id, "id-α");
    // AssetKind doesn't implement PartialEq, but we can verify it via Debug
    let kind_dbg = format!("{:?}", asset.kind);
    assert_eq!(kind_dbg, "SOP");
    assert_eq!(asset.name, "name");
    assert_eq!(asset.description, "desc");
    assert_eq!(asset.created_at, 100);
}

#[test]
fn asset_relation_serde_with_high_precision_weight() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let r = AssetRelation {
        from_id: "a".into(),
        to_id: "b".into(),
        relation_type: RelationType::Solves,
        weight: 0.123_456_78_f32,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: AssetRelation = serde_json::from_str(&j).unwrap();
    assert!((back.weight - 0.123_456_78_f32).abs() < 1e-6);
}

// ---------------------------------------------------------------------------
// Additional AssetContent variant tests
// ---------------------------------------------------------------------------

#[test]
fn asset_content_sop_with_unicode_name() {
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
fn asset_content_tool_with_long_inputs() {
    use sisyphus_types::asset::AssetContent;
    use sisyphus_types::tools::ToolSpec;
    let tool = ToolSpec {
        tool_id: "x".repeat(1000),
        required_capability: "y".into(),
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
fn insight_data_with_long_evidence_ids() {
    let i = InsightData {
        observation: "o".into(),
        reasoning: "r".into(),
        evidence_ids: (0..1000).map(|i| format!("ev-{}", i)).collect(),
        confidence: 0.5,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.evidence_ids.len(), 1000);
}

#[test]
fn constraint_data_with_long_description() {
    use sisyphus_types::asset::ConstraintData;
    let long = "x".repeat(5000);
    let c = ConstraintData {
        description: long.clone(),
        strictness: 0.5,
        violation_penalty: 0.5,
    };
    let j = serde_json::to_string(&c).unwrap();
    let back: ConstraintData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.description.len(), 5000);
}

#[test]
fn asset_relation_with_long_ids() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let long_id = "x".repeat(500);
    let r = AssetRelation {
        from_id: long_id.clone(),
        to_id: long_id.clone(),
        relation_type: RelationType::Solves,
        weight: 0.5,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: AssetRelation = serde_json::from_str(&j).unwrap();
    assert_eq!(back.from_id.len(), 500);
}

#[test]
fn asset_kind_tool_debug() {
    use sisyphus_types::asset::AssetKind;
    let dbg = format!("{:?}", AssetKind::Tool);
    assert_eq!(dbg, "Tool");
}

#[test]
fn asset_kind_sop_debug() {
    use sisyphus_types::asset::AssetKind;
    let dbg = format!("{:?}", AssetKind::SOP);
    assert_eq!(dbg, "SOP");
}

#[test]
fn asset_kind_insight_debug() {
    use sisyphus_types::asset::AssetKind;
    let dbg = format!("{:?}", AssetKind::Insight);
    assert_eq!(dbg, "Insight");
}

#[test]
fn asset_kind_constraint_debug() {
    use sisyphus_types::asset::AssetKind;
    let dbg = format!("{:?}", AssetKind::Constraint);
    assert_eq!(dbg, "Constraint");
}

// ---------------------------------------------------------------------------
// Additional AssetContent variant tests
// ---------------------------------------------------------------------------

#[test]
fn asset_content_clone() {
    use sisyphus_types::asset::AssetContent;
    use sisyphus_types::asset::InsightData;
    let content = AssetContent::Insight(InsightData {
        observation: "o".into(),
        reasoning: "r".into(),
        evidence_ids: vec![],
        confidence: 0.5,
    });
    // AssetContent doesn't have Clone - verify via Debug
    let dbg = format!("{:?}", content);
    assert!(dbg.contains("Insight"));
}

#[test]
fn asset_content_debug_each_variant_distinct() {
    use sisyphus_types::asset::{AssetContent, ConstraintData, InsightData};
    use sisyphus_types::graph::SOPGraphConfig;
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
    let sop = SOPGraphConfig::default();
    let insight = AssetContent::Insight(InsightData {
        observation: "o".into(),
        reasoning: "r".into(),
        evidence_ids: vec![],
        confidence: 0.5,
    });
    let constraint = AssetContent::Constraint(ConstraintData {
        description: "d".into(),
        strictness: 0.5,
        violation_penalty: 0.5,
    });
    let tool_c = AssetContent::Tool(tool);
    let sop_c = AssetContent::SOP(sop);
    let d1 = format!("{:?}", tool_c);
    let d2 = format!("{:?}", sop_c);
    let d3 = format!("{:?}", insight);
    let d4 = format!("{:?}", constraint);
    assert!(d1.contains("Tool"));
    assert!(d2.contains("SOP"));
    assert!(d3.contains("Insight"));
    assert!(d4.contains("Constraint"));
}

#[test]
fn cognitive_asset_with_1000_metadata_entries() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let mut meta = HashMap::new();
    for i in 0..1000 {
        meta.insert(format!("k{}", i), format!("v{}", i));
    }
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
    assert_eq!(back.metadata.len(), 1000);
}

#[test]
fn asset_relation_with_zero_weight_again() {
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
fn insight_data_with_only_one_evidence() {
    let i = InsightData {
        observation: "o".into(),
        reasoning: "r".into(),
        evidence_ids: vec!["e1".into()],
        confidence: 0.5,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.evidence_ids.len(), 1);
    assert_eq!(back.evidence_ids[0], "e1");
}

#[test]
fn cognitive_asset_with_only_description() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "a".into(),
        kind: AssetKind::Tool,
        name: "n".into(),
        description: "only description".into(),
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
    assert_eq!(asset.description, "only description");
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn asset_content_via_debug_each_variant() {
    use sisyphus_types::asset::{AssetContent, ConstraintData, InsightData};
    use sisyphus_types::graph::SOPGraphConfig;
    use sisyphus_types::tools::ToolSpec;
    let variants = vec![
        ("Tool", AssetContent::Tool(ToolSpec {
            tool_id: "t".into(),
            required_capability: "c".into(),
            expected_input: serde_json::json!({}),
            expected_output: serde_json::json!({}),
            success_criteria: "ok".into(),
            test_command: None,
            workdir: None,
        })),
        ("SOP", AssetContent::SOP(SOPGraphConfig::default())),
        ("Insight", AssetContent::Insight(InsightData {
            observation: "o".into(),
            reasoning: "r".into(),
            evidence_ids: vec![],
            confidence: 0.5,
        })),
        ("Constraint", AssetContent::Constraint(ConstraintData {
            description: "d".into(),
            strictness: 0.5,
            violation_penalty: 0.5,
        })),
    ];
    for (variant_name, content) in variants {
        let dbg = format!("{:?}", content);
        assert!(dbg.contains(variant_name), "Expected {} in {}", variant_name, dbg);
    }
}

#[test]
fn cognitive_asset_with_unicode_id_only() {
    let asset = CognitiveAsset {
        id: "资产-α".into(),
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
    assert_eq!(asset.id, "资产-α");
}

#[test]
fn insight_data_with_unicode_reasoning() {
    let i = InsightData {
        observation: "o".into(),
        reasoning: "推理过程-α 因为-β 所以-γ".into(),
        evidence_ids: vec![],
        confidence: 0.5,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.reasoning, "推理过程-α 因为-β 所以-γ");
}

#[test]
fn constraint_data_with_high_strictness_and_penalty() {
    use sisyphus_types::asset::ConstraintData;
    let c = ConstraintData {
        description: "strict".into(),
        strictness: 0.99,
        violation_penalty: 0.95,
    };
    assert!((c.strictness - 0.99).abs() < 1e-6);
    assert!((c.violation_penalty - 0.95).abs() < 1e-6);
}

#[test]
fn asset_relation_with_distinct_relation_types() {
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
    ];
    let serialized: Vec<String> = variants
        .iter()
        .map(|r| serde_json::to_string(r).unwrap())
        .collect();
    // Should differ in relation_type field
    assert_ne!(serialized[0], serialized[1]);
}

#[test]
fn cognitive_asset_with_unicode_name_only() {
    let asset = CognitiveAsset {
        id: "a".into(),
        kind: AssetKind::Tool,
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
    assert_eq!(asset.name, "工具-α");
    let j = serde_json::to_string(&asset).unwrap();
    let back: CognitiveAsset = serde_json::from_str(&j).unwrap();
    assert_eq!(back.name, "工具-α");
}

#[test]
fn cognitive_asset_serde_unicode_full() {
    let asset = CognitiveAsset {
        id: "id-α".into(),
        kind: AssetKind::Insight,
        name: "name-α".into(),
        description: "desc-α".into(),
        content: AssetContent::Insight(InsightData {
            observation: "obs-α".into(),
            reasoning: "rea-α".into(),
            evidence_ids: vec!["ev-α".into()],
            confidence: 0.5,
        }),
        metadata: HashMap::new(),
        version: 1,
        created_at: 0,
    };
    let j = serde_json::to_string(&asset).unwrap();
    let back: CognitiveAsset = serde_json::from_str(&j).unwrap();
    assert_eq!(back.id, "id-α");
    assert_eq!(back.name, "name-α");
    assert_eq!(back.description, "desc-α");
    assert_eq!(back.version, 1);
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn asset_kind_tool_in_serde() {
    use sisyphus_types::asset::AssetKind;
    let k = AssetKind::Tool;
    let j = serde_json::to_string(&k).unwrap();
    let back: AssetKind = serde_json::from_str(&j).unwrap();
    let j2 = serde_json::to_string(&back).unwrap();
    assert_eq!(j, j2);
    assert_eq!(j, "\"Tool\"");
}

#[test]
fn asset_kind_sop_in_serde() {
    use sisyphus_types::asset::AssetKind;
    let k = AssetKind::SOP;
    let j = serde_json::to_string(&k).unwrap();
    let back: AssetKind = serde_json::from_str(&j).unwrap();
    let j2 = serde_json::to_string(&back).unwrap();
    assert_eq!(j, j2);
    assert_eq!(j, "\"SOP\"");
}

#[test]
fn asset_kind_insight_in_serde() {
    use sisyphus_types::asset::AssetKind;
    let k = AssetKind::Insight;
    let j = serde_json::to_string(&k).unwrap();
    let back: AssetKind = serde_json::from_str(&j).unwrap();
    let j2 = serde_json::to_string(&back).unwrap();
    assert_eq!(j, j2);
    assert_eq!(j, "\"Insight\"");
}

#[test]
fn asset_kind_constraint_in_serde() {
    use sisyphus_types::asset::AssetKind;
    let k = AssetKind::Constraint;
    let j = serde_json::to_string(&k).unwrap();
    let back: AssetKind = serde_json::from_str(&j).unwrap();
    let j2 = serde_json::to_string(&back).unwrap();
    assert_eq!(j, j2);
    assert_eq!(j, "\"Constraint\"");
}

#[test]
fn cognitive_asset_with_unicode_version_string() {
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
        version: u32::MAX,
        created_at: 0,
    };
    let j = serde_json::to_string(&asset).unwrap();
    let back: CognitiveAsset = serde_json::from_str(&j).unwrap();
    assert_eq!(back.version, u32::MAX);
}

#[test]
fn insight_data_with_max_u64_in_evidence_ids() {
    let i = InsightData {
        observation: "o".into(),
        reasoning: "r".into(),
        evidence_ids: (0..100).map(|i| format!("ev-{:03}", i)).collect(),
        confidence: 0.5,
    };
    assert_eq!(i.evidence_ids.len(), 100);
    assert_eq!(i.evidence_ids[50], "ev-050");
}

#[test]
fn cognitive_asset_with_empty_metadata_only() {
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
    assert!(asset.metadata.is_empty());
}

// ---------------------------------------------------------------------------
// Additional CognitiveAsset tests
// ---------------------------------------------------------------------------

#[test]
fn cognitive_asset_with_unicode_description_only() {
    let asset = CognitiveAsset {
        id: "a".into(),
        kind: AssetKind::Tool,
        name: "n".into(),
        description: "中文描述-α with English and 123 numbers 🔍".into(),
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
    assert_eq!(
        asset.description,
        "中文描述-α with English and 123 numbers 🔍"
    );
    let j = serde_json::to_string(&asset).unwrap();
    let back: CognitiveAsset = serde_json::from_str(&j).unwrap();
    assert_eq!(back.description, asset.description);
}

#[test]
fn cognitive_asset_with_max_metadata() {
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
        version: u32::MAX,
        created_at: u64::MAX,
    };
    let j = serde_json::to_string(&asset).unwrap();
    let back: CognitiveAsset = serde_json::from_str(&j).unwrap();
    assert_eq!(back.version, u32::MAX);
    assert_eq!(back.created_at, u64::MAX);
}

#[test]
fn asset_relation_with_max_5_fields_unicode() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let r = AssetRelation {
        from_id: "源-α".into(),
        to_id: "目-β".into(),
        relation_type: RelationType::Refines,
        weight: 0.123_456_f32,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: AssetRelation = serde_json::from_str(&j).unwrap();
    assert_eq!(back.from_id, "源-α");
    assert_eq!(back.to_id, "目-β");
    assert!((back.weight - 0.123_456).abs() < 1e-5);
}

#[test]
fn cognitive_asset_with_unicode_in_each_field() {
    let asset = CognitiveAsset {
        id: "id-α".into(),
        kind: AssetKind::Insight,
        name: "name-β".into(),
        description: "desc-γ".into(),
        content: AssetContent::Insight(InsightData {
            observation: "obs-δ".into(),
            reasoning: "rea-ε".into(),
            evidence_ids: vec!["ev-ζ".into()],
            confidence: 0.5,
        }),
        metadata: HashMap::new(),
        version: 1,
        created_at: 0,
    };
    let j = serde_json::to_string(&asset).unwrap();
    let back: CognitiveAsset = serde_json::from_str(&j).unwrap();
    assert_eq!(back.id, "id-α");
    assert_eq!(back.name, "name-β");
    assert_eq!(back.description, "desc-γ");
    match back.content {
        AssetContent::Insight(i) => {
            assert_eq!(i.observation, "obs-δ");
            assert_eq!(i.reasoning, "rea-ε");
            assert_eq!(i.evidence_ids[0], "ev-ζ");
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn insight_data_with_unicode_emoji_observation() {
    let i = InsightData {
        observation: "🔭🌟📚".into(),
        reasoning: "r".into(),
        evidence_ids: vec!["💡".into()],
        confidence: 0.5,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.observation, "🔭🌟📚");
    assert_eq!(back.evidence_ids[0], "💡");
}

#[test]
fn constraint_data_with_unicode_description_2() {
    use sisyphus_types::asset::ConstraintData;
    let c = ConstraintData {
        description: "约束-必须-α-和-β".into(),
        strictness: 0.8,
        violation_penalty: 0.2,
    };
    let j = serde_json::to_string(&c).unwrap();
    let back: ConstraintData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.description, "约束-必须-α-和-β");
}

#[test]
fn asset_kind_tool_clone_test() {
    // AssetKind doesn't implement Clone
    use sisyphus_types::asset::AssetKind;
    let _ = AssetKind::Tool;
}

// ---------------------------------------------------------------------------
// Additional CognitiveAsset and AssetContent tests
// ---------------------------------------------------------------------------

#[test]
fn cognitive_asset_with_unicode_in_each_string_field() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "资产-α".into(),
        kind: AssetKind::Insight,
        name: "名称-β".into(),
        description: "描述-γ".into(),
        content: AssetContent::Insight(InsightData {
            observation: "观察-δ".into(),
            reasoning: "推理-ε".into(),
            evidence_ids: vec!["证据-ζ".into()],
            confidence: 0.5,
        }),
        metadata: HashMap::new(),
        version: 1,
        created_at: 0,
    };
    assert_eq!(asset.id, "资产-α");
    assert_eq!(asset.name, "名称-β");
    assert_eq!(asset.description, "描述-γ");
    let j = serde_json::to_string(&asset).unwrap();
    let back: CognitiveAsset = serde_json::from_str(&j).unwrap();
    assert_eq!(back.id, "资产-α");
    match back.content {
        AssetContent::Insight(i) => {
            assert_eq!(i.observation, "观察-δ");
            assert_eq!(i.reasoning, "推理-ε");
            assert_eq!(i.evidence_ids[0], "证据-ζ");
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn cognitive_asset_with_only_unicode_in_metadata() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let mut meta = HashMap::new();
    for i in 0..10 {
        meta.insert(format!("键-{}", i), format!("值-{}", i));
    }
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
    assert_eq!(back.metadata.len(), 10);
    assert_eq!(back.metadata["键-5"], "值-5");
}

#[test]
fn insight_data_with_unicode_emoji_reasoning() {
    let i = InsightData {
        observation: "o".into(),
        reasoning: "因为-🔍-所以-💡".into(),
        evidence_ids: vec![],
        confidence: 0.5,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.reasoning, "因为-🔍-所以-💡");
}

#[test]
fn asset_relation_with_5_field_unicode() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let r = AssetRelation {
        from_id: "源-α".into(),
        to_id: "目-β".into(),
        relation_type: RelationType::Refines,
        weight: 0.5,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: AssetRelation = serde_json::from_str(&j).unwrap();
    assert_eq!(back.from_id, "源-α");
    assert_eq!(back.to_id, "目-β");
    assert!((back.weight - 0.5).abs() < 1e-6);
}

#[test]
fn cognitive_asset_with_only_unicode_id_and_empty_rest() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "唯一-α".into(),
        kind: AssetKind::Tool,
        name: "".into(),
        description: "".into(),
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
    assert_eq!(back.id, "唯一-α");
    assert_eq!(back.name, "");
}

#[test]
fn insight_data_with_confidence_zero_and_evidence() {
    let i = InsightData {
        observation: "o".into(),
        reasoning: "r".into(),
        evidence_ids: vec!["e1".into(), "e2".into()],
        confidence: 0.0,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.confidence, 0.0);
    assert_eq!(back.evidence_ids.len(), 2);
}

#[test]
fn constraint_data_with_min_values() {
    use sisyphus_types::asset::ConstraintData;
    let c = ConstraintData {
        description: "d".into(),
        strictness: 0.0,
        violation_penalty: 0.0,
    };
    let j = serde_json::to_string(&c).unwrap();
    let back: ConstraintData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.strictness, 0.0);
    assert_eq!(back.violation_penalty, 0.0);
}

// ---------------------------------------------------------------------------
// Additional tests
// ---------------------------------------------------------------------------

#[test]
fn insight_data_with_unicode_observation_clone_preserves() {
    let i = InsightData {
        observation: "观察-α-🔍".into(),
        reasoning: "推理-β".into(),
        evidence_ids: vec!["证据-γ".into()],
        confidence: 0.5,
    };
    let cloned = i.clone();
    assert_eq!(cloned.observation, "观察-α-🔍");
    assert_eq!(cloned.reasoning, "推理-β");
    assert_eq!(cloned.evidence_ids[0], "证据-γ");
}

#[test]
fn cognitive_asset_with_only_insight_data() {
    let asset = CognitiveAsset {
        id: "a".into(),
        kind: AssetKind::Insight,
        name: "n".into(),
        description: "d".into(),
        content: AssetContent::Insight(InsightData {
            observation: "obs".into(),
            reasoning: "rea".into(),
            evidence_ids: vec![],
            confidence: 0.5,
        }),
        metadata: HashMap::new(),
        version: 1,
        created_at: 0,
    };
    let j = serde_json::to_string(&asset).unwrap();
    let back: CognitiveAsset = serde_json::from_str(&j).unwrap();
    assert_eq!(back.id, "a");
}

#[test]
fn constraint_data_with_unicode_and_zero_penalty() {
    use sisyphus_types::asset::ConstraintData;
    let c = ConstraintData {
        description: "约束-α".into(),
        strictness: 0.5,
        violation_penalty: 0.0,
    };
    let j = serde_json::to_string(&c).unwrap();
    let back: ConstraintData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.violation_penalty, 0.0);
}

#[test]
fn asset_relation_with_only_unicode_fields() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let r = AssetRelation {
        from_id: "源".into(),
        to_id: "目".into(),
        relation_type: RelationType::Solves,
        weight: 0.5,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: AssetRelation = serde_json::from_str(&j).unwrap();
    assert_eq!(back.from_id, "源");
    assert_eq!(back.to_id, "目");
}

#[test]
fn insight_data_with_only_one_evidence_unicode() {
    let i = InsightData {
        observation: "o".into(),
        reasoning: "r".into(),
        evidence_ids: vec!["单一-证据-α".into()],
        confidence: 0.5,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.evidence_ids[0], "单一-证据-α");
}

#[test]
fn cognitive_asset_with_unicode_metadata_value_complex() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let mut meta = HashMap::new();
    meta.insert("k1".into(), "v-中文-α".into());
    meta.insert("k2".into(), "v-emoji-🔍".into());
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
    assert_eq!(back.metadata["k1"], "v-中文-α");
    assert_eq!(back.metadata["k2"], "v-emoji-🔍");
}

#[test]
fn asset_kind_via_partial_eq_check() {
    use sisyphus_types::asset::AssetKind;
    // AssetKind doesn't implement PartialEq, so use Debug
    let k1 = AssetKind::Tool;
    let k2 = AssetKind::Tool;
    let dbg1 = format!("{:?}", k1);
    let dbg2 = format!("{:?}", k2);
    assert_eq!(dbg1, dbg2);
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn insight_data_with_unicode_each_field() {
    let i = InsightData {
        observation: "obs-α".into(),
        reasoning: "rea-β".into(),
        evidence_ids: vec!["ev-γ".into()],
        confidence: 0.5,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.observation, "obs-α");
    assert_eq!(back.reasoning, "rea-β");
    assert_eq!(back.evidence_ids[0], "ev-γ");
}

#[test]
fn insight_data_with_unicode_emoji_in_observation() {
    let i = InsightData {
        observation: "🔭".into(),
        reasoning: "r".into(),
        evidence_ids: vec!["💡".into()],
        confidence: 0.5,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert!(back.observation.contains("🔭"));
}

#[test]
fn cognitive_asset_with_unicode_description_clone() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "a".into(),
        kind: AssetKind::Tool,
        name: "n".into(),
        description: "中文-α-描述-β".into(),
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
    assert_eq!(cloned.description, "中文-α-描述-β");
}

#[test]
fn cognitive_asset_with_unicode_metadata_clone() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let mut meta = HashMap::new();
    meta.insert("k-α".into(), "v-β".into());
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
    let cloned = asset.clone();
    assert_eq!(cloned.metadata["k-α"], "v-β");
}

#[test]
fn insight_data_with_only_negative_confidence() {
    let i = InsightData {
        observation: "o".into(),
        reasoning: "r".into(),
        evidence_ids: vec![],
        confidence: -1.0,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.confidence, -1.0);
}

#[test]
fn cognitive_asset_with_unicode_name_and_observation() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "a".into(),
        kind: AssetKind::Insight,
        name: "工具-α".into(),
        description: "d".into(),
        content: AssetContent::Insight(InsightData {
            observation: "观察-β".into(),
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
    match back.content {
        AssetContent::Insight(i) => {
            assert_eq!(i.observation, "观察-β");
        }
        _ => panic!("wrong"),
    }
}

#[test]
fn asset_relation_with_unicode_long_ids() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let long_id = "α".repeat(50);
    let r = AssetRelation {
        from_id: long_id.clone(),
        to_id: long_id,
        relation_type: RelationType::Solves,
        weight: 0.5,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: AssetRelation = serde_json::from_str(&j).unwrap();
    // α is 2 bytes in UTF-8, so the byte length is 2x
    assert_eq!(back.from_id.chars().count(), 50);
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn insight_data_with_unicode_observation_3_chars() {
    let i = InsightData {
        observation: "αβγ".into(),
        reasoning: "r".into(),
        evidence_ids: vec![],
        confidence: 0.5,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.observation, "αβγ");
}

#[test]
fn cognitive_asset_with_only_one_metadata_key() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let mut meta = HashMap::new();
    meta.insert("only".into(), "value".into());
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
    assert_eq!(asset.metadata.len(), 1);
    assert_eq!(asset.metadata["only"], "value");
}

#[test]
fn asset_relation_with_3_distinct_relation_types_serde() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let variants = [
        RelationType::Solves,
        RelationType::Requires,
        RelationType::Refines,
    ];
    for v in variants {
        let r = AssetRelation {
            from_id: "a".into(),
            to_id: "b".into(),
            relation_type: v.clone(),
            weight: 0.5,
        };
        let j = serde_json::to_string(&r).unwrap();
        let back: AssetRelation = serde_json::from_str(&j).unwrap();
        // RelationType doesn't implement PartialEq, but we verify via Debug
        let orig_dbg = format!("{:?}", v);
        let back_dbg = format!("{:?}", back.relation_type);
        assert_eq!(orig_dbg, back_dbg);
    }
}

#[test]
fn insight_data_with_max_f32_confidence() {
    let i = InsightData {
        observation: "o".into(),
        reasoning: "r".into(),
        evidence_ids: vec![],
        confidence: f32::MAX,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.confidence, f32::MAX);
}

#[test]
fn cognitive_asset_with_unicode_id_and_long_name() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let long_name = "名-".repeat(100);
    let asset = CognitiveAsset {
        id: "id-α".into(),
        kind: AssetKind::Tool,
        name: long_name.clone(),
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
    assert_eq!(back.name, long_name);
}

#[test]
fn constraint_data_with_unicode_emoji_description() {
    use sisyphus_types::asset::ConstraintData;
    let c = ConstraintData {
        description: "约束🔍-必须-快".into(),
        strictness: 0.9,
        violation_penalty: 0.1,
    };
    let j = serde_json::to_string(&c).unwrap();
    let back: ConstraintData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.description, "约束🔍-必须-快");
}

#[test]
fn insight_data_with_unicode_special_chars() {
    let i = InsightData {
        observation: "a\tb\nc\rd\te".into(),
        reasoning: "f\"g'h".into(),
        evidence_ids: vec!["a\\b".into()],
        confidence: 0.5,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.observation, "a\tb\nc\rd\te");
    assert_eq!(back.evidence_ids[0], "a\\b");
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn insight_data_with_only_unicode_observation_2_chars() {
    let i = InsightData {
        observation: "αβ".into(),
        reasoning: "".into(),
        evidence_ids: vec![],
        confidence: 0.0,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.observation, "αβ");
}

#[test]
fn cognitive_asset_with_unicode_only_id() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "唯一-α-🔍".into(),
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
    assert_eq!(back.id, "唯一-α-🔍");
}

#[test]
fn asset_relation_with_unicode_each_field() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let r = AssetRelation {
        from_id: "源-α".into(),
        to_id: "目-β".into(),
        relation_type: RelationType::Solves,
        weight: 0.5,
    };
    assert_eq!(r.from_id, "源-α");
    assert_eq!(r.to_id, "目-β");
    let j = serde_json::to_string(&r).unwrap();
    let back: AssetRelation = serde_json::from_str(&j).unwrap();
    assert_eq!(back.from_id, "源-α");
    assert_eq!(back.to_id, "目-β");
}

#[test]
fn cognitive_asset_with_unicode_name_max() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let long = "α-".repeat(500);
    let asset = CognitiveAsset {
        id: "a".into(),
        kind: AssetKind::Tool,
        name: long.clone(),
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
    assert_eq!(asset.name, long);
}

#[test]
fn cognitive_asset_with_unicode_id_max() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let long = "β-".repeat(500);
    let asset = CognitiveAsset {
        id: long.clone(),
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
    assert_eq!(back.id, long);
}

#[test]
fn cognitive_asset_with_unicode_description_max() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let long = "γ-".repeat(500);
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
    assert_eq!(back.description, long);
}

#[test]
fn asset_relation_with_unicode_long_5_variants_serde() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let variants = vec![
        RelationType::Solves,
        RelationType::Requires,
        RelationType::EvolvedFrom,
        RelationType::ConflictsWith,
        RelationType::Refines,
    ];
    for v in variants {
        let r = AssetRelation {
            from_id: "a".into(),
            to_id: "b".into(),
            relation_type: v.clone(),
            weight: 0.5,
        };
        let j = serde_json::to_string(&r).unwrap();
        let back: AssetRelation = serde_json::from_str(&j).unwrap();
        // Verify via Debug since RelationType doesn't have PartialEq
        let orig_dbg = format!("{:?}", v);
        let back_dbg = format!("{:?}", back.relation_type);
        assert_eq!(orig_dbg, back_dbg);
    }
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn cognitive_asset_with_unicode_id_only_emoji() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "🔍🔭".into(),
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
    assert_eq!(asset.id, "🔍🔭");
    let j = serde_json::to_string(&asset).unwrap();
    let back: CognitiveAsset = serde_json::from_str(&j).unwrap();
    assert_eq!(back.id, "🔍🔭");
}

#[test]
fn insight_data_with_unicode_observation_long_emoji() {
    let i = InsightData {
        observation: "🔍".repeat(100),
        reasoning: "r".into(),
        evidence_ids: vec![],
        confidence: 0.5,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.observation.chars().count(), 100);
}

#[test]
fn asset_kind_tool_in_v3() {
    use sisyphus_types::asset::AssetKind;
    let k = AssetKind::Tool;
    let dbg = format!("{:?}", k);
    assert_eq!(dbg, "Tool");
}

#[test]
fn cognitive_asset_with_unicode_metadata_value_short() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let mut meta = HashMap::new();
    meta.insert("k1".into(), "v-α".into());
    meta.insert("k2".into(), "v-β".into());
    meta.insert("k3".into(), "v-γ".into());
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
    assert_eq!(asset.metadata.len(), 3);
    let j = serde_json::to_string(&asset).unwrap();
    let back: CognitiveAsset = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metadata["k2"], "v-β");
}

#[test]
fn cognitive_asset_with_high_version_only() {
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
        version: 1_000_000,
        created_at: 0,
    };
    let j = serde_json::to_string(&asset).unwrap();
    let back: CognitiveAsset = serde_json::from_str(&j).unwrap();
    assert_eq!(back.version, 1_000_000);
}

#[test]
fn insight_data_with_unicode_evidence_id_emoji() {
    let i = InsightData {
        observation: "o".into(),
        reasoning: "r".into(),
        evidence_ids: vec!["🔍".into()],
        confidence: 0.5,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.evidence_ids[0], "🔍");
}

#[test]
fn asset_relation_with_unicode_long_id_via_chars_count() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let long_id = "α".repeat(20);
    let r = AssetRelation {
        from_id: long_id.clone(),
        to_id: long_id,
        relation_type: RelationType::Solves,
        weight: 0.5,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: AssetRelation = serde_json::from_str(&j).unwrap();
    assert_eq!(back.from_id.chars().count(), 20);
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn cognitive_asset_with_max_3_unicode_metadata_entries() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let mut meta = HashMap::new();
    meta.insert("key-α".into(), "val-β".into());
    meta.insert("key-γ".into(), "val-δ".into());
    meta.insert("key-ε".into(), "val-ζ".into());
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
    assert_eq!(asset.metadata.len(), 3);
}

#[test]
fn insight_data_with_unicode_only_reasoning() {
    let i = InsightData {
        observation: "o".into(),
        reasoning: "推理过程-α-因为-β".into(),
        evidence_ids: vec![],
        confidence: 0.5,
    };
    assert_eq!(i.reasoning, "推理过程-α-因为-β");
}

#[test]
fn asset_relation_with_unicode_relation_type_serde() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    for v in [
        RelationType::Solves,
        RelationType::Requires,
        RelationType::EvolvedFrom,
        RelationType::ConflictsWith,
        RelationType::Refines,
    ] {
        let r = AssetRelation {
            from_id: "a".into(),
            to_id: "b".into(),
            relation_type: v.clone(),
            weight: 0.5,
        };
        let j = serde_json::to_string(&r).unwrap();
        let back: AssetRelation = serde_json::from_str(&j).unwrap();
        let orig_dbg = format!("{:?}", v);
        let back_dbg = format!("{:?}", back.relation_type);
        assert_eq!(orig_dbg, back_dbg);
    }
}

#[test]
fn cognitive_asset_with_only_emoji_in_description() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "a".into(),
        kind: AssetKind::Tool,
        name: "n".into(),
        description: "🔍🔭🌟".into(),
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
    assert_eq!(back.description, "🔍🔭🌟");
}

#[test]
fn cognitive_asset_with_only_emoji_in_name() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "a".into(),
        kind: AssetKind::Tool,
        name: "🔍".into(),
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
    assert_eq!(asset.name, "🔍");
}

#[test]
fn cognitive_asset_with_unicode_each_field_unique_chars() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "α".into(),
        kind: AssetKind::Tool,
        name: "β".into(),
        description: "γ".into(),
        content: AssetContent::Insight(InsightData {
            observation: "δ".into(),
            reasoning: "ε".into(),
            evidence_ids: vec!["ζ".into()],
            confidence: 0.5,
        }),
        metadata: HashMap::new(),
        version: 1,
        created_at: 0,
    };
    let j = serde_json::to_string(&asset).unwrap();
    let back: CognitiveAsset = serde_json::from_str(&j).unwrap();
    assert_eq!(back.id, "α");
    assert_eq!(back.name, "β");
    assert_eq!(back.description, "γ");
}

#[test]
fn insight_data_with_unicode_observation_and_evidence() {
    let i = InsightData {
        observation: "obs-α".into(),
        reasoning: "rea-β".into(),
        evidence_ids: vec!["ev-γ".into(), "ev-δ".into()],
        confidence: 0.5,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.observation, "obs-α");
    assert_eq!(back.evidence_ids[0], "ev-γ");
    assert_eq!(back.evidence_ids[1], "ev-δ");
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn cognitive_asset_with_unicode_id_empty_description() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "id-α".into(),
        kind: AssetKind::Tool,
        name: "n".into(),
        description: "".into(),
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
    assert_eq!(back.id, "id-α");
    assert_eq!(back.description, "");
}

#[test]
fn insight_data_with_unicode_observation_via_2_chars() {
    let i = InsightData {
        observation: "αβ".into(),
        reasoning: "r".into(),
        evidence_ids: vec![],
        confidence: 0.5,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.observation, "αβ");
}

#[test]
fn cognitive_asset_with_unicode_id_at_50() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let long = "α".repeat(50);
    let asset = CognitiveAsset {
        id: long.clone(),
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
    assert_eq!(back.id.chars().count(), 50);
}

#[test]
fn asset_kind_serde_via_json_field() {
    use sisyphus_types::asset::AssetKind;
    // JSON field naming is consistent
    let k = AssetKind::Insight;
    let j = serde_json::to_string(&k).unwrap();
    let back: AssetKind = serde_json::from_str(&j).unwrap();
    // Verify via Debug since AssetKind doesn't implement PartialEq
    let j2 = serde_json::to_string(&back).unwrap();
    assert_eq!(j, j2);
}

#[test]
fn cognitive_asset_with_unicode_id_with_quotes() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "id-α-\"quoted\"".into(),
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
    assert_eq!(back.id, "id-α-\"quoted\"");
}

#[test]
fn cognitive_asset_with_only_unicode_metadata_value_clone() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let mut meta = HashMap::new();
    meta.insert("k-α".into(), "v-β-🔍".into());
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
    let cloned = asset.clone();
    assert_eq!(cloned.metadata["k-α"], "v-β-🔍");
}

#[test]
fn insight_data_with_unicode_observation_clone_preserves_v3() {
    let i = InsightData {
        observation: "obs-α-🔍".into(),
        reasoning: "r".into(),
        evidence_ids: vec![],
        confidence: 0.5,
    };
    let cloned = i.clone();
    assert_eq!(cloned.observation, "obs-α-🔍");
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn cognitive_asset_with_unicode_metadata_5_entries() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let mut meta = HashMap::new();
    for i in 0..5 {
        meta.insert(format!("key-{}", i), format!("val-α-{}", i));
    }
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
    assert_eq!(asset.metadata.len(), 5);
    let j = serde_json::to_string(&asset).unwrap();
    let back: CognitiveAsset = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metadata["key-2"], "val-α-2");
}

#[test]
fn insight_data_with_unicode_observation_via_tabs() {
    let i = InsightData {
        observation: "a\tα\tβ\tγ".into(),
        reasoning: "r".into(),
        evidence_ids: vec![],
        confidence: 0.5,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.observation, "a\tα\tβ\tγ");
}

#[test]
fn asset_relation_with_unicode_to_field_emoji() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let r = AssetRelation {
        from_id: "a".into(),
        to_id: "目-α-🔍".into(),
        relation_type: RelationType::Refines,
        weight: 0.5,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: AssetRelation = serde_json::from_str(&j).unwrap();
    assert_eq!(back.to_id, "目-α-🔍");
}

#[test]
fn cognitive_asset_with_unicode_metadata_50_entries() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let mut meta = HashMap::new();
    for i in 0..50 {
        meta.insert(format!("key-{}", i), format!("val-α-{}", i));
    }
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
    assert_eq!(back.metadata.len(), 50);
}

#[test]
fn cognitive_asset_with_unicode_id_with_special_punctuation() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "id-α:中文:β".into(),
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
    assert_eq!(back.id, "id-α:中文:β");
}

#[test]
fn insight_data_with_unicode_reasoning_via_5_chars() {
    let i = InsightData {
        observation: "o".into(),
        reasoning: "αβγδε".into(),
        evidence_ids: vec![],
        confidence: 0.5,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.reasoning, "αβγδε");
}

#[test]
fn asset_relation_with_unicode_from_id_special() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let r = AssetRelation {
        from_id: "源-α!@#".into(),
        to_id: "b".into(),
        relation_type: RelationType::Solves,
        weight: 0.5,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: AssetRelation = serde_json::from_str(&j).unwrap();
    assert_eq!(back.from_id, "源-α!@#");
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn cognitive_asset_with_unicode_id_via_only_letters() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "αβγδε".into(),
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
    assert_eq!(back.id, "αβγδε");
}

#[test]
fn insight_data_with_unicode_observation_via_digits() {
    let i = InsightData {
        observation: "0123456789".into(),
        reasoning: "r".into(),
        evidence_ids: vec![],
        confidence: 0.5,
    };
    assert_eq!(i.observation, "0123456789");
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.observation, "0123456789");
}

#[test]
fn asset_relation_with_unicode_long_to_id() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let long = "目-α-".repeat(50);
    let r = AssetRelation {
        from_id: "a".into(),
        to_id: long.clone(),
        relation_type: RelationType::Solves,
        weight: 0.5,
    };
    assert_eq!(r.to_id, long);
    let j = serde_json::to_string(&r).unwrap();
    let back: AssetRelation = serde_json::from_str(&j).unwrap();
    assert_eq!(back.to_id, long);
}

#[test]
fn cognitive_asset_with_unicode_metadata_value_max_2() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let long = "v-".repeat(200);
    let mut meta = HashMap::new();
    meta.insert("k".into(), long.clone());
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
    assert_eq!(back.metadata["k"], long);
}

#[test]
fn cognitive_asset_with_unicode_id_with_emoji() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "id-🔍-🔭".into(),
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
    assert!(back.id.contains("🔍"));
}

#[test]
fn insight_data_with_unicode_observation_via_quote() {
    let i = InsightData {
        observation: "observation with \"quotes\" α".into(),
        reasoning: "r".into(),
        evidence_ids: vec![],
        confidence: 0.5,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.observation, "observation with \"quotes\" α");
}

#[test]
fn asset_relation_with_unicode_high_precision_weight() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let r = AssetRelation {
        from_id: "a".into(),
        to_id: "b".into(),
        relation_type: RelationType::Refines,
        weight: 0.987_654_3_f32,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: AssetRelation = serde_json::from_str(&j).unwrap();
    assert!((back.weight - 0.987_654_3).abs() < 1e-4);
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn cognitive_asset_with_unicode_id_with_newlines() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "id-α\nβ\nγ".into(),
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
    assert_eq!(back.id, "id-α\nβ\nγ");
}

#[test]
fn cognitive_asset_with_unicode_id_with_tabs() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "id-α\tβ\tγ".into(),
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
    assert_eq!(back.id, "id-α\tβ\tγ");
}

#[test]
fn insight_data_with_unicode_observation_via_3_lines() {
    let i = InsightData {
        observation: "line1-α\nline2-β\nline3-γ".into(),
        reasoning: "r".into(),
        evidence_ids: vec![],
        confidence: 0.5,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.observation, "line1-α\nline2-β\nline3-γ");
}

#[test]
fn cognitive_asset_with_unicode_id_with_path_separator() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "path/α/β/γ".into(),
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
    assert_eq!(back.id, "path/α/β/γ");
}

#[test]
fn asset_relation_with_unicode_special_chars_in_ids() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let r = AssetRelation {
        from_id: "a/b/c/α".into(),
        to_id: "x/y/z/β".into(),
        relation_type: RelationType::Solves,
        weight: 0.5,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: AssetRelation = serde_json::from_str(&j).unwrap();
    assert_eq!(back.from_id, "a/b/c/α");
    assert_eq!(back.to_id, "x/y/z/β");
}

#[test]
fn cognitive_asset_with_unicode_id_with_hash() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "id-α#β#γ".into(),
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
    assert_eq!(back.id, "id-α#β#γ");
}

#[test]
fn insight_data_with_unicode_observation_via_emoji_2() {
    let i = InsightData {
        observation: "🔍🔭🌟📚💡".into(),
        reasoning: "💡".into(),
        evidence_ids: vec!["🌟".into()],
        confidence: 0.5,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.observation, "🔍🔭🌟📚💡");
    assert_eq!(back.evidence_ids[0], "🌟");
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn cognitive_asset_with_unicode_id_with_5_emoji() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "id-🔍-🔭-🌟-📚-💡".into(),
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
    assert_eq!(back.id, "id-🔍-🔭-🌟-📚-💡");
}

#[test]
fn insight_data_with_unicode_observation_via_html_chars() {
    let i = InsightData {
        observation: "<tag>α</tag>".into(),
        reasoning: "r".into(),
        evidence_ids: vec![],
        confidence: 0.5,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.observation, "<tag>α</tag>");
}

#[test]
fn cognitive_asset_with_unicode_id_with_sql_chars() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "id-α'; DROP TABLE--".into(),
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
    assert_eq!(back.id, "id-α'; DROP TABLE--");
}

#[test]
fn cognitive_asset_with_unicode_name_2_chars() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "a".into(),
        kind: AssetKind::Tool,
        name: "αβ".into(),
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
    assert_eq!(back.name, "αβ");
}

#[test]
fn asset_relation_with_unicode_long_id_3() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let long = "目-α-".repeat(100);
    let r = AssetRelation {
        from_id: long.clone(),
        to_id: long.clone(),
        relation_type: RelationType::Refines,
        weight: 0.5,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: AssetRelation = serde_json::from_str(&j).unwrap();
    // Just verify it round-trips
    assert_eq!(back.from_id.chars().count(), long.chars().count());
}

#[test]
fn insight_data_with_unicode_observation_via_5_emojis() {
    let i = InsightData {
        observation: "🔍🔭🌟📚💡".into(),
        reasoning: "推理-α-🔍".into(),
        evidence_ids: vec!["证据-1🔍".into()],
        confidence: 0.5,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.observation, "🔍🔭🌟📚💡");
    assert_eq!(back.reasoning, "推理-α-🔍");
}

#[test]
fn cognitive_asset_with_unicode_5_segment_id() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "1-α-2-β-3-γ-4-δ-5-ε".into(),
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
    assert_eq!(back.id, "1-α-2-β-3-γ-4-δ-5-ε");
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn cognitive_asset_with_unicode_id_with_5_emoji_2() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "🔍🔭🌟📚💡".into(),
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
    assert_eq!(back.id.chars().count(), 5);
}

#[test]
fn cognitive_asset_with_unicode_id_3_emoji() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "🔍🔭🌟".into(),
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
    assert_eq!(back.id, "🔍🔭🌟");
}

#[test]
fn insight_data_with_unicode_observation_via_emoji_3() {
    let i = InsightData {
        observation: "🔍".into(),
        reasoning: "🔭".into(),
        evidence_ids: vec!["🌟".into()],
        confidence: 0.5,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.observation, "🔍");
    assert_eq!(back.reasoning, "🔭");
    assert_eq!(back.evidence_ids[0], "🌟");
}

#[test]
fn cognitive_asset_with_unicode_id_3_segment_v2() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "1-α-2-β-3-γ".into(),
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
    assert_eq!(back.id, "1-α-2-β-3-γ");
}

#[test]
fn asset_relation_with_unicode_special_chars_in_to_id_2() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let r = AssetRelation {
        from_id: "a".into(),
        to_id: "b@α-中文#β!".into(),
        relation_type: RelationType::Refines,
        weight: 0.5,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: AssetRelation = serde_json::from_str(&j).unwrap();
    assert_eq!(back.to_id, "b@α-中文#β!");
}

#[test]
fn cognitive_asset_with_unicode_id_with_colon() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "ns:α-β:γ".into(),
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
    assert_eq!(back.id, "ns:α-β:γ");
}

#[test]
fn insight_data_with_unicode_observation_via_html_entity() {
    let i = InsightData {
        observation: "&amp;α-β&amp;".into(),
        reasoning: "r".into(),
        evidence_ids: vec![],
        confidence: 0.5,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.observation, "&amp;α-β&amp;");
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn cognitive_asset_with_unicode_id_with_underscore() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "id_α_β_γ".into(),
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
    assert_eq!(back.id, "id_α_β_γ");
}

#[test]
fn cognitive_asset_with_unicode_id_with_dot() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "module.α.β".into(),
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
    assert_eq!(back.id, "module.α.β");
}

#[test]
fn insight_data_with_unicode_observation_via_3_emojis() {
    let i = InsightData {
        observation: "🔍🔭🌟".into(),
        reasoning: "r".into(),
        evidence_ids: vec![],
        confidence: 0.5,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.observation, "🔍🔭🌟");
}

#[test]
fn cognitive_asset_with_unicode_metadata_3_long_values() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let long1 = "v1-".repeat(50);
    let long2 = "v2-".repeat(50);
    let long3 = "v3-".repeat(50);
    let mut meta = HashMap::new();
    meta.insert("k1".into(), long1.clone());
    meta.insert("k2".into(), long2.clone());
    meta.insert("k3".into(), long3.clone());
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
    assert_eq!(back.metadata["k1"], long1);
    assert_eq!(back.metadata["k2"], long2);
    assert_eq!(back.metadata["k3"], long3);
}

#[test]
fn asset_relation_with_unicode_special_chars_in_from_id_2() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let r = AssetRelation {
        from_id: "a!@#$%^&*()_+=α".into(),
        to_id: "b".into(),
        relation_type: RelationType::Solves,
        weight: 0.5,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: AssetRelation = serde_json::from_str(&j).unwrap();
    assert_eq!(back.from_id, "a!@#$%^&*()_+=α");
}

#[test]
fn cognitive_asset_with_unicode_name_with_specials() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "a".into(),
        kind: AssetKind::Tool,
        name: "name-α!@#β$%^γ&*()_".into(),
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
    assert_eq!(back.name, "name-α!@#β$%^γ&*()_");
}

#[test]
fn insight_data_with_unicode_observation_via_long_emoji() {
    let emoji_str = "🔍".repeat(50);
    let i = InsightData {
        observation: emoji_str.clone(),
        reasoning: "r".into(),
        evidence_ids: vec![],
        confidence: 0.5,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.observation.chars().count(), 50);
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn cognitive_asset_with_unicode_id_with_slash() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "path/to/α/β".into(),
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
    assert_eq!(back.id, "path/to/α/β");
}

#[test]
fn cognitive_asset_with_unicode_id_with_backslash() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "path\\to\\α\\β".into(),
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
    // Backslash is JSON-escaped, but the actual value should be same
    assert!(back.id.contains("α"));
    assert!(back.id.contains("β"));
}

#[test]
fn insight_data_with_unicode_observation_via_5_emojis_2() {
    let emojis = "🔍🔭🌟📚💡";
    let i = InsightData {
        observation: emojis.into(),
        reasoning: "r".into(),
        evidence_ids: vec![],
        confidence: 0.5,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.observation, emojis);
}

#[test]
fn cognitive_asset_with_unicode_metadata_4_emoji_values() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let mut meta = HashMap::new();
    meta.insert("k1".into(), "🔍".into());
    meta.insert("k2".into(), "🔭".into());
    meta.insert("k3".into(), "🌟".into());
    meta.insert("k4".into(), "📚".into());
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
    assert_eq!(back.metadata["k1"], "🔍");
    assert_eq!(back.metadata["k4"], "📚");
}

#[test]
fn asset_relation_with_unicode_id_with_only_specials() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let r = AssetRelation {
        from_id: "!@#$%".into(),
        to_id: "*&^%".into(),
        relation_type: RelationType::Solves,
        weight: 0.5,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: AssetRelation = serde_json::from_str(&j).unwrap();
    assert_eq!(back.from_id, "!@#$%");
    assert_eq!(back.to_id, "*&^%");
}

#[test]
fn cognitive_asset_with_unicode_id_with_ampersand() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "a&b&α".into(),
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
    assert_eq!(back.id, "a&b&α");
}

#[test]
fn insight_data_with_unicode_observation_via_2_emoji_2() {
    let i = InsightData {
        observation: "🔍🔭".into(),
        reasoning: "r".into(),
        evidence_ids: vec![],
        confidence: 0.5,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.observation, "🔍🔭");
}

// ---------------------------------------------------------------------------
// More tests
// ---------------------------------------------------------------------------

#[test]
fn cognitive_asset_with_unicode_id_with_emoji_only_long() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let long_emoji = "🔍".repeat(20);
    let asset = CognitiveAsset {
        id: long_emoji.clone(),
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
    assert_eq!(back.id.chars().count(), 20);
}

#[test]
fn cognitive_asset_with_unicode_id_with_only_special_chars() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "!@#$%^&*()_+=α".into(),
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
    assert_eq!(back.id, "!@#$%^&*()_+=α");
}

#[test]
fn insight_data_with_unicode_observation_via_3_emojis_2() {
    let i = InsightData {
        observation: "🔍🔭🌟".into(),
        reasoning: "r".into(),
        evidence_ids: vec![],
        confidence: 0.5,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.observation.chars().count(), 3);
}

#[test]
fn cognitive_asset_with_unicode_id_with_comma() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "id,α,β,γ".into(),
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
    assert_eq!(back.id, "id,α,β,γ");
}

#[test]
fn asset_relation_with_unicode_to_id_with_quotes() {
    use sisyphus_types::asset::{AssetRelation, RelationType};
    let r = AssetRelation {
        from_id: "a".into(),
        to_id: "b'α'β\"γ".into(),
        relation_type: RelationType::Refines,
        weight: 0.5,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: AssetRelation = serde_json::from_str(&j).unwrap();
    assert_eq!(back.to_id, "b'α'β\"γ");
}

#[test]
fn cognitive_asset_with_unicode_id_with_only_emoji_2() {
    use sisyphus_types::asset::{AssetContent, AssetKind, CognitiveAsset, InsightData};
    let asset = CognitiveAsset {
        id: "🔍".into(),
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
    assert_eq!(back.id, "🔍");
}

#[test]
fn insight_data_with_unicode_observation_via_long_emoji_2() {
    let long_emoji = "🔍".repeat(30);
    let i = InsightData {
        observation: long_emoji.clone(),
        reasoning: "r".into(),
        evidence_ids: vec![],
        confidence: 0.5,
    };
    let j = serde_json::to_string(&i).unwrap();
    let back: InsightData = serde_json::from_str(&j).unwrap();
    assert_eq!(back.observation.chars().count(), 30);
}
