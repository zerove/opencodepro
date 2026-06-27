use crate::{AgentInfo, AgentRegistry};
use std::path::Path;

/// A parsed agent definition from AGENTS.md.
#[derive(Debug, Clone)]
pub struct ProjectAgentDef {
    pub agent_id: String,
    pub display_name: String,
    pub description: String,
    pub instructions: Vec<String>,
    pub tool_prefixes: Vec<String>,
}

/// Result of loading project rules.
#[derive(Debug, Clone)]
pub struct ProjectRules {
    /// Path to the AGENTS.md file that was loaded.
    pub source: Option<String>,
    /// Project-defined agents.
    pub agents: Vec<ProjectAgentDef>,
}

/// Find and load AGENTS.md from the given project root.
///
/// Searches in order:
/// 1. `<root>/AGENTS.md`
/// 2. `<root>/.opencode/AGENTS.md`
/// 3. `<root>/docs/AGENTS.md`
pub fn load_project_rules(root: &Path) -> ProjectRules {
    let candidates = [
        root.join("AGENTS.md"),
        root.join(".opencode").join("AGENTS.md"),
        root.join("docs").join("AGENTS.md"),
    ];

    for candidate in &candidates {
        if candidate.exists() {
            match std::fs::read_to_string(candidate) {
                Ok(content) => {
                    let agents = parse_agents_md(&content);
                    return ProjectRules {
                        source: Some(candidate.to_string_lossy().to_string()),
                        agents,
                    };
                }
                Err(_) => continue,
            }
        }
    }

    ProjectRules {
        source: None,
        agents: vec![],
    }
}

/// Register all project-defined agents from a ProjectRules into an AgentRegistry.
pub fn register_project_agents(rules: &ProjectRules, registry: &AgentRegistry) -> usize {
    let mut count = 0;
    for def in &rules.agents {
        registry.register(
            &def.agent_id,
            AgentInfo {
                display_name: def.display_name.clone(),
                description: def.description.clone(),
                tool_prefixes: def.tool_prefixes.clone(),
                max_concurrency: 4,
            },
        );
        count += 1;
    }
    count
}

/// Parse AGENTS.md content into a list of agent definitions.
///
/// Expected format:
/// ```markdown
/// ## agent: my-custom-tool
/// Description of this custom agent.
///
/// ### Instructions
/// - Always do X
/// - Prefer Y over Z
///
/// ### Tools
/// - tool_prefix_a/
/// - tool_prefix_b/
/// ```
fn parse_agents_md(content: &str) -> Vec<ProjectAgentDef> {
    let mut agents: Vec<ProjectAgentDef> = Vec::new();
    let mut current: Option<ProjectAgentDef> = None;
    let mut in_instructions = false;
    let mut in_tools = false;

    for line in content.lines() {
        let trimmed = line.trim();

        // Detect agent headers: ## agent: <id> or ## Agent: <id>
        if let Some(rest) = trimmed.strip_prefix("## ").and_then(|s| {
            s.strip_prefix("agent: ")
                .or_else(|| s.strip_prefix("Agent: "))
        }) {
            // Save previous
            if let Some(prev) = current.take() {
                agents.push(prev);
            }
            let agent_id = rest.split_whitespace().next().unwrap_or(rest).to_string();
            let display_name = to_title(&agent_id);
            current = Some(ProjectAgentDef {
                agent_id,
                display_name,
                description: String::new(),
                instructions: Vec::new(),
                tool_prefixes: Vec::new(),
            });
            in_instructions = false;
            in_tools = false;
            continue;
        }

        if let Some(ref mut agent) = current {
            // Detect subsection headers
            if trimmed.starts_with("### ") {
                let header = trimmed.trim_start_matches("### ").to_lowercase();
                in_instructions = header == "instructions";
                in_tools = header == "tools";
                continue;
            }

            // First non-header, non-empty line after agent header → description
            if agent.description.is_empty()
                && !trimmed.is_empty()
                && !trimmed.starts_with('#')
                && !trimmed.starts_with('-')
            {
                agent.description = trimmed.to_string();
                continue;
            }

            // Bullet items in sections
            if in_instructions {
                if let Some(text) = trimmed.strip_prefix("- ") {
                    agent.instructions.push(text.to_string());
                }
            } else if in_tools {
                if let Some(prefix) = trimmed.strip_prefix("- ") {
                    agent
                        .tool_prefixes
                        .push(prefix.trim_end_matches('/').to_string() + "/");
                }
            }
        }
    }

    // Save last agent
    if let Some(agent) = current {
        agents.push(agent);
    }

    agents
}

fn to_title(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize = true;
    for ch in s.chars() {
        if ch == '-' || ch == '_' {
            result.push(' ');
            capitalize = true;
        } else if capitalize {
            for c in ch.to_uppercase() {
                result.push(c);
            }
            capitalize = false;
        } else {
            result.push(ch);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_agent() {
        let md = "\
## agent: my-tool
A custom tool for doing things.

### Instructions
- Always check input
- Log errors

### Tools
- custom/
- db/
";
        let agents = parse_agents_md(md);
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].agent_id, "my-tool");
        assert_eq!(agents[0].display_name, "My Tool");
        assert_eq!(agents[0].description, "A custom tool for doing things.");
        assert_eq!(agents[0].instructions.len(), 2);
        assert_eq!(agents[0].tool_prefixes, vec!["custom/", "db/"]);
    }

    #[test]
    fn test_parse_multiple_agents() {
        let md = "\
## agent: code-reviewer
Reviews pull requests.

### Tools
- momus/

## agent: doc-writer
Writes documentation.

### Tools
- scribe/
";
        let agents = parse_agents_md(md);
        assert_eq!(agents.len(), 2);
        assert_eq!(agents[0].agent_id, "code-reviewer");
        assert_eq!(agents[1].agent_id, "doc-writer");
        assert_eq!(agents[1].tool_prefixes, vec!["scribe/"]);
    }

    #[test]
    fn test_parse_empty_content() {
        let agents = parse_agents_md("");
        assert!(agents.is_empty());
    }

    #[test]
    fn test_parse_no_agents() {
        let md = "# Just a regular README\n\nNo agents defined here.\n";
        let agents = parse_agents_md(md);
        assert!(agents.is_empty());
    }

    #[test]
    fn test_parse_agent_without_sections() {
        let md = "\
## agent: minimal
Just a description, no sections.
";
        let agents = parse_agents_md(md);
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].agent_id, "minimal");
        assert_eq!(agents[0].description, "Just a description, no sections.");
        assert!(agents[0].instructions.is_empty());
        assert!(agents[0].tool_prefixes.is_empty());
    }

    #[test]
    fn test_register_project_agents() {
        use crate::llm::DynamicLlmClient;
        use sisyphus_types::Config;
        use std::sync::{Arc, RwLock};

        let config = Arc::new(RwLock::new(Config::default()));
        let llm = Arc::new(DynamicLlmClient::new(config));
        let registry = AgentRegistry::new(llm);

        let rules = ProjectRules {
            source: Some("/tmp/AGENTS.md".into()),
            agents: vec![ProjectAgentDef {
                agent_id: "my-agent".into(),
                display_name: "My Agent".into(),
                description: "Test agent".into(),
                instructions: vec![],
                tool_prefixes: vec!["test/".into()],
            }],
        };

        let count = register_project_agents(&rules, &registry);
        assert_eq!(count, 1);
        assert!(registry.has("my-agent"));
        let info = registry.get("my-agent").unwrap();
        assert_eq!(info.display_name, "My Agent");
        assert_eq!(info.tool_prefixes, vec!["test/"]);
    }

    #[test]
    fn test_to_title() {
        assert_eq!(to_title("my-custom-tool"), "My Custom Tool");
        assert_eq!(to_title("hello_world"), "Hello World");
        assert_eq!(to_title("simple"), "Simple");
    }

    #[test]
    fn test_load_project_rules_no_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let rules = load_project_rules(dir.path());
        assert!(rules.source.is_none());
        assert!(rules.agents.is_empty());
    }

    #[test]
    fn test_load_project_rules_with_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let agents_md = dir.path().join("AGENTS.md");
        std::fs::write(
            &agents_md,
            "\
## agent: local-dev
Local development helper.

### Tools
- bash/
- file/
",
        )
        .unwrap();

        let rules = load_project_rules(dir.path());
        assert!(rules.source.is_some());
        assert_eq!(rules.agents.len(), 1);
        assert_eq!(rules.agents[0].agent_id, "local-dev");
        assert_eq!(rules.agents[0].tool_prefixes, vec!["bash/", "file/"]);
    }

    #[test]
    fn test_parse_agent_with_case_insensitive_header() {
        let md = "\
## Agent: Capitalized
Description with capital-A header.

### Instructions
- Step one

### Tools
- magic/
";
        let agents = parse_agents_md(md);
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].agent_id, "Capitalized");
    }
}
