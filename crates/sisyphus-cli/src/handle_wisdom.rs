use anyhow::Result;

use crate::commands::WisdomCommands;

pub(crate) async fn handle_wisdom(sub: &WisdomCommands) -> Result<()> {
    match sub {
        WisdomCommands::Record { category, content } => {
            sisyphus_tools::wisdom_record(category, content).await?;
            println!("Recorded wisdom in category: {category}");
        }
        WisdomCommands::List => {
            let categories = sisyphus_tools::wisdom_list().await?;
            if categories.is_empty() {
                println!("No wisdom categories found.");
            } else {
                println!("Available wisdom categories:");
                for cat in &categories {
                    println!("  {cat}");
                }
            }
        }
        WisdomCommands::Show { category } => {
            let content = sisyphus_tools::wisdom_get(category).await?;
            println!("--- Wisdom: {} ---\n{content}", category);
        }
        WisdomCommands::Delete { category } => {
            sisyphus_tools::wisdom_delete(category).await?;
            println!("Deleted wisdom category: {category}");
        }
        WisdomCommands::Inject => {
            let context = sisyphus_tools::wisdom_inject().await?;
            if context.is_empty() {
                println!("No wisdom to inject.");
            } else {
                println!("{context}");
            }
        }
    }
    Ok(())
}
