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
    SessionUpdated { session_id: String },
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

/// Spawn a background task that executes a `:bridge` shell command.
/// Returns log entries to be displayed when the operation completes.
pub fn spawn_bridge_cmd(
    bridge_address: String,
    cmd: String,
) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let parts: Vec<&str> = cmd.splitn(3, ' ').collect();
        let sub = parts.get(1).copied().unwrap_or("status");
        let entries = match BridgeClient::connect(&bridge_address).await {
            Ok(mut client) => match sub {
                "status" | "health" => match client.health_check().await {
                    Ok(resp) => vec![super::LogEntry::Success(format!(
                        "Bridge: {} (v{})",
                        resp.status, resp.version
                    ))],
                    Err(e) => vec![super::LogEntry::Error(format!("Health check failed: {e}"))],
                },
                "sops" | "list" => match client.list_sops().await {
                    Ok(sops) if sops.is_empty() => {
                        vec![super::LogEntry::Info("No SOPs registered.".into())]
                    }
                    Ok(sops) => {
                        let mut entries: Vec<super::LogEntry> =
                            vec![super::LogEntry::Info(format!("Bridge SOPs ({}):", sops.len()))];
                        for sop in &sops {
                            entries.push(super::LogEntry::Success(format!(
                                "  {} ({} nodes)",
                                sop.sop_id, sop.node_count
                            )));
                        }
                        entries
                    }
                    Err(e) => vec![super::LogEntry::Error(format!("List SOPs failed: {e}"))],
                },
                "run" => {
                    let sop_id = parts.get(2).unwrap_or(&"");
                    if sop_id.is_empty() {
                        vec![super::LogEntry::Error("Usage: :bridge run <sop_id>".into())]
                    } else {
                        match client.execute_sop(sop_id).await {
                            Ok(result) => vec![super::LogEntry::Success(result)],
                            Err(e) => {
                                vec![super::LogEntry::Error(format!("SOP execution failed: {e}"))]
                            }
                        }
                    }
                }
                other => vec![super::LogEntry::Error(format!(
                    "Unknown bridge command: {other} (try: status, sops, run)"
                ))],
            },
            Err(e) => vec![super::LogEntry::Error(format!(
                "Failed to connect to bridge at {bridge_address}: {e}"
            ))],
        };
        let _ = tx.send(BgOp::BridgeCmd { entries });
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
        if let Err(e) = sisyphus_tools::session_add_message(&session_db, &session_id, &role, &content) {
            let _ = tx.send(BgOp::BridgeCmd {
                entries: vec![super::LogEntry::Error(format!("Failed to add message: {e}"))],
            });
            return;
        }

        // If bridge is available, send the message to trigger AI response
        if let Some(ref addr) = bridge_address {
            match BridgeClient::connect(addr).await {
                Ok(mut client) => {
                    match client
                        .add_message(&session_id, &role, &content, &session_db.to_string_lossy())
                        .await
                    {
                        Ok(resp) => {
                            if !resp.success {
                                let err_msg = if resp.error.is_empty() {
                                    "LLM returned empty response".into()
                                } else {
                                    resp.error
                                };
                                let _ = tx.send(BgOp::BridgeCmd {
                                    entries: vec![super::LogEntry::Error(format!(
                                        "Bridge LLM error: {err_msg}"
                                    ))],
                                });
                                return;
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(BgOp::BridgeCmd {
                                entries: vec![super::LogEntry::Error(format!(
                                    "Bridge add_message failed: {e}"
                                ))],
                            });
                            return;
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(BgOp::BridgeCmd {
                        entries: vec![super::LogEntry::Warn(format!(
                            "Bridge not available, message saved locally: {e}"
                        ))],
                    });
                    return;
                }
            }
        }

        let _ = tx.send(BgOp::SessionUpdated { session_id });
    });
    rx
}
