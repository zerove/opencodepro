use std::path::Path;

use anyhow::{Context, Result};

pub(crate) fn handle_daemon() -> Result<()> {
    println!("Starting Sisyphus bridge daemon...");
    let target_dir = Path::new("target");
    let binary = if cfg!(debug_assertions) {
        target_dir.join("debug").join("sisyphus-bridge")
    } else {
        target_dir.join("release").join("sisyphus-bridge")
    };
    let child = if binary.exists() {
        std::process::Command::new(&binary)
            .spawn()
            .with_context(|| format!("Failed to start {}", binary.display()))?
    } else {
        println!(
            "Binary not found at {}, falling back to cargo run...",
            binary.display()
        );
        std::process::Command::new("cargo")
            .args(["run", "-p", "sisyphus-bridge", "--release"])
            .spawn()
            .context("Failed to start bridge daemon via cargo run")?
    };
    println!(
        "Bridge daemon started (PID: {}). Running in background.",
        child.id()
    );
    println!("Use Ctrl+C to stop the daemon.");
    std::thread::spawn(move || {
        let _ = child.wait_with_output();
    });
    Ok(())
}
