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
}