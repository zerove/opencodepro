use std::io::Write;
use std::path::Path;

use anyhow::Result;

use crate::commands::SessionCommands;
use crate::commands::{print_table, short_id};

pub(crate) async fn handle_session(sub: &SessionCommands, session_db: &Path) -> Result<()> {
    match sub {
        SessionCommands::Create { name } => {
            let session = sisyphus_tools::session_create(session_db, name)?;
            println!("Created session: {} ({})", session.id, session.name);
        }
        SessionCommands::List => {
            let sessions = sisyphus_tools::session_list(session_db)?;
            if sessions.is_empty() {
                println!("No sessions.");
            } else {
                let rows: Vec<Vec<String>> = sessions
                    .iter()
                    .map(|s| {
                        vec![
                            short_id(&s.id),
                            s.name.clone(),
                            format!("{} msgs", s.message_count),
                        ]
                    })
                    .collect();
                print_table(&["ID", "Name", "Messages"], &rows);
            }
        }
        SessionCommands::Get { id } => {
            let session = sisyphus_tools::session_get(session_db, id)?;
            println!("Session: {} ({})", session.id, session.name);
            for msg in &session.messages {
                println!("  [{}] {}", msg.role, msg.content);
            }
        }
        SessionCommands::Delete { id, yes: true } => {
            sisyphus_tools::session_delete(session_db, id)?;
            println!("Deleted session: {id}");
        }
        SessionCommands::Delete { id, yes: false } => {
            eprint!("Delete session {id}? [y/N] ");
            std::io::stdout().flush()?;
            let mut answer = String::new();
            std::io::stdin().read_line(&mut answer)?;
            let answer = answer.trim().to_lowercase();
            if answer == "y" || answer == "yes" {
                sisyphus_tools::session_delete(session_db, id)?;
                println!("Deleted session: {id}");
            } else {
                println!("Cancelled.");
            }
        }
        SessionCommands::AddMessage {
            id,
            role,
            content,
        } => {
            let session =
                sisyphus_tools::session_add_message(session_db, id, role, content)?;
            println!(
                "Message added to {} ({} msgs)",
                session.id, session.message_count
            );
        }
        SessionCommands::Rename { id, name } => {
            let session = sisyphus_tools::session_rename(session_db, id, name)?;
            println!("Renamed session {} to {}", session.id, session.name);
        }
        SessionCommands::Fork { id, name } => {
            let session = sisyphus_tools::session_fork(session_db, id, name)?;
            println!(
                "Forked session {} from {} — {} messages",
                session.id, id, session.message_count
            );
        }
        SessionCommands::Continue { id } => {
            let session = sisyphus_tools::session_continue(session_db, id)?;
            println!(
                "Continued session {} — {} messages",
                session.id, session.message_count
            );
        }
        SessionCommands::Undo { id } => {
            let session = sisyphus_tools::session_undo(session_db, id)?;
            println!(
                "Undid last message — session {} now has {} messages",
                session.id, session.message_count
            );
        }
        SessionCommands::Export {
            id,
            output,
            format,
        } => {
            match format.as_str() {
                "html" => sisyphus_tools::session_export_html(session_db, id, output)?,
                "json" => sisyphus_tools::session_export(session_db, id, output)?,
                other => anyhow::bail!(
                    "unsupported export format '{other}'; use 'json' or 'html'"
                ),
            }
            println!(
                "Exported session {} to {} ({})",
                id,
                output.display(),
                format
            );
        }
        SessionCommands::Import { input } => {
            let session = sisyphus_tools::session_import(session_db, input)?;
            println!("Imported session {} ({})", session.id, session.name);
        }
        SessionCommands::Compress { id, threshold } => {
            let strategy = match threshold {
                Some(t) => sisyphus_tools::CompressStrategy::Threshold(*t),
                None => sisyphus_tools::CompressStrategy::default(),
            };
            let session = sisyphus_tools::session_compress(session_db, id, &strategy)?;
            println!(
                "Compressed session {} — now has {} messages",
                session.id, session.message_count
            );
        }
    }
    Ok(())
}
