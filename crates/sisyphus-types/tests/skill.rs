//! Integration tests for sisyphus-types `skill` module.
//!
//! Exercises the public `SkillBundle` / `SkillMetadata` API used to
//! package cognitive skills (an SOP + required tools) for distribution.
//! `add_sub_sop` and `add_required_tool` are the mutation helpers.

use sisyphus_types::graph::SOPGraphConfig;
use sisyphus_types::skill::{SkillBundle, SkillMetadata};

fn meta(id: &str) -> SkillMetadata {
    SkillMetadata {
        id: id.to_string(),
        name: format!("{id}-name"),
        version: "1.0.0".to_string(),
        author: "test".to_string(),
        description: "test skill".to_string(),
        tags: vec!["test".to_string()],
        created_at: 1700000000,
    }
}

fn empty_sop() -> SOPGraphConfig {
    SOPGraphConfig::default()
}

// ---------------------------------------------------------------------------
// SkillBundle::new
// ---------------------------------------------------------------------------

#[test]
fn new_bundle_has_empty_sub_sops_and_tools() {
    let bundle = SkillBundle::new(meta("a"), empty_sop());
    assert!(bundle.sub_sops.is_empty());
    assert!(bundle.required_tools.is_empty());
    assert!(bundle.acceptance_criteria.is_empty());
    assert_eq!(bundle.metadata.id, "a");
    assert_eq!(bundle.main_sop.nodes.len(), 0);
    assert_eq!(bundle.main_sop.edges.len(), 0);
}

#[test]
fn new_bundle_preserves_metadata() {
    let m = meta("xyz");
    let bundle = SkillBundle::new(m.clone(), empty_sop());
    assert_eq!(bundle.metadata.id, m.id);
    assert_eq!(bundle.metadata.version, m.version);
    assert_eq!(bundle.metadata.author, m.author);
}

#[test]
fn new_bundle_preserves_main_sop() {
    let mut sop = SOPGraphConfig::default();
    sop.nodes.push(sisyphus_types::graph::SOPNodeConfig {
        name: "n1".into(),
        kind: sisyphus_types::NodeKind::Test {
            command: "echo".into(),
            workdir: None,
        },
    });
    let bundle = SkillBundle::new(meta("a"), sop);
    assert_eq!(bundle.main_sop.nodes.len(), 1);
    assert_eq!(bundle.main_sop.nodes[0].name, "n1");
}

// ---------------------------------------------------------------------------
// add_sub_sop
// ---------------------------------------------------------------------------

#[test]
fn add_sub_sop_inserts_into_map() {
    let mut bundle = SkillBundle::new(meta("a"), empty_sop());
    bundle.add_sub_sop("helper".into(), empty_sop());
    assert_eq!(bundle.sub_sops.len(), 1);
    assert!(bundle.sub_sops.contains_key("helper"));
}

#[test]
fn add_sub_sop_overwrites_same_id() {
    let mut bundle = SkillBundle::new(meta("a"), empty_sop());
    bundle.add_sub_sop("helper".into(), empty_sop());
    let mut new_sop = SOPGraphConfig::default();
    new_sop.nodes.push(sisyphus_types::graph::SOPNodeConfig {
        name: "n".into(),
        kind: sisyphus_types::NodeKind::Test {
            command: "ls".into(),
            workdir: None,
        },
    });
    bundle.add_sub_sop("helper".into(), new_sop);
    assert_eq!(bundle.sub_sops.len(), 1);
    assert_eq!(bundle.sub_sops.get("helper").unwrap().nodes.len(), 1);
}

#[test]
fn add_sub_sop_supports_multiple_keys() {
    let mut bundle = SkillBundle::new(meta("a"), empty_sop());
    bundle.add_sub_sop("k1".into(), empty_sop());
    bundle.add_sub_sop("k2".into(), empty_sop());
    bundle.add_sub_sop("k3".into(), empty_sop());
    assert_eq!(bundle.sub_sops.len(), 3);
}

// ---------------------------------------------------------------------------
// add_required_tool
// ---------------------------------------------------------------------------

#[test]
fn add_required_tool_appends_to_vec() {
    let mut bundle = SkillBundle::new(meta("a"), empty_sop());
    bundle.add_required_tool("tool1".into());
    bundle.add_required_tool("tool2".into());
    assert_eq!(
        bundle.required_tools,
        vec!["tool1".to_string(), "tool2".to_string()]
    );
}

#[test]
fn add_required_tool_dedups() {
    let mut bundle = SkillBundle::new(meta("a"), empty_sop());
    bundle.add_required_tool("t".into());
    bundle.add_required_tool("t".into());
    bundle.add_required_tool("t".into());
    assert_eq!(bundle.required_tools, vec!["t".to_string()]);
}

#[test]
fn add_required_tool_mixed_dedup() {
    let mut bundle = SkillBundle::new(meta("a"), empty_sop());
    bundle.add_required_tool("a".into());
    bundle.add_required_tool("b".into());
    bundle.add_required_tool("a".into()); // duplicate
    bundle.add_required_tool("c".into());
    assert_eq!(
        bundle.required_tools,
        vec!["a".to_string(), "b".to_string(), "c".to_string()]
    );
}

// ---------------------------------------------------------------------------
// Serialization roundtrip
// ---------------------------------------------------------------------------

#[test]
fn skill_bundle_serializes_to_json() {
    let mut bundle = SkillBundle::new(meta("json-test"), empty_sop());
    bundle.add_required_tool("tool1".into());
    let json = serde_json::to_string(&bundle).expect("serialize");
    assert!(json.contains("json-test"));
    assert!(json.contains("tool1"));
}

#[test]
fn skill_bundle_round_trips_through_json() {
    let mut original = SkillBundle::new(meta("rt"), empty_sop());
    original.add_sub_sop("sub1".into(), empty_sop());
    original.add_required_tool("t1".into());
    original.acceptance_criteria = "all green".into();

    let json = serde_json::to_string(&original).expect("serialize");
    let restored: SkillBundle = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(restored.metadata.id, "rt");
    assert_eq!(restored.sub_sops.len(), 1);
    assert_eq!(restored.required_tools, vec!["t1".to_string()]);
    assert_eq!(restored.acceptance_criteria, "all green");
}
