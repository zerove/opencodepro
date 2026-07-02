use anyhow::Result;

use crate::commands::HashlineCommands;

pub(crate) fn handle_hashline(sub: &HashlineCommands) -> Result<()> {
    match sub {
        HashlineCommands::Annotate { path } => {
            let count = sisyphus_cli::tools::hashline::annotate_file(path)?;
            println!("Annotated {} lines in {}", count, path.display());
        }
        HashlineCommands::Verify { path } => {
            let content = std::fs::read_to_string(path)?;
            let mut valid = 0;
            let mut invalid = 0;

            for (i, line) in content.lines().enumerate() {
                if sisyphus_cli::tools::hashline::parse_hashline(line).is_some() {
                    if sisyphus_cli::tools::hashline::verify_line(&content, line).unwrap_or(false) {
                        valid += 1;
                    } else {
                        invalid += 1;
                        eprintln!("Invalid hashline at line {}: {}", i + 1, line.trim());
                    }
                }
            }
            println!("Verification complete: {} valid, {} invalid", valid, invalid);
            if invalid > 0 {
                std::process::exit(1);
            }
        }
        HashlineCommands::Edit {
            path,
            hashline,
            new_content,
        } => {
            let old =
                sisyphus_cli::tools::hashline::edit_with_hashline(path, hashline, new_content)?;
            println!("Successfully edited line. Old content: '{}'", old);
        }
    }
    Ok(())
}
