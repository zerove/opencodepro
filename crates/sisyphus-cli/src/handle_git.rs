use anyhow::Result;

use crate::commands::*;

/// Handle git.
pub fn handle_git(sub: &GitCommands) -> Result<()> {
    match sub {
        GitCommands::Status { cwd } => sisyphus_cli::git::run_status(cwd.clone())?,
        GitCommands::Diff {
            pathspec,
            cached,
            cwd,
        } => sisyphus_cli::git::run_diff(pathspec.clone(), *cached, cwd.clone())?,
        GitCommands::Add { pathspecs, cwd } => {
            sisyphus_cli::git::run_add(pathspecs.clone(), cwd.clone())?
        }
        GitCommands::Commit {
            message,
            allow_empty,
            cwd,
        } => sisyphus_cli::git::run_commit(message.clone(), *allow_empty, cwd.clone())?,
        GitCommands::Log { count, cwd } => sisyphus_cli::git::run_log(*count, cwd.clone())?,
        GitCommands::Branch { cwd } => sisyphus_cli::git::run_branch(cwd.clone())?,
        GitCommands::Checkout {
            branch,
            create,
            cwd,
        } => sisyphus_cli::git::run_checkout(branch.clone(), *create, cwd.clone())?,
        GitCommands::Push {
            remote,
            branch,
            force,
            cwd,
        } => sisyphus_cli::git::run_push(remote.clone(), branch.clone(), *force, cwd.clone())?,
        GitCommands::Pull {
            remote,
            branch,
            cwd,
        } => sisyphus_cli::git::run_pull(remote.clone(), branch.clone(), cwd.clone())?,
        GitCommands::Summary { cwd } => sisyphus_cli::git::run_summary(cwd.clone())?,
    }
    Ok(())
}
