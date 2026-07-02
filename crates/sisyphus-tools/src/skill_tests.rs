use super::*;

#[test]
fn test_parse_frontmatter_empty() {
    let (meta, body) = parse_frontmatter("just some text");
    assert_eq!(meta.name, None);
    assert_eq!(body, "just some text");
}

#[test]
fn test_parse_frontmatter_full() {
    let content = r#"---
name: my-test-skill
description: A test skill
version: "1.0.0"
author: test-user
tags: [rust, testing]
triggers:
  - tools: ["metis_analyse"]
    description: Activated during metis analysis
---

This is the instruction body.
"#;
    let (meta, body) = parse_frontmatter(content);
    assert_eq!(meta.name, Some("my-test-skill".into()));
    assert_eq!(meta.description, Some("A test skill".into()));
    assert_eq!(meta.version, "1.0.0");
    assert_eq!(meta.author, Some("test-user".into()));
    assert_eq!(meta.tags, vec!["rust", "testing"]);
    assert_eq!(meta.triggers.len(), 1);
    assert_eq!(meta.triggers[0].tools, vec!["metis_analyse"]);
    assert_eq!(
        meta.triggers[0].description,
        Some("Activated during metis analysis".into())
    );
    assert!(body.contains("instruction body"));
}

#[test]
fn test_parse_frontmatter_no_frontmatter() {
    let content = "plain markdown body";
    let (meta, body) = parse_frontmatter(content);
    assert_eq!(meta.name, None);
    assert_eq!(body, "plain markdown body");
}

#[test]
fn test_priority_ordering() {
    assert!(SkillSource::Project.priority() > SkillSource::User.priority());
    assert!(SkillSource::User.priority() > SkillSource::BuiltIn.priority());
}

#[test]
fn test_render_instructions() {
    let skill = Skill {
        name: "test".into(),
        description: "".into(),
        version: "0.1.0".into(),
        author: None,
        tags: vec![],
        triggers: vec![],
        instructions: "Hello {{name}}, your tool is {{tool}}".into(),
        source: SkillSource::BuiltIn,
        source_path: None,
    };
    let mut vars = HashMap::new();
    vars.insert("name".into(), "Sisyphus".into());
    vars.insert("tool".into(), "metis".into());
    let result = render_instructions(&skill, &vars);
    assert_eq!(result, "Hello Sisyphus, your tool is metis");
}

#[test]
fn test_find_by_tool() {
    let skills = vec![
        Skill {
            name: "metis-helper".into(),
            description: "".into(),
            version: "0.1.0".into(),
            author: None,
            tags: vec![],
            triggers: vec![TriggerDef {
                pattern: None,
                tools: vec!["metis_analyse".into()],
                description: None,
                hook_phase: None,
                hook_effect: None,
                hook_message: None,
            }],
            instructions: "metis instructions".into(),
            source: SkillSource::BuiltIn,
            source_path: None,
        },
        Skill {
            name: "generic".into(),
            description: "".into(),
            version: "0.1.0".into(),
            author: None,
            tags: vec![],
            triggers: vec![],
            instructions: "generic".into(),
            source: SkillSource::BuiltIn,
            source_path: None,
        },
    ];
    let loader = SkillLoader::new();
    let matched = loader.find_by_tool("metis_analyse", &skills);
    assert_eq!(matched.len(), 2); // both match: one by trigger, one by empty triggers
}

#[test]
fn test_parse_frontmatter_only_opening_delimiter() {
    let content = "---\nno closing delimiter here";
    let (meta, body) = parse_frontmatter(content);
    assert_eq!(meta.name, None);
    assert_eq!(body, content.trim());
}

#[test]
fn test_parse_frontmatter_empty_delimiters() {
    let content = "---\n---\nbody text";
    let (meta, body) = parse_frontmatter(content);
    assert_eq!(meta.name, None);
    assert!(body.contains("body text"));
}

#[test]
fn test_parse_frontmatter_malformed_yaml() {
    let content = "---\nname: [unclosed list\n---\nbody";
    let (meta, body) = parse_frontmatter(content);
    assert_eq!(meta.name, None);
    assert!(
        body.contains("body"),
        "should fall back to treating entire content as body"
    );
}

#[test]
fn test_parse_frontmatter_extra_fields() {
    let content = r#"---
name: extra-fields
unknown_field: should_be_ignored
nested:
  a: 1
---
body
"#;
    let (meta, body) = parse_frontmatter(content);
    assert_eq!(meta.name, Some("extra-fields".into()));
    assert!(body.contains("body"));
}

#[test]
fn test_render_instructions_no_vars_match() {
    let skill = Skill {
        name: "t".into(),
        description: "".into(),
        version: "0.1.0".into(),
        author: None,
        tags: vec![],
        triggers: vec![],
        instructions: "Hello {{name}}".into(),
        source: SkillSource::BuiltIn,
        source_path: None,
    };
    let vars = HashMap::new();
    let result = render_instructions(&skill, &vars);
    assert_eq!(result, "Hello {{name}}");
}

#[test]
fn test_render_instructions_empty() {
    let skill = Skill {
        name: "t".into(),
        description: "".into(),
        version: "0.1.0".into(),
        author: None,
        tags: vec![],
        triggers: vec![],
        instructions: "".into(),
        source: SkillSource::BuiltIn,
        source_path: None,
    };
    let vars = HashMap::new();
    let result = render_instructions(&skill, &vars);
    assert_eq!(result, "");
}

#[test]
fn test_find_by_tool_no_matching_tool() {
    let skills = vec![Skill {
        name: "specific".into(),
        description: "".into(),
        version: "0.1.0".into(),
        author: None,
        tags: vec![],
        triggers: vec![TriggerDef {
            pattern: None,
            tools: vec!["bash_exec".into()],
            description: None,
            hook_phase: None,
            hook_effect: None,
            hook_message: None,
        }],
        instructions: "".into(),
        source: SkillSource::BuiltIn,
        source_path: None,
    }];
    let loader = SkillLoader::new();
    let matched = loader.find_by_tool("metis_analyse", &skills);
    assert!(matched.is_empty(), "should not match different tool name");
}

#[test]
fn test_find_by_tool_empty_skills() {
    let loader = SkillLoader::new();
    let matched = loader.find_by_tool("anything", &[]);
    assert!(matched.is_empty());
}

#[test]
fn test_load_all_empty_dir() {
    let dir = tempfile::TempDir::new().unwrap();
    let loader = SkillLoader::new().with_dir(dir.path().to_path_buf());
    let skills = loader.load_all();
    assert!(skills.is_empty(), "empty dir should produce no skills");
}

#[test]
fn test_load_all_nonexistent_dir() {
    let loader =
        SkillLoader::new().with_dir(PathBuf::from("/tmp/__sisyphus_nonexistent_skills__"));
    let skills = loader.load_all();
    assert!(
        skills.is_empty(),
        "non-existent dir should be silently skipped"
    );
}

#[test]
fn test_classify_source_variants() {
    let user_dir = Path::new("/home/user/.config/sisyphus/skills");
    let project_dir = Path::new("/some/project/.sisyphus/skills");
    let builtin_dir = Path::new("/usr/share/sisyphus/skills");

    assert_eq!(classify_source(user_dir), SkillSource::User);
    assert_eq!(classify_source(project_dir), SkillSource::Project);
    assert_eq!(classify_source(builtin_dir), SkillSource::BuiltIn);
}

#[test]
fn test_load_all_priority_override() {
    let dir = tempfile::TempDir::new().unwrap();
    let builtin_dir = dir.path().join("share").join("skills");
    let project_dir = dir.path().join("project").join(".sisyphus").join("skills");
    fs::create_dir_all(&builtin_dir).unwrap();
    fs::create_dir_all(&project_dir).unwrap();

    fs::write(
        builtin_dir.join("test-skill.md"),
        "---\nname: test-skill\n---\nlow priority instructions",
    )
    .unwrap();
    fs::write(
        project_dir.join("test-skill.md"),
        "---\nname: test-skill\n---\nhigh priority instructions",
    )
    .unwrap();

    let loader = SkillLoader::new()
        .with_builtin_dir(builtin_dir)
        .with_dir(project_dir);
    let skills = loader.load_all();
    assert_eq!(skills.len(), 1);
    assert!(
        skills[0].instructions.contains("high priority"),
        "higher priority skill should override: got '{}'",
        skills[0].instructions
    );
}
