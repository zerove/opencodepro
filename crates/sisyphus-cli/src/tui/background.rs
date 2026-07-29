use std::path::PathBuf;

use crate::bridge::{BridgeClient, SopInfo};
use crate::tui::app::{RemoteSkillEntry, WorkspaceEntry};

// ── Background operation types ──────────────────────────

#[derive(Debug, Clone, serde::Deserialize)]
pub struct SopTraceEvent {
    pub node_id: String,
    pub status: String,
    pub output: Option<String>,
    pub fuel_consumed: i64,
    pub duration_ms: i64,
}

#[derive(Debug)]
pub enum BgOp {
    SopsListed {
        count: usize,
        sops: Vec<SopInfo>,
    },
    SopsFailed(String),
    SopExecuted {
        sop_id: String,
        result: String,
    },
    SopExecuteFailed {
        sop_id: String,
        error: String,
    },
    SopTraceFetched {
        sop_id: String,
        trace: Vec<SopTraceEvent>,
    },
    SopTraceFailed(String),
    RoadmapFetched {
        projection: String,
        bottlenecks: String,
    },
    RoadmapFailed(String),
    BridgeCmd {
        entries: Vec<super::LogEntry>,
    },
    SessionUpdated {
        session_id: String,
        errors: Vec<super::LogEntry>,
    },
    TodoUpdated,
    DevResult {
        goal: String,
        report: String,
        success: bool,
    },
    SkillsAvailable {
        skills: Vec<RemoteSkillEntry>,
    },
    SkillsInstalled {
        skills: Vec<RemoteSkillEntry>,
    },
    SkillOpResult {
        success: bool,
        message: String,
    },
    WorkspacesListed {
        workspaces: Vec<WorkspaceEntry>,
    },
    WorkspaceSwitched {
        id: String,
    },
    WorkspaceOpResult {
        success: bool,
        message: String,
    },
    /// Generic operation status update from the bridge (e.g. anomaly_detector, chaos_engine).
    OpUpdate {
        op_id: String,
        status: String,
        result: String,
    },
}

// ── Background spawners ─────────────────────────────────

pub fn spawn_list_sops(
    client: Option<crate::bridge::BridgeClient>,
    bridge_address: String,
) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let result = async {
            let mut client = match client {
                Some(c) => c,
                None => BridgeClient::connect(&bridge_address)
                    .await
                    .map_err(|e| format!("connect: {e}"))?,
            };
            let sops = client.list_sops().await.map_err(|e| format!("list: {e}"))?;
            let count = sops.len();
            Ok((count, sops))
        }
        .await;
        let _ = tx.send(match result {
            Ok((count, sops)) => BgOp::SopsListed { count, sops },
            Err(e) => BgOp::SopsFailed(e),
        });
    });
    rx
}

/// Spawn a background task that executes an SOP on the bridge.
pub fn spawn_execute_sop(
    client: Option<crate::bridge::BridgeClient>,
    bridge_address: String,
    sop_id: String,
) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let result = async {
            let mut client = match client {
                Some(c) => c,
                None => BridgeClient::connect(&bridge_address)
                    .await
                    .map_err(|e| format!("connect: {e}"))?,
            };
            client
                .execute_sop(&sop_id)
                .await
                .map_err(|e| format!("execute: {e}"))
        }
        .await;
        let _ = tx.send(match result {
            Ok(result) => BgOp::SopExecuted { sop_id, result },
            Err(e) => BgOp::SopExecuteFailed { sop_id, error: e },
        });
    });
    rx
}

/// Spawn a background task that fetches roadmap data (projection + bottlenecks).
pub fn spawn_fetch_roadmap(
    client: Option<crate::bridge::BridgeClient>,
    bridge_address: String,
) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let result = async {
            let mut client = match client {
                Some(c) => c,
                None => BridgeClient::connect(&bridge_address)
                    .await
                    .map_err(|e| format!("connect: {e}"))?,
            };
            let projection = client
                .execute_tool("strategic:project_costs", "")
                .await
                .map_err(|e| format!("projection: {e}"))?;
            let bottlenecks = client
                .execute_tool("strategic:bottlenecks", "")
                .await
                .map_err(|e| format!("bottleneck: {e}"))?;
            Ok((projection.result_json, bottlenecks.result_json))
        }
        .await;
        let _ = tx.send(match result {
            Ok((projection, bottlenecks)) => BgOp::RoadmapFetched {
                projection,
                bottlenecks,
            },
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
        if let Err(e) = crate::tools::session_add_message(&session_db, &session_id, &role, &content)
        {
            let _ = tx.send(BgOp::BridgeCmd {
                entries: vec![super::LogEntry::Error(format!(
                    "Failed to add message: {e}"
                ))],
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
                let success =
                    report.overall_status == crate::tools::developer::DevOverallStatus::Success;
                let _ = tx.send(BgOp::DevResult {
                    goal: goal_c,
                    report: formatted,
                    success,
                });
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

pub fn spawn_approve_proposal(
    client: Option<crate::bridge::BridgeClient>,
    bridge_address: String,
    proposal_id: String,
    reviewer: String,
) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let result = async {
            let mut client = match client {
                Some(c) => c,
                None => BridgeClient::connect(&bridge_address)
                    .await
                    .map_err(|e| format!("connect: {e}"))?,
            };
            let args = serde_json::json!({
                "proposal_id": proposal_id,
                "reviewer": reviewer,
            });
            client
                .execute_tool("strategic:approve_proposal", &args.to_string())
                .await
                .map_err(|e| format!("approve: {e}"))
        }
        .await;
        let _ = tx.send(match result {
            Ok(_) => BgOp::BridgeCmd {
                entries: vec![super::LogEntry::Success("Proposal approved".to_string())],
            },
            Err(e) => BgOp::BridgeCmd {
                entries: vec![super::LogEntry::Error(format!("Approve failed: {e}"))],
            },
        });
    });
    rx
}

pub fn spawn_fetch_sop_trace(
    bridge_address: String,
    sop_id: String,
) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let url = format!(
            "http://{}/api/execute/goal/{}/trace",
            bridge_address, sop_id
        );
        let result = async {
            let resp = reqwest::get(&url)
                .await
                .map_err(|e| format!("HTTP request failed: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("Bridge returned error status: {}", resp.status()));
            }
            let data: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| format!("JSON parse failed: {e}"))?;
            let events: Vec<SopTraceEvent> = serde_json::from_value(data["events"].clone())
                .map_err(|e| format!("Trace event parse failed: {e}"))?;
            Ok(events)
        }
        .await;
        let _ = tx.send(match result {
            Ok(trace) => BgOp::SopTraceFetched { sop_id, trace },
            Err(e) => BgOp::SopTraceFailed(e),
        });
    });
    rx
}

pub fn spawn_fetch_available_skills(
    bridge_address: String,
) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let url = format!("http://{}/api/skills/available", bridge_address);
        let result = async {
            let resp = reqwest::get(&url)
                .await
                .map_err(|e| format!("HTTP request failed: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("Bridge returned error status: {}", resp.status()));
            }
            let skills: Vec<RemoteSkillEntry> = resp
                .json()
                .await
                .map_err(|e| format!("JSON parse failed: {e}"))?;
            Ok(skills)
        }
        .await;
        let _ = tx.send(match result {
            Ok(skills) => BgOp::SkillsAvailable { skills },
            Err(e) => BgOp::BridgeCmd {
                entries: vec![super::LogEntry::Error(e)],
            },
        });
    });
    rx
}

pub fn spawn_fetch_installed_skills(
    bridge_address: String,
) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let url = format!("http://{}/api/skills/installed", bridge_address);
        let result = async {
            let resp = reqwest::get(&url)
                .await
                .map_err(|e| format!("HTTP request failed: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("Bridge returned error status: {}", resp.status()));
            }
            let skills: Vec<RemoteSkillEntry> = resp
                .json()
                .await
                .map_err(|e| format!("JSON parse failed: {e}"))?;
            Ok(skills)
        }
        .await;
        let _ = tx.send(match result {
            Ok(skills) => BgOp::SkillsInstalled { skills },
            Err(e) => BgOp::BridgeCmd {
                entries: vec![super::LogEntry::Error(e)],
            },
        });
    });
    rx
}

pub fn spawn_install_skill(
    bridge_address: String,
    skill_id: String,
) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let url = format!("http://{}/api/skills/install", bridge_address);
        let result = async {
            let client = reqwest::Client::new();
            let resp = client
                .post(&url)
                .json(&serde_json::json!({ "skill_id": skill_id }))
                .send()
                .await
                .map_err(|e| format!("HTTP request failed: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("Bridge returned error status: {}", resp.status()));
            }
            Ok(())
        }
        .await;
        let _ = tx.send(match result {
            Ok(_) => BgOp::SkillOpResult {
                success: true,
                message: "Skill installed successfully".into(),
            },
            Err(e) => BgOp::SkillOpResult {
                success: false,
                message: e,
            },
        });
    });
    rx
}

pub fn spawn_uninstall_skill(
    bridge_address: String,
    skill_id: String,
) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let url = format!("http://{}/api/skills/remote/{}", bridge_address, skill_id);
        let result = async {
            let client = reqwest::Client::new();
            let resp = client
                .delete(&url)
                .send()
                .await
                .map_err(|e| format!("HTTP request failed: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("Bridge returned error status: {}", resp.status()));
            }
            Ok(())
        }
        .await;
        let _ = tx.send(match result {
            Ok(_) => BgOp::SkillOpResult {
                success: true,
                message: "Skill uninstalled successfully".into(),
            },
            Err(e) => BgOp::SkillOpResult {
                success: false,
                message: e,
            },
        });
    });
    rx
}

pub fn spawn_list_workspaces(bridge_address: String) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let url = format!("http://{}/api/workspaces", bridge_address);
        let result = async {
            let resp = reqwest::get(&url)
                .await
                .map_err(|e| format!("HTTP request failed: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("Bridge returned error status: {}", resp.status()));
            }
            let workspaces: Vec<WorkspaceEntry> = resp
                .json()
                .await
                .map_err(|e| format!("JSON parse failed: {e}"))?;
            Ok(workspaces)
        }
        .await;
        let _ = tx.send(match result {
            Ok(workspaces) => BgOp::WorkspacesListed { workspaces },
            Err(e) => BgOp::WorkspaceOpResult {
                success: false,
                message: e,
            },
        });
    });
    rx
}

pub fn spawn_switch_workspace(
    bridge_address: String,
    workspace_id: String,
) -> tokio::sync::oneshot::Receiver<BgOp> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let url = format!("http://{}/api/workspaces/switch", bridge_address);
        let result = async {
            let client = reqwest::Client::new();
            let resp = client
                .post(&url)
                .json(&serde_json::json!({ "id": workspace_id }))
                .send()
                .await
                .map_err(|e| format!("HTTP request failed: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("Bridge returned error status: {}", resp.status()));
            }
            Ok(())
        }
        .await;
        let _ = tx.send(match result {
            Ok(_) => BgOp::WorkspaceSwitched { id: workspace_id },
            Err(e) => BgOp::WorkspaceOpResult {
                success: false,
                message: e,
            },
        });
    });
    rx
}
