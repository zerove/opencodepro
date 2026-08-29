//! Integration tests for sisyphus-tools-session `skill::types` module's
//! pure data types: `Skill`, `SkillSource`, default version, and `priority()`.

use sisyphus_tools_session::skill::{Skill, SkillSource};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// SkillSource
// ---------------------------------------------------------------------------

#[test]
fn skill_source_default_is_builtin() {
    let s: SkillSource = Default::default();
    assert_eq!(s, SkillSource::BuiltIn);
}

#[test]
fn skill_source_equality() {
    assert_eq!(SkillSource::BuiltIn, SkillSource::BuiltIn);
    assert_eq!(SkillSource::User, SkillSource::User);
    assert_eq!(SkillSource::Project, SkillSource::Project);
    assert_ne!(SkillSource::BuiltIn, SkillSource::User);
    assert_ne!(SkillSource::User, SkillSource::Project);
}

#[test]
fn skill_source_priority_ordering() {
    // Project should have the highest priority, then User, then BuiltIn
    assert!(SkillSource::Project.priority() > SkillSource::User.priority());
    assert!(SkillSource::User.priority() > SkillSource::BuiltIn.priority());
}

#[test]
fn skill_source_priority_values() {
    assert_eq!(SkillSource::Project.priority(), 30);
    assert_eq!(SkillSource::User.priority(), 20);
    assert_eq!(SkillSource::BuiltIn.priority(), 10);
}

#[test]
fn skill_source_serde() {
    for s in [SkillSource::BuiltIn, SkillSource::User, SkillSource::Project] {
        let j = serde_json::to_string(&s).unwrap();
        let back: SkillSource = serde_json::from_str(&j).unwrap();
        assert_eq!(back, s);
    }
}

// ---------------------------------------------------------------------------
// Skill
// ---------------------------------------------------------------------------

#[test]
fn skill_field_assignment() {
    let s = Skill {
        name: "my-skill".into(),
        description: "Does X".into(),
        version: "1.0.0".into(),
        author: Some("alice".into()),
        tags: vec!["test".into(), "demo".into()],
        triggers: vec![],
        instructions: "Do something useful".into(),
        source: SkillSource::Project,
        source_path: Some(PathBuf::from("/skills/my-skill.md")),
    };
    assert_eq!(s.name, "my-skill");
    assert_eq!(s.description, "Does X");
    assert_eq!(s.version, "1.0.0");
    assert_eq!(s.author.as_deref(), Some("alice"));
    assert_eq!(s.tags.len(), 2);
    assert!(s.triggers.is_empty());
    assert_eq!(s.source, SkillSource::Project);
}

#[test]
fn skill_default_version() {
    // When deserializing without version, default is "0.1.0"
    let s: Skill = serde_json::from_value(serde_json::json!({
        "name": "x",
        "description": "y"
    }))
    .unwrap();
    assert_eq!(s.version, "0.1.0");
    assert!(s.author.is_none());
    assert!(s.tags.is_empty());
}

#[test]
fn skill_with_explicit_version() {
    let s: Skill = serde_json::from_value(serde_json::json!({
        "name": "x",
        "description": "y",
        "version": "2.5.0",
        "author": "bob",
        "tags": ["a", "b"]
    }))
    .unwrap();
    assert_eq!(s.version, "2.5.0");
    assert_eq!(s.author.as_deref(), Some("bob"));
    assert_eq!(s.tags, vec!["a", "b"]);
}

#[test]
fn skill_source_skipped_in_serde() {
    let s = Skill {
        name: "x".into(),
        description: "y".into(),
        version: "1.0".into(),
        author: None,
        tags: vec![],
        triggers: vec![],
        instructions: "body".into(),
        source: SkillSource::User,
        source_path: Some(PathBuf::from("/x.md")),
    };
    let j = serde_json::to_string(&s).unwrap();
    // source and source_path are skipped
    assert!(!j.contains("\"source\""));
    assert!(!j.contains("User"));
    assert!(!j.contains("source_path"));
    assert!(!j.contains("\"instructions\""));
    // But name + description + version are there
    assert!(j.contains("\"name\":\"x\""));
    assert!(j.contains("\"description\":\"y\""));
    assert!(j.contains("\"version\":\"1.0\""));
}

#[test]
fn skill_clone() {
    let s = Skill {
        name: "x".into(),
        description: "y".into(),
        version: "1.0".into(),
        author: Some("a".into()),
        tags: vec!["t".into()],
        triggers: vec![],
        instructions: "i".into(),
        source: SkillSource::BuiltIn,
        source_path: None,
    };
    let b = s.clone();
    assert_eq!(s.name, b.name);
    assert_eq!(s.description, b.description);
    assert_eq!(s.tags, b.tags);
    assert_eq!(s.source, b.source);
}

#[test]
fn skill_debug() {
    let s = Skill {
        name: "debug-skill".into(),
        description: "d".into(),
        version: "0.1".into(),
        author: None,
        tags: vec![],
        triggers: vec![],
        instructions: "i".into(),
        source: SkillSource::BuiltIn,
        source_path: None,
    };
    let formatted = format!("{s:?}");
    assert!(formatted.contains("debug-skill"));
}
