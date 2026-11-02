use std::io::{Read, Write};
use std::path::PathBuf;

use anyhow::Result;

/// Handle exec.
pub async fn handle_exec(
    command: &str,
    dir: &Option<PathBuf>,
    cli_perms: &sisyphus_cli::permission::CliPermission,
) -> Result<()> {
    // 权限检查
    match cli_perms.check("exec") {
        Some(true) => {},
        Some(false) => {
            anyhow::bail!("Permission denied: shell execution is blocked by config");
        },
        None => {
            eprint!("⚠  Allow shell execution? [Y/n] ");
            std::io::stderr().flush().ok();
            let mut buf = String::new();
            std::io::stdin().read_line(&mut buf).ok();
            let answer = buf.trim().to_lowercase();
            if !answer.is_empty() && answer != "y" && answer != "yes" {
                anyhow::bail!("Shell execution denied by user");
            }
        },
    }
    let dir_ref = dir.as_deref();
    let output = sisyphus_cli::tools::bash::execute(command, dir_ref, 30000).await?;
    if !output.stdout.is_empty() {
        println!("{}", output.stdout);
    }
    if !output.stderr.is_empty() {
        eprintln!("{}", output.stderr);
    }
    println!(
        "Exit: {} ({}ms)",
        output.exit_code,
        output.duration.as_millis()
    );
    Ok(())
}

/// Handle search.
pub async fn handle_search(query: &str, num: usize) -> Result<()> {
    let results = sisyphus_cli::tools::web::websearch(query, num).await?;
    for (i, r) in results.iter().enumerate() {
        println!("[{i}] {} -- {}", r.title, r.url);
        // 按字符数安全截断 snippet，避免字节索引切在 UTF-8 多字节字符边界上引发 panic。
        let snippet: String = r.snippet.chars().take(120).collect();
        println!("    {snippet}");
    }
    Ok(())
}

use std::path::Path;

/// Handle apply patch.
pub fn handle_apply_patch(file: &Path, input: &Path) -> Result<()> {
    let patch_content = if input.to_string_lossy() == "-" {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        std::fs::read_to_string(input)?
    };
    let modified = sisyphus_cli::tools::patch::apply_patch(file, &patch_content)?;
    for f in &modified {
        println!("Patched: {f}");
    }
    Ok(())
}
