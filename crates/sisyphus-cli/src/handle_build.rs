use anyhow::Result;

use crate::commands::*;

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
    use crate::commands::BuildCommands;
    use clap::Parser;

    #[test]
    fn test_build_commands_parse_test() {
        let cmd = BuildCommands::try_parse_from(["test", "-p", "sisyphus-core"]);
        assert!(cmd.is_ok());
        match cmd.unwrap() {
            BuildCommands::Test { package, all, .. } => {
                assert_eq!(package, Some("sisyphus-core".to_string()));
                assert!(!all);
            }
            _ => panic!("Expected Test variant"),
        }
    }

    #[test]
    fn test_build_commands_parse_check_all() {
        let cmd = BuildCommands::try_parse_from(["check", "--all"]);
        assert!(cmd.is_ok());
        match cmd.unwrap() {
            BuildCommands::Check { all, .. } => {
                assert!(all);
            }
            _ => panic!("Expected Check variant"),
        }
    }

    #[test]
    fn test_build_commands_parse_fmt_check() {
        let cmd = BuildCommands::try_parse_from(["fmt", "--check"]);
        assert!(cmd.is_ok());
        match cmd.unwrap() {
            BuildCommands::Fmt { check, .. } => {
                assert!(check);
            }
            _ => panic!("Expected Fmt variant"),
        }
    }

    #[test]
    fn test_build_commands_parse_lint() {
        let cmd = BuildCommands::try_parse_from(["lint", "-p", "sisyphus-core"]);
        assert!(cmd.is_ok());
        match cmd.unwrap() {
            BuildCommands::Lint { package, .. } => {
                assert_eq!(package, Some("sisyphus-core".to_string()));
            }
            _ => panic!("Expected Lint variant"),
        }
    }
}
