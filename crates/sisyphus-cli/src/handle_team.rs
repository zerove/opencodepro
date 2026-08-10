use anyhow::Result;

use crate::commands::*;

pub async fn handle_team(sub: &TeamCommands) -> Result<()> {
    match sub {
        TeamCommands::Run {
            goal,
            context,
            agents,
            max_concurrent,
        } => {
            sisyphus_cli::delegate::run_team(
                goal.clone(),
                context.clone(),
                agents.clone(),
                *max_concurrent,
            )
            .await?;
        }
        TeamCommands::List => {
            // 列出所有内置代理
            let agents = sisyphus_tools_agent::agent_registry::AgentRegistry::builtin_agents();
            println!("可用内置代理:");
            for (id, info) in agents {
                println!("  {id} - {}", info.description);
            }
        }
        TeamCommands::History { limit } => {
            // 显示执行历史（从本地存储读取）
            println!("团队执行历史 (最近 {limit} 条):");
            println!("  (暂未实现持久化存储，后续版本将支持)");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::parse_commands;
    use crate::commands::{Commands, TeamCommands};

    #[test]
    fn test_team_commands_parse_run() {
        let cmd = parse_commands(&[
            "team",
            "run",
            "fix bugs",
            "--context",
            "project X",
            "--agents",
            "3",
            "--concurrent",
            "2",
        ]);
        match cmd {
            Commands::Team(TeamCommands::Run {
                goal,
                context,
                agents,
                max_concurrent,
            }) => {
                assert_eq!(goal, "fix bugs");
                assert_eq!(context, Some("project X".to_string()));
                assert_eq!(agents, Some("3".to_string()));
                assert_eq!(max_concurrent, 2);
            }
            _ => panic!("Expected Team/Run variant"),
        }
    }

    #[test]
    fn test_team_commands_parse_list() {
        let cmd = parse_commands(&["team", "list"]);
        match cmd {
            Commands::Team(TeamCommands::List) => {}
            _ => panic!("Expected Team/List variant"),
        }
    }

    #[test]
    fn test_team_commands_parse_history() {
        let cmd = parse_commands(&["team", "history", "--limit", "5"]);
        match cmd {
            Commands::Team(TeamCommands::History { limit }) => {
                assert_eq!(limit, 5);
            }
            _ => panic!("Expected Team/History variant"),
        }
    }
}
