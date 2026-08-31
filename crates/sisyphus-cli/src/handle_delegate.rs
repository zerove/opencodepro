use anyhow::Result;

use crate::commands::*;

/// Handle delegate.
pub async fn handle_delegate(sub: &DelegateCommands) -> Result<()> {
    match sub {
        DelegateCommands::Run {
            file,
            max_concurrent,
        } => {
            sisyphus_cli::delegate::run_tasks(file.clone(), *max_concurrent).await?;
        },
        DelegateCommands::Inline {
            commands,
            max_concurrent,
        } => {
            sisyphus_cli::delegate::run_inline(commands, *max_concurrent).await?;
        },
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
        },
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::parse_commands;
    use crate::commands::{Commands, DelegateCommands};

    #[test]
    fn test_delegate_commands_parse_run() {
        let cmd = parse_commands(&["delegate", "run", "tasks.json", "--concurrent", "3"]);
        match cmd {
            Commands::Delegate(DelegateCommands::Run { max_concurrent, .. }) => {
                assert_eq!(max_concurrent, 3);
            },
            _ => panic!("Expected Delegate/Run variant"),
        }
    }

    #[test]
    fn test_delegate_commands_parse_inline() {
        let cmd = parse_commands(&["delegate", "inline", "echo hello"]);
        match cmd {
            Commands::Delegate(DelegateCommands::Inline { commands, .. }) => {
                assert_eq!(commands, &["echo hello"]);
            },
            _ => panic!("Expected Delegate/Inline variant"),
        }
    }

    #[test]
    fn test_delegate_commands_parse_team() {
        let cmd = parse_commands(&[
            "delegate",
            "team",
            "fix bugs",
            "--context",
            "project X",
            "--agents",
            "3",
        ]);
        match cmd {
            Commands::Delegate(DelegateCommands::Team {
                goal,
                context,
                agents,
                max_concurrent,
            }) => {
                assert_eq!(goal, "fix bugs");
                assert_eq!(context, Some("project X".to_string()));
                assert_eq!(agents, Some("3".to_string()));
                // 未指定时 --concurrent 默认为 4
                assert_eq!(max_concurrent, 4);
            },
            _ => panic!("Expected Delegate/Team variant"),
        }
    }
}
