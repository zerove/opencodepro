use anyhow::Result;

use crate::commands::*;

pub async fn handle_lsp(sub: &LspCommands) -> Result<()> {
    match sub {
        LspCommands::Diagnostics {
            file,
            language_id,
            server,
            server_args,
            root_uri,
        } => {
            sisyphus_cli::lsp::run_diagnostics(file, language_id, server, server_args, root_uri)
                .await?;
        }
        LspCommands::GotoDef {
            file,
            line,
            col,
            language_id,
            server,
            server_args,
            root_uri,
        } => {
            sisyphus_cli::lsp::run_goto_def(
                file,
                *line,
                *col,
                language_id,
                server,
                server_args,
                root_uri,
            )
            .await?;
        }
        LspCommands::FindRefs {
            file,
            line,
            col,
            language_id,
            server,
            server_args,
            root_uri,
        } => {
            sisyphus_cli::lsp::run_find_refs(
                file,
                *line,
                *col,
                language_id,
                server,
                server_args,
                root_uri,
            )
            .await?;
        }
        LspCommands::Hover {
            file,
            line,
            col,
            language_id,
            server,
            server_args,
            root_uri,
        } => {
            sisyphus_cli::lsp::run_hover(
                file,
                *line,
                *col,
                language_id,
                server,
                server_args,
                root_uri,
            )
            .await?;
        }
        LspCommands::Symbols {
            file,
            language_id,
            server,
            server_args,
            root_uri,
        } => {
            sisyphus_cli::lsp::run_symbols(file, language_id, server, server_args, root_uri)
                .await?;
        }
    }
    Ok(())
}
