use std::path::PathBuf;

use crate::bridge::{BridgeClient, SopInfo};

// ── Background operation types ──────────────────────────

#[derive(Debug)]
pub enum BgOp {
    SopsListed { count: usize, sops: Vec<SopInfo> },
    SopsFailed(String),
    SopExecuted { sop_id: String, result: String },
    SopExecuteFailed { sop_id: String, error: String },
    RoadmapFetched { projection: String, bottlenecks: String },
    RoadmapFailed(String),
    BridgeCmd { entries: Vec<super::LogEntry> },
    SessionUpdated { session_id: String, errors: Vec<super::LogEntry> },
    DevResult { goal: String, report: String, success: bool },
}

// ── Background spawners ─────────────────────────────────

pub fn spawn_list_sops(bridge_address: String) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let result = async {
            let mut client = BridgeClient::connect(&bridge_address).await.map_err(|e| format!("connect: {e}"))?;
            let sops = client.list_sops().await.map_err(|e| format!("list: {e}"))?;
            let count = sops.len();
            Ok((count, sops))
        }.await;
        let _ = tx.send(match result {
            Ok((count, sops)) => BgOp::SopsListed { count, sops },
            Err(e) => BgOp::SopsFailed(e),
        });
    });
    rx
}

/// Spawn a background task that executes an SOP on the bridge.
pub fn spawn_execute_sop(bridge_address: String, sop_id: String) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let result = async {
            let mut client = BridgeClient::connect(&bridge_address).await.map_err(|e| format!("connect: {e}"))?;
            client.execute_sop(&sop_id).await.map_err(|e| format!("execute: {e}"))
        }.await;
        let _ = tx.send(match result {
            Ok(result) => BgOp::SopExecuted { sop_id, result },
            Err(e) => BgOp::SopExecuteFailed { sop_id, error: e },
        });
    });
    rx
}

/// Spawn a background task that fetches roadmap data (projection + bottlenecks).
pub fn spawn_fetch_roadmap(bridge_address: String) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let result = async {
            let mut client = BridgeClient::connect(&bridge_address).await.map_err(|e| format!("connect: {e}"))?;
            let projection = client.execute_tool("strategic:project_costs", "").await.map_err(|e| format!("projection: {e}"))?;
            let bottlenecks = client.execute_tool("strategic:bottlenecks", "").await.map_err(|e| format!("bottleneck: {e}"))?;
            Ok((projection.result_json, bottlenecks.result_json))
        }.await;
        let _ = tx.send(match result {
            Ok((projection, bottlenecks)) => BgOp::RoadmapFetched { projection, bottlenecks },
            Err(e) => BgOp::RoadmapFailed(e),
        });
    });
    rx
}

pub fn spawn_session_add_message(
    data_dir: PathBuf,
    session_id: String,
    role: String,
    content: String,
    bridge_address: Option<String>,
) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let session_db = data_dir.join("sessions.json");
        if let Err(e) = crate::tools::session_add_message(&session_db, &session_id, &role, &content) {
            let _ = tx.send(BgOp::BridgeCmd {
                entries: vec![super::LogEntry::Error(format!("Failed to add message: {e}"))],
            });
            return;
        }

        // If bridge is available, send the message to trigger AI response.
        // Bridge errors are logged but DON'T prevent SessionUpdated — the session
        // must always be reloaded so the UI doesn't get stuck in "thinking" limbo.
        let mut bridge_errors: Vec<super::LogEntry> = Vec::new();
        if let Some(ref addr) = bridge_address {
            let bridge_op = async {
                let mut client = BridgeClient::connect(addr)
                    .await
                    .map_err(|e| format!("{e}"))?;
                client
                    .add_message(&session_id, &role, &content, &session_db.to_string_lossy())
                    .await
                    .map_err(|e| format!("{e}"))
            };
            match tokio::time::timeout(std::time::Duration::from_secs(120), bridge_op).await {
                Ok(Ok(resp)) => {
                    if !resp.success {
                        let err_msg = if resp.error.is_empty() {
                            "LLM returned empty response".into()
                        } else {
                            resp.error
                        };
                        bridge_errors.push(super::LogEntry::Error(format!(
                            "Bridge LLM error: {err_msg}",
                        )));
                    }
                }
                Ok(Err(e)) => {
                    bridge_errors.push(super::LogEntry::Error(format!(
                        "Bridge add_message failed: {e}",
                    )));
                }
                Err(_) => {
                    bridge_errors.push(super::LogEntry::Warn(
                        "LLM response timed out (120s), check bridge logs".into(),
                    ));
                }
            }
        }

        // Always reload session detail from disk so the "thinking" indicator stops
        // and the user's message is reflected in persisted state.
        let _ = tx.send(BgOp::SessionUpdated {
            session_id,
            errors: bridge_errors,
        });
    });
    rx
}

/// Spawn a background task that runs the developer agent and returns a formatted report.
pub fn spawn_dev_agent(
    config: sisyphus_types::Config,
    goal: String,
    context: String,
    parallel: bool,
    resume: bool,
) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let config = std::sync::Arc::new(std::sync::RwLock::new(config));
        let llm: std::sync::Arc<dyn crate::tools::metis::LlmClient> =
            std::sync::Arc::new(crate::tools::llm::DynamicLlmClient::new(config));
        let developer = crate::tools::developer::Developer::new(llm);
        let goal_c = goal.clone();
        match developer.develop(&goal, &context, parallel, resume).await {
            Ok(report) => {
                let formatted = crate::tools::developer::format_report(&report);
                let success = report.overall_status
                    == crate::tools::developer::DevOverallStatus::Success;
                let _ = tx.send(BgOp::DevResult { goal: goal_c, report: formatted, success });
            }
            Err(e) => {
                let _ = tx.send(BgOp::DevResult {
                    goal: goal_c,
                    report: format!("Development agent failed:\n{e}"),
                    success: false,
                });
            }
        }
    });
    rx
}
