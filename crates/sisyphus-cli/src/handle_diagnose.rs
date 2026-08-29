use anyhow::Result;

use crate::commands::*;

/// Handle diagnose.
pub fn handle_diagnose(sub: &DiagnoseCommands) -> Result<()> {
    match sub {
        DiagnoseCommands::Run {
            lang,
            package,
            all,
            no_lint,
            cwd,
        } => sisyphus_cli::diagnose::run_diagnose(
            lang.clone(),
            package.clone(),
            *all,
            *no_lint,
            cwd.clone(),
        )?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::parse_commands;
    use crate::commands::{Commands, DiagnoseCommands};

    #[test]
    fn test_diagnose_commands_parse() {
        let cmd = parse_commands(&["diagnose", "run", "--package", "sisyphus-core"]);
        match cmd {
            Commands::Diagnose(DiagnoseCommands::Run {
                package,
                all,
                no_lint,
                ..
            }) => {
                assert_eq!(package, Some("sisyphus-core".to_string()));
                assert!(!all);
                assert!(!no_lint);
            }
            _ => panic!("Expected Diagnose/Run variant"),
        }
    }

    #[test]
    fn test_diagnose_commands_parse_all_no_lint() {
        let cmd = parse_commands(&["diagnose", "run", "--all", "--no-lint"]);
        match cmd {
            Commands::Diagnose(DiagnoseCommands::Run { all, no_lint, .. }) => {
                assert!(all);
                assert!(no_lint);
            }
            _ => panic!("Expected Diagnose/Run variant"),
        }
    }
}
