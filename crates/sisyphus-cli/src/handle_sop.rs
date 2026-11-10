use anyhow::{Context, Result};

use crate::commands::*;

fn http_base_url(cli_http: Option<&str>) -> String {
    if let Some(h) = cli_http {
        return h.trim_end_matches('/').to_string();
    }
    if let Ok(v) = std::env::var("SISYPHUS_HTTP_ADDRESS") {
        if !v.trim().is_empty() {
            return v.trim().trim_end_matches('/').to_string();
        }
    }
    "http://127.0.0.1:8080".to_string()
}

/// Handle sop.
pub async fn handle_sop(sub: &SopCommands, bridge_addr: &str) -> Result<()> {
    match sub {
        SopCommands::Run { sop_id } => {
            let mut client = sisyphus_cli::bridge::BridgeClient::connect(bridge_addr).await?;
            let result = client.execute_sop(sop_id).await?;
            println!("{result}");
        },
        SopCommands::List => {
            let mut client = sisyphus_cli::bridge::BridgeClient::connect(bridge_addr).await?;
            let sops = client.list_sops().await?;
            if sops.is_empty() {
                println!("No SOPs registered.");
                println!(
                    "提示: 可通过 REST `POST /api/sops` 保存模板（见 docs/examples/sop-templates/README.md），或运行 scripts/single-node-demo-flow.sh 查看端到端流程。"
                );
            } else {
                for sop in &sops {
                    println!("  {} ({} nodes)", sop.sop_id, sop.node_count);
                }
                println!(
                    "提示: 使用 `sisyphus --bridge <addr> sop run <sop-id>` 执行；`sisyphus sop show <sop-id> --http <http_addr>` 查看详情；`execute/goal` 为目标驱动路径（可能依赖 LLM 配置）。"
                );
            }
        },
        SopCommands::Show {
            sop_id,
            http,
            workspace,
        } => {
            let base = http_base_url(http.as_deref());
            let url = format!("{base}/api/sops/{sop_id}");
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .context("failed to build HTTP client")?;
            let resp = client
                .get(&url)
                .header("X-Sisyphus-Workspace-Id", workspace)
                .send()
                .await
                .with_context(|| format!("failed to request SOP detail: {url}"))?;
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            if !status.is_success() {
                anyhow::bail!(
                    "sop show failed: HTTP {} {} (url={url}, workspace={workspace})\n提示: 确认 bridge HTTP 地址、SOP ID 与工作区头是否正确。",
                    status.as_u16(),
                    body.chars().take(300).collect::<String>().replace('\n', " ")
                );
            }
            // pretty-print JSON if possible
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
                println!("{}", serde_json::to_string_pretty(&v)?);
            } else {
                println!("{body}");
            }
        },
    }
    Ok(())
}
