use anyhow::Result;

use crate::commands::*;

pub async fn handle_delegate(sub: &DelegateCommands) -> Result<()> {
    match sub {
        DelegateCommands::Run {
            file,
            max_concurrent,
        } => {
            sisyphus_cli::delegate::run_tasks(file.clone(), *max_concurrent).await?;
        }
        DelegateCommands::Inline {
            commands,
            max_concurrent,
        } => {
            sisyphus_cli::delegate::run_inline(commands, *max_concurrent).await?;
        }
        DelegateCommands::Team {
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
    use crate::commands::DelegateCommands;
    use clap::Parser;

    #[test]
    fn test_delegate_commands_parse_run() {
        let cmd = DelegateCommands::try_parse_from(["run", "tasks.json", "--max-concurrent", "3"]);
        assert!(cmd.is_ok());
        match cmd.unwrap() {
            DelegateCommands::Run { max_concurrent, .. } => {
                assert_eq!(max_concurrent, 3);
            }
            _ => panic!("Expected Run variant"),
        }
    }

    #[test]
    fn test_delegate_commands_parse_inline() {
        let cmd = DelegateCommands::try_parse_from(["inline", "echo hello"]);
        assert!(cmd.is_ok());
        match cmd.unwrap() {
            DelegateCommands::Inline { commands, .. } => {
                assert_eq!(commands, &["echo hello"]);
            }
            _ => panic!("Expected Inline variant"),
        }
    }

    #[test]
    fn test_delegate_commands_parse_team() {
        let cmd = DelegateCommands::try_parse_from([
            "team",
            "fix bugs",
            "--context",
            "project X",
            "--agents",
            "3",
        ]);
        assert!(cmd.is_ok());
        match cmd.unwrap() {
            DelegateCommands::Team {
                goal,
                context,
                agents,
                max_concurrent,
            } => {
                assert_eq!(goal, "fix bugs");
                assert_eq!(context, Some("project X".to_string()));
                assert_eq!(agents, 3);
                assert_eq!(max_concurrent, 3);
            }
            _ => panic!("Expected Team variant"),
        }
    }
}
