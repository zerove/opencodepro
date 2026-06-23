use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Deserialize)]
struct TaskDef {
    id: String,
    name: String,
    command: String,
    #[serde(default)]
    dir: Option<String>,
}

pub async fn run_tasks(file: PathBuf, max_concurrent: usize) -> Result<()> {
    let content = std::fs::read_to_string(&file)
        .with_context(|| format!("read task file: {}", file.display()))?;
    let tasks: Vec<TaskDef> = serde_json::from_str(&content)
        .context("parse task JSON — expected array of {id, name, command}")?;

    if tasks.is_empty() {
        println!("No tasks defined.");
        return Ok(());
    }

    let units: Vec<sisyphus_tools::WorkUnit> = tasks
        .into_iter()
        .map(|t| {
            sisyphus_tools::work_unit(&t.id, &t.name, move || {
                let cmd = t.command.clone();
                let dir = t.dir.clone();
                async move {
                    let dir_ref = dir.as_deref().map(std::path::Path::new);
                    let output = sisyphus_tools::bash::execute(&cmd, dir_ref, 60000).await?;
                    Ok(serde_json::json!({
                        "stdout": output.stdout,
                        "stderr": output.stderr,
                        "exit_code": output.exit_code,
                        "duration_ms": output.duration.as_millis(),
                    }))
                }
            })
        })
        .collect();

    let orchestrator = sisyphus_tools::WorkOrchestrator::new(max_concurrent);
    let (mut rx, handle) = orchestrator.run_all_with_events(units).await;

    // Display progress in real-time
    let mut completed = 0usize;
    let total = handle
        .await
        .context("task execution failed")?;

    // Collect events until done
    use sisyphus_tools::DelegateEvent;
    loop {
        match rx.try_recv() {
            Ok(DelegateEvent::Started { id, name }) => {
                println!("[{id}] {name} ... running");
            }
            Ok(DelegateEvent::Completed(report)) => {
                completed += 1;
                let status = if report.status == sisyphus_tools::WorkStatus::Completed {
                    format!("OK ({}ms)", report.duration_ms)
                } else {
                    format!("FAIL ({})", report.error.as_deref().unwrap_or("unknown"))
                };
                println!("[{}] {} ... {}", report.id, report.name, status);
            }
            Ok(DelegateEvent::Progress { id, message }) => {
                println!("[{id}] {message}");
            }
            Err(_) => {
                if completed == total.len() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }

    println!("\nDone: {}/{} tasks completed", completed, total.len());
    Ok(())
}

pub async fn run_inline(
    commands: &[String],
    max_concurrent: usize,
) -> Result<()> {
    let units: Vec<sisyphus_tools::WorkUnit> = commands
        .iter()
        .enumerate()
        .map(|(i, cmd)| {
            let id = format!("task-{i}");
            let cmd = cmd.clone();
            let cmd_display = cmd[..cmd.len().min(40)].to_string();
            sisyphus_tools::work_unit(&id, &cmd_display, move || {
                let cmd = cmd.clone();
                async move {
                    let output = sisyphus_tools::bash::execute(&cmd, None, 60000).await?;
                    Ok(serde_json::json!({
                        "stdout": output.stdout,
                        "stderr": output.stderr,
                        "exit_code": output.exit_code,
                        "duration_ms": output.duration.as_millis(),
                    }))
                }
            })
        })
        .collect();

    let orchestrator = sisyphus_tools::WorkOrchestrator::new(max_concurrent);
    let reports = orchestrator.run_all(units).await?;

    for r in &reports {
        let tag = if r.status == sisyphus_tools::WorkStatus::Completed {
            "OK"
        } else {
            "FAIL"
        };
        println!("[{}] {} {} ({}ms)", r.id, tag, r.name, r.duration_ms);
    }
    println!("\nDone: {}/{}", reports.iter().filter(|r| r.status == sisyphus_tools::WorkStatus::Completed).count(), reports.len());
    Ok(())
}
