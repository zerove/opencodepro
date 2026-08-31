use anyhow::{Context, Result};

use crate::commands::*;

/// Handle team.
pub async fn handle_team(sub: &TeamCommands) -> Result<()> {
    match sub {
        TeamCommands::Run {
            goal,
            context,
            agents,
            max_concurrent,
            config,
            strategy,
        } => {
            // 加载配置文件 (如果提供)
            let mut team_config = if let Some(config_path) = config {
                sisyphus_tools_agent::team_config::TeamConfig::from_file(config_path)?
            } else {
                sisyphus_tools_agent::team_config::TeamConfig::default()
            };

            // CLI 参数覆盖配置文件
            if let Some(strategy_str) = strategy {
                team_config.strategy = strategy_str.parse().context("无效的执行策略")?;
            }

            // 将 agents 字符串解析并合并到配置
            if let Some(agents_str) = agents {
                let selected: std::collections::HashSet<String> =
                    agents_str.split(',').map(|s| s.trim().to_string()).collect();

                // 禁用未选中的内置代理
                for agent in &mut team_config.agents {
                    if !selected.contains(&agent.id) {
                        agent.enabled = false;
                    }
                }
            }

            // 设置目标和上下文到 metadata
            team_config.metadata.description = Some(goal.clone());
            if let Some(ctx) = context {
                team_config.metadata.description = Some(format!("{}\n\n上下文: {}", goal, ctx));
            }

            // 运行团队
            sisyphus_cli::delegate::run_team_with_config(
                goal.clone(),
                context.clone(),
                agents.clone(),
                *max_concurrent,
                team_config,
            )
            .await?;
        },
        TeamCommands::List => {
            // 列出所有内置代理
            let agents = sisyphus_tools_agent::agent_registry::AgentRegistry::builtin_agents();
            println!("可用内置代理:");
            for (id, info) in agents {
                println!("  {id} - {}", info.description);
            }
        },
        TeamCommands::History { limit } => {
            // 显示执行历史（从本地存储读取）
            println!("团队执行历史 (最近 {limit} 条):");
            println!("  (暂未实现持久化存储，后续版本将支持)");
        },
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
                config,
                strategy,
            }) => {
                assert_eq!(goal, "fix bugs");
                assert_eq!(context, Some("project X".to_string()));
                assert_eq!(agents, Some("3".to_string()));
                assert_eq!(max_concurrent, 2);
                assert_eq!(config, None);
                assert_eq!(strategy, None);
            },
            _ => panic!("Expected Team/Run variant"),
        }
    }

    #[test]
    fn test_team_commands_parse_run_with_config() {
        let cmd = parse_commands(&[
            "team",
            "run",
            "fix bugs",
            "--config",
            "team.yaml",
            "--strategy",
            "sequential",
        ]);
        match cmd {
            Commands::Team(TeamCommands::Run {
                goal,
                config,
                strategy,
                ..
            }) => {
                assert_eq!(goal, "fix bugs");
                assert_eq!(config, Some(std::path::PathBuf::from("team.yaml")));
                assert_eq!(strategy, Some("sequential".to_string()));
            },
            _ => panic!("Expected Team/Run variant"),
        }
    }

    #[test]
    fn test_team_commands_parse_list() {
        let cmd = parse_commands(&["team", "list"]);
        match cmd {
            Commands::Team(TeamCommands::List) => {},
            _ => panic!("Expected Team/List variant"),
        }
    }

    #[test]
    fn test_team_commands_parse_history() {
        let cmd = parse_commands(&["team", "history", "--limit", "5"]);
        match cmd {
            Commands::Team(TeamCommands::History { limit }) => {
                assert_eq!(limit, 5);
            },
            _ => panic!("Expected Team/History variant"),
        }
    }
}
