use anyhow::Result;

use crate::commands::*;

pub fn handle_rules(sub: &RulesCommands) -> Result<()> {
    match sub {
        RulesCommands::Show => {
            let root = std::env::current_dir()?;
            let rules = sisyphus_tools::load_project_rules(&root);
            match &rules.source {
                Some(src) => println!("Project rules loaded from: {src}"),
                None => println!(
                    "No AGENTS.md found (checked: AGENTS.md, .opencode/AGENTS.md, docs/AGENTS.md)"
                ),
            }
            if rules.agents.is_empty() {
                println!("No project-defined agents.");
            } else {
                println!("Project-defined agents ({}):", rules.agents.len());
                for a in &rules.agents {
                    println!("  - {} ({})", a.agent_id, a.display_name);
                    println!("    description: {}", a.description);
                    if !a.tool_prefixes.is_empty() {
                        println!("    tools: {}", a.tool_prefixes.join(", "));
                    }
                }
            }
        }
        RulesCommands::Reload => {
            let root = std::env::current_dir()?;
            let rules = sisyphus_tools::load_project_rules(&root);
            match &rules.source {
                Some(src) => {
                    let count = rules.agents.len();
                    println!("Reloaded {count} project agent(s) from {src}");
                }
                None => println!("No AGENTS.md found."),
            }
        }
    }
    Ok(())
}
