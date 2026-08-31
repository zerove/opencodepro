use crate::commands::WorkspaceCommands;
use anyhow::Result;
use sisyphus_bridge_api::{
    CreateWorkspaceRequest, DeleteWorkspaceRequest, GetCurrentWorkspaceRequest,
    ListWorkspacesRequest, SwitchWorkspaceRequest,
};

/// Handle workspace.
pub async fn handle_workspace(sub: &WorkspaceCommands, bridge_addr: &str) -> Result<()> {
    let mut client = sisyphus_bridge_api::connect(bridge_addr).await?;

    match sub {
        WorkspaceCommands::Create {
            name,
            project_id,
            root_dir,
        } => {
            let request = CreateWorkspaceRequest {
                name: name.clone(),
                project_id: project_id.clone(),
                root_dir: root_dir.clone(),
            };
            let response = client.create_workspace(request).await?.into_inner();
            if response.success {
                println!(
                    "✅ Created workspace: {} (ID: {})",
                    name, response.workspace_id
                );
            } else {
                eprintln!("❌ Failed to create workspace: {}", response.error);
                std::process::exit(1);
            }
        },
        WorkspaceCommands::Switch { workspace_id } => {
            let request = SwitchWorkspaceRequest {
                workspace_id: workspace_id.clone(),
            };
            let response = client.switch_workspace(request).await?.into_inner();
            if response.success {
                println!("✅ Switched to workspace: {}", workspace_id);
            } else {
                eprintln!("❌ Failed to switch workspace: {}", response.error);
                std::process::exit(1);
            }
        },
        WorkspaceCommands::List => {
            let request = ListWorkspacesRequest {};
            let response = client.list_workspaces(request).await?.into_inner();
            if response.workspaces.is_empty() {
                println!("No workspaces found.");
            } else {
                println!("{:<40} {:<20} {:<20} Root Dir", "ID", "Name", "Project ID");
                println!("{}", "-".repeat(100));
                for ws in response.workspaces {
                    println!(
                        "{:<40} {:<20} {:<20} {}",
                        ws.id, ws.name, ws.project_id, ws.root_dir
                    );
                }
            }
        },
        WorkspaceCommands::Current => {
            let request = GetCurrentWorkspaceRequest {};
            let response = client.get_current_workspace(request).await?.into_inner();
            if response.found {
                if let Some(ws) = response.workspace {
                    println!("Current workspace:");
                    println!("  ID:        {}", ws.id);
                    println!("  Name:      {}", ws.name);
                    println!("  Project:   {}", ws.project_id);
                    println!("  Root Dir:  {}", ws.root_dir);
                }
            } else {
                println!("No active workspace. Use 'sisyphus workspace create' to create one.");
            }
        },
        WorkspaceCommands::Delete { workspace_id, yes } => {
            if !yes {
                print!("Delete workspace \"{}\"? [y/N] ", workspace_id);
                use std::io::Write;
                std::io::stdout().flush()?;
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("Aborted.");
                    return Ok(());
                }
            }
            let request = DeleteWorkspaceRequest {
                workspace_id: workspace_id.clone(),
            };
            let response = client.delete_workspace(request).await?.into_inner();
            if response.success {
                println!("✅ Deleted workspace: {}", workspace_id);
            } else {
                eprintln!("❌ Failed to delete workspace: {}", response.error);
                std::process::exit(1);
            }
        },
    }

    Ok(())
}
