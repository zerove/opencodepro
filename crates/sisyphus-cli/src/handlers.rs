use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};

use crate::commands::*;
use crate::Cli;

// ── Handler modules ────────────────────────────────────────

#[path = "handle_build.rs"]
mod handle_build;
#[path = "handle_daemon.rs"]
mod handle_daemon;
#[path = "handle_delegate.rs"]
mod handle_delegate;
#[path = "handle_dev.rs"]
mod handle_dev;
#[path = "handle_diagnose.rs"]
mod handle_diagnose;
#[path = "handle_exec.rs"]
mod handle_exec;
#[path = "handle_explore.rs"]
mod handle_explore;
#[path = "handle_file.rs"]
mod handle_file;
#[path = "handle_fix.rs"]
mod handle_fix;
#[path = "handle_git.rs"]
mod handle_git;
#[path = "handle_github.rs"]
mod handle_github;
#[path = "handle_hashline.rs"]
mod handle_hashline;
#[path = "handle_init.rs"]
mod handle_init;
#[path = "handle_librarian.rs"]
mod handle_librarian;
#[path = "handle_lsp.rs"]
mod handle_lsp;
#[path = "handle_mcp.rs"]
mod handle_mcp;
#[path = "handle_metis.rs"]
mod handle_metis;
#[path = "handle_momus.rs"]
mod handle_momus;
#[path = "handle_oracle.rs"]
mod handle_oracle;
#[path = "handle_plan.rs"]
mod handle_plan;
#[path = "handle_plugin.rs"]
mod handle_plugin;
#[path = "handle_pr.rs"]
mod handle_pr;
#[path = "handle_precommit.rs"]
mod handle_precommit;
#[path = "handle_provider.rs"]
mod handle_provider;
#[path = "handle_remote.rs"]
mod handle_remote;
#[path = "handle_rules.rs"]
mod handle_rules;
#[path = "handle_run.rs"]
mod handle_run;
#[path = "handle_session.rs"]
mod handle_session;
#[path = "handle_skill.rs"]
mod handle_skill;
#[path = "handle_sop.rs"]
mod handle_sop;
#[path = "handle_todo.rs"]
mod handle_todo;
#[path = "handle_tool.rs"]
mod handle_tool;
#[path = "handle_wisdom.rs"]
mod handle_wisdom;

#[path = "handle_workspace.rs"]
mod handle_workspace;

#[cfg(test)]
#[path = "test_helpers.rs"]
mod test_helpers;

// ── Public dispatch ──────────────────────────────────────────

pub async fn dispatch(
    cli: &Cli,
    sisyphus_cfg: &sisyphus_types::Config,
    todo_db: &Path,
    session_db: &Path,
    cli_perms: &sisyphus_cli::permission::CliPermission,
    llm_client: Arc<dyn sisyphus_cli::tools::metis::LlmClient>,
    bridge_addr: &str,
) -> Result<()> {
    match cli.command.as_ref().context("No command specified")? {
        // ── file tools ──────────────────────────────────
        Commands::Read { path } => handle_file::handle_read(path)?,
        Commands::ReadLines {
            path,
            offset,
            limit,
        } => handle_file::handle_read_lines(path, *offset, *limit)?,
        Commands::Write { path, content } => handle_file::handle_write(path, content)?,
        Commands::Edit {
            path,
            old_string,
            new_string,
            all,
        } => handle_file::handle_edit(path, old_string, new_string, *all)?,
        Commands::Glob {
            pattern,
            root,
            limit,
        } => handle_file::handle_glob(pattern, root, *limit)?,
        Commands::Grep {
            pattern,
            path,
            include,
        } => handle_file::handle_grep(pattern, path, include.as_deref())?,
        Commands::Exec { command, dir } => {
            handle_exec::handle_exec(command, dir, cli_perms).await?
        }
        Commands::Search { query, num } => handle_exec::handle_search(query, *num).await?,
        Commands::ApplyPatch { file, input } => handle_exec::handle_apply_patch(file, input)?,

        // ── todo ────────────────────────────────────────
        Commands::Todo(sub) => handle_todo::handle_todo(sub, todo_db)?,

        // ── session ─────────────────────────────────────
        Commands::Session(sub) => handle_session::handle_session(sub, session_db).await?,

        // ── plan ────────────────────────────────────────
        Commands::Plan(sub) => handle_plan::handle_plan(sub)?,

        // ── oracle ──────────────────────────────────────
        Commands::Oracle(sub) => handle_oracle::handle_oracle(sub)?,

        // ── explore ─────────────────────────────────────
        Commands::Explore(sub) => handle_explore::handle_explore(sub)?,

        // ── librarian ───────────────────────────────────
        Commands::Librarian(sub) => {
            handle_librarian::handle_librarian(sub, llm_client.clone()).await?
        }

        // ── momus ────────────────────────────────────────
        Commands::Momus(sub) => handle_momus::handle_momus(sub, &llm_client).await?,

        // ── metis ────────────────────────────────────────
        Commands::Metis(sub) => handle_metis::handle_metis(sub, llm_client.clone()).await?,

        // ── skill ───────────────────────────────────────
        Commands::Skill(sub) => handle_skill::handle_skill(sub)?,

        // ── rules ────────────────────────────────────────
        Commands::Rules(sub) => handle_rules::handle_rules(sub)?,

        // ── lsp ─────────────────────────────────────────
        Commands::Lsp(sub) => handle_lsp::handle_lsp(sub).await?,

        // ── delegate ────────────────────────────────────
        Commands::Delegate(sub) => handle_delegate::handle_delegate(sub).await?,

        // ── mcp ─────────────────────────────────────────
        Commands::Mcp(sub) => handle_mcp::handle_mcp(sub, llm_client.clone()).await?,

        // ── engine ──────────────────────────────────────
        Commands::Sop(sub) => handle_sop::handle_sop(sub, bridge_addr).await?,

        // ── git ─────────────────────────────────────────
        Commands::Git(sub) => handle_git::handle_git(sub)?,

        // ── build ────────────────────────────────────────
        Commands::Build(sub) => handle_build::handle_build(sub)?,

        // ── run ────────────────────────────────────────────
        Commands::Run(sub) => handle_run::handle_run(sub)?,

        // ── init ───────────────────────────────────────────
        Commands::Init(sub) => handle_init::handle_init(sub)?,

        // ── precommit ──────────────────────────────────────
        Commands::Precommit(sub) => handle_precommit::handle_precommit(sub)?,

        // ── pr ─────────────────────────────────────────────
        Commands::Pr(sub) => handle_pr::handle_pr(sub, &llm_client).await?,

        // ── wisdom ─────────────────────────────────────────
        Commands::Wisdom(sub) => handle_wisdom::handle_wisdom(sub).await?,

        // ── hashline ───────────────────────────────────────
        Commands::Hashline(sub) => handle_hashline::handle_hashline(sub)?,

        // ── diagnose ──────────────────────────────────────
        Commands::Diagnose(sub) => handle_diagnose::handle_diagnose(sub)?,

        // ── fix ────────────────────────────────────────────
        Commands::Fix(sub) => handle_fix::handle_fix(sub)?,

        // ── github ─────────────────────────────────────────
        Commands::Github(sub) => handle_github::handle_github(sub)?,

        // ── provider ───────────────────────────────────────
        Commands::Provider(sub) => handle_provider::handle_provider(sub, sisyphus_cfg)?,

        // ── tool ───────────────────────────────────────────
        Commands::Tool(sub) => handle_tool::handle_tool(sub, cli, sisyphus_cfg)?,

        // ── plugin / remote / health / status ──────────────
        Commands::Plugin(sub) => handle_plugin::handle_plugin(sub, bridge_addr).await?,
        Commands::LearningInsights { strategy } => {
            handle_remote::handle_learning_insights(bridge_addr, strategy).await?
        }
        Commands::Health => handle_remote::handle_health(bridge_addr).await?,
        Commands::Status => handle_remote::handle_status(bridge_addr).await?,
        Commands::Remote(sub) => handle_remote::handle_remote(sub, bridge_addr).await?,

        // ── daemon ─────────────────────────────────────────
        Commands::Daemon => handle_daemon::handle_daemon()?,
        // ── workspace ───────────────────────────────────────
        Commands::Workspace(sub) => handle_workspace::handle_workspace(sub, bridge_addr).await?,

        // ── dev ────────────────────────────────────────────
        Commands::Dev {
            goal,
            context,
            parallel,
            resume,
        } => handle_dev::handle_dev(goal, context, *parallel, *resume, sisyphus_cfg).await?,
    }

    Ok(())
}
