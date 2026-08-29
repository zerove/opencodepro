//! Shared gRPC types and client for the Sisyphus Bridge protocol.
//!
//! This crate compiles `bridge.proto` once and re-exports all generated
//! types at the crate root for ergonomic access. Both the bridge server
//! (`sisyphus-bridge`) and CLI client (`sisyphus-cli`) depend on this crate
//! instead of compiling the proto independently.

// tonic 生成的 gRPC 服务方法返回 `Result<_, tonic::Status>`，`Status` 类型
// 较大触发 `result_large_err`；生成代码不可修改，此处统一放行。
#![allow(clippy::result_large_err)]

use metrics::counter;
use std::time::Duration;
use tonic::transport::Endpoint;

pub mod proto {
    tonic::include_proto!("sisyphus.bridge");
}

// Re-export all generated types and sub-modules at the crate root
// so callers can write `use sisyphus_bridge_api::sisyphus_bridge_server::SisyphusBridge`
// or `use sisyphus_bridge_api::ExecuteSopRequest` directly.
pub use proto::*;

/// Convenience function to connect to the Sisyphus bridge.
/// Tries IPv6 first, falls back to IPv4 for [::1] addresses.
/// Each attempt has a 3-second timeout.
/// Connect to the Sisyphus bridge gRPC service.
///
/// Tries `addr` as-is for 3 seconds, then falls back to an IPv4 form
/// when `addr` references `[::1]`. Each attempt is independently
/// timeout-bounded so a single failure cannot stall the caller.
///
/// Most callers should not use this directly; prefer
/// [`sisyphus_cli::bridge::BridgeClient::connect`] which adds auth.
pub async fn connect(
    addr: &str,
) -> anyhow::Result<sisyphus_bridge_client::SisyphusBridgeClient<tonic::transport::Channel>> {
    counter!("sisyphus_bridge_api_connections_total").increment(1);
    let orig = addr.to_string();
    // Try IPv6 first
    if let Ok(Ok(client)) = tokio::time::timeout(Duration::from_secs(3), connect_to(&orig)).await {
        return Ok(client);
    }

    // Fall back to IPv4 for [::1]
    if addr.contains("[::1]") {
        let v4_addr = addr.replace("[::1]", "127.0.0.1");
        if let Ok(Ok(client)) =
            tokio::time::timeout(Duration::from_secs(3), connect_to(&v4_addr)).await
        {
            return Ok(client);
        }
    }

    Err(anyhow::anyhow!(
        "Bridge at {addr} unreachable (tried IPv6 and IPv4)"
    ))
}

async fn connect_to(
    target: &str,
) -> Result<
    sisyphus_bridge_client::SisyphusBridgeClient<tonic::transport::Channel>,
    Box<dyn std::error::Error + Send + Sync>,
> {
    let endpoint = Endpoint::new(target.to_string())?;
    let inner = sisyphus_bridge_client::SisyphusBridgeClient::connect(endpoint).await?;
    Ok(inner)
}
