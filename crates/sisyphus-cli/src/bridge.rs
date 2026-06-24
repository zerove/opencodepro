use anyhow::{Context, Result};
use std::collections::HashMap;
use tonic::transport::Endpoint;

tonic::include_proto!("sisyphus.bridge");

pub struct BridgeClient {
    inner: sisyphus_bridge_client::SisyphusBridgeClient<tonic::transport::Channel>,
}

impl BridgeClient {
    /// Connect to a running Sisyphus bridge daemon.
    pub async fn connect(addr: &str) -> Result<Self> {
        let endpoint = Endpoint::new(addr.to_string())
            .context("Invalid bridge address")?;
        let inner = sisyphus_bridge_client::SisyphusBridgeClient::connect(endpoint)
            .await
            .context(format!("Failed to connect to bridge at {addr}"))?;
        Ok(Self { inner })
    }

    /// Execute an SOP (Standard Operating Procedure) by ID.
    pub async fn execute_sop(&mut self, sop_id: &str) -> Result<String> {
        let request = tonic::Request::new(ExecuteSopRequest {
            sop_id: sop_id.to_string(),
            args: HashMap::new(),
        });
        let response = self
            .inner
            .execute_sop(request)
            .await
            .context("ExecuteSOP RPC failed")?;
        Ok(response.into_inner().result)
    }

    /// Check if the bridge is healthy.
    pub async fn health_check(&mut self) -> Result<HealthCheckResponse> {
        let request = tonic::Request::new(HealthCheckRequest {});
        let response = self
            .inner
            .health_check(request)
            .await
            .context("HealthCheck RPC failed")?;
        Ok(response.into_inner())
    }

    /// List available SOPs.
    pub async fn list_sops(&mut self) -> Result<Vec<SopInfo>> {
        let request = tonic::Request::new(ListSoPsRequest {});
        let response = self
            .inner
            .list_so_ps(request)
            .await
            .context("ListSOPs RPC failed")?;
        Ok(response.into_inner().sops)
    }

    /// Execute a built-in or agent tool via the bridge's ExecutorRouter.
    ///
    /// `tool_id` can be any BuiltinToolExecutor ID (e.g. "file_read",
    /// "librarian/research", "metis/analyse") and `args_json` follows the
    /// JSON schema documented in the mdBook (crates/tools.md).
    pub async fn execute_tool(&mut self, tool_id: &str, args_json: &str) -> Result<ExecuteToolResponse> {
        let request = tonic::Request::new(ExecuteToolRequest {
            tool_id: tool_id.to_string(),
            args_json: args_json.to_string(),
            fuel_limit: 0,
        });
        let response = self
            .inner
            .execute_tool(request)
            .await
            .context(format!("ExecuteTool({tool_id}) RPC failed"))?;
        Ok(response.into_inner())
    }
}
