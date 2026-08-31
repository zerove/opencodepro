//! Integration tests for sisyphus-tools-agent `project_rules`.
//!
//! Exercises the public `ProjectRules` / `ProjectAgentDef` types and
//! `load_project_rules` / `register_project_agents` functions, which
//! parse AGENTS.md files into structured agent definitions and register
//! them with an `AgentRegistry`.

use sisyphus_tools_agent::project_rules::{
    load_project_rules, register_project_agents, ProjectAgentDef, ProjectRules,
};
use std::fs;
use tempfile::TempDir;

fn write_agents_md(dir: &std::path::Path, rel: &str, body: &str) -> std::path::PathBuf {
    let p = dir.join(rel);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).expect("mkdir");
    }
    fs::write(&p, body).expect("write");
    p
}

#[test]
fn load_returns_empty_when_no_agents_md() {
    let dir = TempDir::new().expect("tempdir");
    let rules = load_project_rules(dir.path());
    assert!(rules.source.is_none());
    assert!(rules.agents.is_empty());
}

#[test]
fn load_finds_agents_md_at_root() {
    let dir = TempDir::new().expect("tempdir");
    write_agents_md(
        dir.path(),
        "AGENTS.md",
        "## agent: my-agent\nMy custom agent.\n",
    );
    let rules = load_project_rules(dir.path());
    assert!(rules.source.is_some());
    let src = rules.source.unwrap();
    assert!(src.ends_with("AGENTS.md"));
    assert_eq!(rules.agents.len(), 1);
    assert_eq!(rules.agents[0].agent_id, "my-agent");
    assert_eq!(rules.agents[0].display_name, "My Agent");
    assert_eq!(rules.agents[0].description, "My custom agent.");
}

#[test]
fn load_finds_agents_md_in_opencode_subdir() {
    let dir = TempDir::new().expect("tempdir");
    write_agents_md(
        dir.path(),
        ".opencode/AGENTS.md",
        "## agent: second\nSecond agent.\n",
    );
    let rules = load_project_rules(dir.path());
    assert!(rules.source.unwrap().contains(".opencode"));
    assert_eq!(rules.agents.len(), 1);
    assert_eq!(rules.agents[0].agent_id, "second");
}

#[test]
fn load_finds_agents_md_in_docs_subdir() {
    let dir = TempDir::new().expect("tempdir");
    write_agents_md(
        dir.path(),
        "docs/AGENTS.md",
        "## agent: third\nThird agent.\n",
    );
    let rules = load_project_rules(dir.path());
    assert!(rules.source.unwrap().contains("docs"));
    assert_eq!(rules.agents[0].agent_id, "third");
}

#[test]
fn load_prefers_root_over_subdirs() {
    // When multiple candidates exist, the first one wins.
    let dir = TempDir::new().expect("tempdir");
    write_agents_md(
        dir.path(),
        "AGENTS.md",
        "## agent: root-agent\nRoot wins.\n",
    );
    write_agents_md(
        dir.path(),
        ".opencode/AGENTS.md",
        "## agent: hidden-agent\nShould not be picked.\n",
    );
    let rules = load_project_rules(dir.path());
    assert_eq!(rules.agents.len(), 1);
    assert_eq!(rules.agents[0].agent_id, "root-agent");
}

#[test]
fn load_parses_instructions_section() {
    let dir = TempDir::new().expect("tempdir");
    write_agents_md(
        dir.path(),
        "AGENTS.md",
        "\
## agent: foo
A foo agent.

### Instructions
- Always validate input
- Log errors carefully
",
    );
    let rules = load_project_rules(dir.path());
    let a = &rules.agents[0];
    assert_eq!(a.instructions.len(), 2);
    assert_eq!(a.instructions[0], "Always validate input");
    assert_eq!(a.instructions[1], "Log errors carefully");
}

#[test]
fn load_parses_tools_section() {
    let dir = TempDir::new().expect("tempdir");
    write_agents_md(
        dir.path(),
        "AGENTS.md",
        "\
## agent: foo
A foo agent.

### Tools
- custom/
- db/
",
    );
    let rules = load_project_rules(dir.path());
    let a = &rules.agents[0];
    assert_eq!(a.tool_prefixes.len(), 2);
    assert_eq!(a.tool_prefixes[0], "custom/");
    assert_eq!(a.tool_prefixes[1], "db/");
}

#[test]
fn load_tool_prefixes_strip_trailing_slash_for_dedup() {
    // "db/" should be normalized to "db/" with consistent trailing slash.
    // The parser trims and re-adds the slash, so "db//" should also become "db/".
    let dir = TempDir::new().expect("tempdir");
    write_agents_md(
        dir.path(),
        "AGENTS.md",
        "\
## agent: foo
Foo.

### Tools
- db
",
    );
    let rules = load_project_rules(dir.path());
    // "db" without trailing slash → "db/" after the parser's trim_end_matches('/') + "/".
    assert_eq!(rules.agents[0].tool_prefixes, vec!["db/".to_string()]);
}

#[test]
fn load_parses_multiple_agents_in_one_file() {
    let dir = TempDir::new().expect("tempdir");
    write_agents_md(
        dir.path(),
        "AGENTS.md",
        "\
## agent: first
First agent description.

### Instructions
- One

## agent: second
Second agent description.

### Tools
- x/
",
    );
    let rules = load_project_rules(dir.path());
    assert_eq!(rules.agents.len(), 2);
    assert_eq!(rules.agents[0].agent_id, "first");
    assert_eq!(rules.agents[0].instructions, vec!["One".to_string()]);
    assert_eq!(rules.agents[1].agent_id, "second");
    assert_eq!(rules.agents[1].tool_prefixes, vec!["x/".to_string()]);
}

#[test]
fn load_handles_agent_header_with_capital_a() {
    let dir = TempDir::new().expect("tempdir");
    write_agents_md(
        dir.path(),
        "AGENTS.md",
        "## Agent: capitalized\nDescription.\n",
    );
    let rules = load_project_rules(dir.path());
    assert_eq!(rules.agents.len(), 1);
    assert_eq!(rules.agents[0].agent_id, "capitalized");
}

#[test]
fn load_handles_hyphenated_agent_id() {
    let dir = TempDir::new().expect("tempdir");
    write_agents_md(
        dir.path(),
        "AGENTS.md",
        "## agent: my-custom-tool\nDescription.\n",
    );
    let rules = load_project_rules(dir.path());
    // to_title converts hyphens to spaces and capitalizes.
    assert_eq!(rules.agents[0].display_name, "My Custom Tool");
}

// ---------------------------------------------------------------------------
// ProjectRules / ProjectAgentDef default Debug
// ---------------------------------------------------------------------------

#[test]
fn project_rules_debug_is_implemented() {
    let rules = ProjectRules {
        source: Some("AGENTS.md".into()),
        agents: vec![],
    };
    let s = format!("{rules:?}");
    assert!(s.contains("AGENTS.md"));
}

#[test]
fn project_agent_def_debug_is_implemented() {
    let def = ProjectAgentDef {
        agent_id: "a".into(),
        display_name: "A".into(),
        description: "desc".into(),
        instructions: vec!["i1".into()],
        tool_prefixes: vec!["t/".into()],
    };
    let s = format!("{def:?}");
    assert!(s.contains("a"));
    assert!(s.contains("desc"));
}

// ---------------------------------------------------------------------------
// register_project_agents
// ---------------------------------------------------------------------------

mod register {
    use super::*;
    use sisyphus_tools_agent::agent_registry::AgentRegistry;
    use sisyphus_tools_llm::DummyLlmClient;
    use std::sync::Arc;

    fn make_registry() -> AgentRegistry {
        AgentRegistry::new(Arc::new(DummyLlmClient))
    }

    #[test]
    fn register_returns_count_of_added_agents() {
        let rules = ProjectRules {
            source: Some("AGENTS.md".into()),
            agents: vec![
                ProjectAgentDef {
                    agent_id: "a".into(),
                    display_name: "A".into(),
                    description: "first".into(),
                    instructions: vec![],
                    tool_prefixes: vec![],
                },
                ProjectAgentDef {
                    agent_id: "b".into(),
                    display_name: "B".into(),
                    description: "second".into(),
                    instructions: vec![],
                    tool_prefixes: vec![],
                },
            ],
        };
        let registry = make_registry();
        let n = register_project_agents(&rules, &registry);
        assert_eq!(n, 2);
    }

    #[test]
    fn register_empty_returns_zero() {
        let rules = ProjectRules {
            source: None,
            agents: vec![],
        };
        let registry = make_registry();
        assert_eq!(register_project_agents(&rules, &registry), 0);
    }

    #[test]
    fn register_agents_are_queryable() {
        let rules = ProjectRules {
            source: Some("x".into()),
            agents: vec![ProjectAgentDef {
                agent_id: "x-agent".into(),
                display_name: "X Agent".into(),
                description: "for testing".into(),
                instructions: vec!["do X".into()],
                tool_prefixes: vec!["x/".into()],
            }],
        };
        let registry = make_registry();
        register_project_agents(&rules, &registry);

        assert!(registry.has("x-agent"));
        let info = registry.get("x-agent").expect("registered");
        assert_eq!(info.display_name, "X Agent");
        assert_eq!(info.description, "for testing");
        assert_eq!(info.tool_prefixes, vec!["x/".to_string()]);
    }

    #[test]
    fn register_loads_and_registers_integration() {
        // End-to-end: write AGENTS.md, load it, register into a registry,
        // and confirm both agents are queryable.
        let dir = TempDir::new().expect("tempdir");
        write_agents_md(
            dir.path(),
            "AGENTS.md",
            "\
## agent: alpha
Alpha description.

### Tools
- alpha/

## agent: beta
Beta description.

### Tools
- beta/
",
        );
        let rules = load_project_rules(dir.path());
        assert_eq!(rules.agents.len(), 2);

        let registry = make_registry();
        let n = register_project_agents(&rules, &registry);
        assert_eq!(n, 2);
        assert!(registry.has("alpha"));
        assert!(registry.has("beta"));
    }
}
