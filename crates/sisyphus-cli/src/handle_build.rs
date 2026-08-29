use anyhow::Result;

use crate::commands::*;

/// Handle build.
pub fn handle_build(sub: &BuildCommands) -> Result<()> {
    match sub {
        BuildCommands::Check {
            package,
            all,
            lang,
            cwd,
        } => sisyphus_cli::build::run_check(package.clone(), *all, cwd.clone(), lang.clone())?,
        BuildCommands::Build {
            package,
            release,
            lang,
            cwd,
        } => sisyphus_cli::build::run_build(package.clone(), *release, cwd.clone(), lang.clone())?,
        BuildCommands::Test {
            package,
            name,
            all,
            lang,
            cwd,
        } => sisyphus_cli::build::run_test(
            package.clone(),
            name.clone(),
            *all,
            cwd.clone(),
            lang.clone(),
        )?,
        BuildCommands::Lint { package, lang, cwd } => {
            sisyphus_cli::build::run_lint(package.clone(), cwd.clone(), lang.clone())?
        }
        BuildCommands::Fmt { check, lang, cwd } => {
            sisyphus_cli::build::run_fmt(*check, cwd.clone(), lang.clone())?
        }
        BuildCommands::Doc { package, lang, cwd } => {
            sisyphus_cli::build::run_doc(package.clone(), cwd.clone(), lang.clone())?
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::parse_commands;
    use crate::commands::{BuildCommands, Commands};

    #[test]
    fn test_build_commands_parse_test() {
        let cmd = parse_commands(&["build", "test", "-p", "sisyphus-core"]);
        match cmd {
            Commands::Build(BuildCommands::Test { package, all, .. }) => {
                assert_eq!(package, Some("sisyphus-core".to_string()));
                assert!(!all);
            }
            _ => panic!("Expected Build/Test variant"),
        }
    }

    #[test]
    fn test_build_commands_parse_check_all() {
        let cmd = parse_commands(&["build", "check", "--all"]);
        match cmd {
            Commands::Build(BuildCommands::Check { all, .. }) => {
                assert!(all);
            }
            _ => panic!("Expected Build/Check variant"),
        }
    }

    #[test]
    fn test_build_commands_parse_fmt_check() {
        let cmd = parse_commands(&["build", "fmt", "--check"]);
        match cmd {
            Commands::Build(BuildCommands::Fmt { check, .. }) => {
                assert!(check);
            }
            _ => panic!("Expected Build/Fmt variant"),
        }
    }

    #[test]
    fn test_build_commands_parse_lint() {
        let cmd = parse_commands(&["build", "lint", "-p", "sisyphus-core"]);
        match cmd {
            Commands::Build(BuildCommands::Lint { package, .. }) => {
                assert_eq!(package, Some("sisyphus-core".to_string()));
            }
            _ => panic!("Expected Build/Lint variant"),
        }
    }
}
