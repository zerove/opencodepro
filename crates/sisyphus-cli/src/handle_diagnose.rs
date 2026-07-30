use anyhow::Result;

use crate::commands::*;

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
    use crate::commands::DiagnoseCommands;
    use clap::Parser;

    #[test]
    fn test_diagnose_commands_parse() {
        let cmd = DiagnoseCommands::try_parse_from(["run", "--package", "sisyphus-core"]);
        assert!(cmd.is_ok());
        match cmd.unwrap() {
            DiagnoseCommands::Run {
                package,
                all,
                no_lint,
                ..
            } => {
                assert_eq!(package, Some("sisyphus-core".to_string()));
                assert!(!all);
                assert!(!no_lint);
            }
        }
    }

    #[test]
    fn test_diagnose_commands_parse_all_no_lint() {
        let cmd = DiagnoseCommands::try_parse_from(["run", "--all", "--no-lint"]);
        assert!(cmd.is_ok());
        match cmd.unwrap() {
            DiagnoseCommands::Run { all, no_lint, .. } => {
                assert!(all);
                assert!(no_lint);
            }
        }
    }
}
