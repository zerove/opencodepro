use std::io::Write;
use std::path::Path;

use anyhow::Result;

use crate::commands::*;

/// Handle todo.
pub fn handle_todo(sub: &TodoCommands, todo_db: &Path) -> Result<()> {
    match sub {
        TodoCommands::Add { content, priority } => {
            let item = sisyphus_cli::tools::todo_add(todo_db, content, priority.clone().into())?;
            println!("Created: {} ({})", item.id, item.status);
        }
        TodoCommands::List { status } => {
            let filter = status
                .as_deref()
                .and_then(|s| s.parse::<sisyphus_cli::tools::TodoStatus>().ok());
            let items = sisyphus_cli::tools::todo_list(todo_db, filter)?;
            if items.is_empty() {
                println!("No todos found.");
            } else {
                let rows: Vec<Vec<String>> = items
                    .iter()
                    .map(|item| {
                        vec![
                            short_id(&item.id),
                            item.content.clone(),
                            item.status.to_string(),
                            item.priority.to_string(),
                        ]
                    })
                    .collect();
                print_table(&["ID", "Content", "Status", "Priority"], &rows);
            }
        }
        TodoCommands::Update {
            id,
            content,
            status,
            priority,
        } => {
            let s = status
                .as_deref()
                .and_then(|s| s.parse::<sisyphus_cli::tools::TodoStatus>().ok());
            let p = priority.clone().map(|p| p.into());
            let item = sisyphus_cli::tools::todo_update(todo_db, id, content.as_deref(), s, p)?;
            println!("Updated: {} — {}", item.id, item.status);
        }
        TodoCommands::Delete { id, yes: true } => {
            sisyphus_cli::tools::todo_delete(todo_db, id)?;
            println!("Deleted: {id}");
        }
        TodoCommands::Delete { id, yes: false } => {
            eprint!("Delete todo {id}? [y/N] ");
            std::io::stdout().flush()?;
            let mut answer = String::new();
            std::io::stdin().read_line(&mut answer)?;
            let answer = answer.trim().to_lowercase();
            if answer == "y" || answer == "yes" {
                sisyphus_cli::tools::todo_delete(todo_db, id)?;
                println!("Deleted: {id}");
            } else {
                println!("Cancelled.");
            }
        }
    }
    Ok(())
}
