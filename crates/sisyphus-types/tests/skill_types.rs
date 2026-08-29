//! Integration tests for sisyphus-types `skill` module.
//!
//! Exercises the public surface of [`SkillMetadata`], [`SkillBundle`],
//! and the SkillBundle helpers.

use sisyphus_types::graph::SOPGraphConfig;
use sisyphus_types::skill::{SkillBundle, SkillMetadata};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// SkillMetadata
// ---------------------------------------------------------------------------

#[test]
fn skill_metadata_field_assignment() {
    let m = SkillMetadata {
        id: "sm-1".into(),
        name: "My Skill".into(),
        version: "1.0.0".into(),
        author: "me".into(),
        description: "d".into(),
        tags: vec!["k1".into()],
        created_at: 12345,
    };
    assert_eq!(m.id, "sm-1");
    assert_eq!(m.version, "1.0.0");
    assert_eq!(m.tags.len(), 1);
}

#[test]
fn skill_metadata_serde_round_trip() {
    let m = SkillMetadata {
        id: "sm-1".into(),
        name: "n".into(),
        version: "v".into(),
        author: "a".into(),
        description: "d".into(),
        tags: vec!["t".into()],
        created_at: 1,
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: SkillMetadata = serde_json::from_str(&j).unwrap();
    assert_eq!(back.id, "sm-1");
    assert_eq!(back.tags, vec!["t".to_string()]);
}

// ---------------------------------------------------------------------------
// SkillBundle
// ---------------------------------------------------------------------------

#[test]
fn skill_bundle_new_initializes() {
    let m = SkillMetadata {
        id: "sm".into(),
        name: "n".into(),
        version: "v".into(),
        author: "a".into(),
        description: "d".into(),
        tags: vec![],
        created_at: 0,
    };
    let main = SOPGraphConfig::default();
    let b = SkillBundle::new(m, main);
    assert_eq!(b.metadata.id, "sm");
    assert!(b.sub_sops.is_empty());
    assert!(b.required_tools.is_empty());
}

#[test]
fn skill_bundle_add_sub_sop() {
    let m = SkillMetadata {
        id: "sm".into(),
        name: "n".into(),
        version: "v".into(),
        author: "a".into(),
        description: "d".into(),
        tags: vec![],
        created_at: 0,
    };
    let mut b = SkillBundle::new(m, SOPGraphConfig::default());
    b.add_sub_sop("sub1".into(), SOPGraphConfig::default());
    assert!(b.sub_sops.contains_key("sub1"));
    assert_eq!(b.sub_sops.len(), 1);
}

#[test]
fn skill_bundle_add_multiple_sub_sops() {
    let m = SkillMetadata {
        id: "sm".into(),
        name: "n".into(),
        version: "v".into(),
        author: "a".into(),
        description: "d".into(),
        tags: vec![],
        created_at: 0,
    };
    let mut b = SkillBundle::new(m, SOPGraphConfig::default());
    b.add_sub_sop("a".into(), SOPGraphConfig::default());
    b.add_sub_sop("b".into(), SOPGraphConfig::default());
    b.add_sub_sop("c".into(), SOPGraphConfig::default());
    assert_eq!(b.sub_sops.len(), 3);
}

#[test]
fn skill_bundle_add_required_tool() {
    let m = SkillMetadata {
        id: "sm".into(),
        name: "n".into(),
        version: "v".into(),
        author: "a".into(),
        description: "d".into(),
        tags: vec![],
        created_at: 0,
    };
    let mut b = SkillBundle::new(m, SOPGraphConfig::default());
    b.add_required_tool("file_read".into());
    b.add_required_tool("bash".into());
    assert_eq!(b.required_tools.len(), 2);
    assert!(b.required_tools.contains(&"file_read".to_string()));
}

#[test]
fn skill_bundle_clone() {
    let m = SkillMetadata {
        id: "sm".into(),
        name: "n".into(),
        version: "v".into(),
        author: "a".into(),
        description: "d".into(),
        tags: vec![],
        created_at: 0,
    };
    let b = SkillBundle::new(m, SOPGraphConfig::default());
    let c = b.clone();
    assert_eq!(c.metadata.id, b.metadata.id);
}

#[test]
fn skill_bundle_serde_round_trip() {
    let m = SkillMetadata {
        id: "sm".into(),
        name: "n".into(),
        version: "v".into(),
        author: "a".into(),
        description: "d".into(),
        tags: vec![],
        created_at: 1,
    };
    let b = SkillBundle {
        metadata: m,
        main_sop: SOPGraphConfig::default(),
        sub_sops: HashMap::new(),
        required_tools: vec!["t1".into()],
        acceptance_criteria: "all green".into(),
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: SkillBundle = serde_json::from_str(&j).unwrap();
    assert_eq!(back.metadata.id, "sm");
    assert_eq!(back.required_tools, vec!["t1".to_string()]);
    assert_eq!(back.acceptance_criteria, "all green");
}
