//! Sisyphus Web 仪表盘 —— 独立服务器入口点。
//!
//! 提供 axum + HTMX + SSE 仪表盘，将 API 调用代理到 gRPC bridge 的 REST API。
//!
//! 用法：
//!   cargo run -p sisyphus-webui -- --bridge http://[::1]:8080 --listen 0.0.0.0:3000
//!
//! 环境变量：
//!   WEBUI_BRIDGE_URL   Bridge REST API 基础 URL (默认: http://[::1]:8080)
//!   WEBUI_LISTEN_ADDR  监听地址 (默认: 0.0.0.0:3000)
//!   WEBUI_API_KEY      可选的 bearer token，用于保护路由

use sisyphus_webui_lib::app;
use std::net::SocketAddr;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let bridge_url =
        std::env::var("WEBUI_BRIDGE_URL").unwrap_or_else(|_| "http://[::1]:8080".into());
    let listen_addr: SocketAddr = std::env::var("WEBUI_LISTEN_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:3000".into())
        .parse()
        .map_err(|e| format!("Invalid WEBUI_LISTEN_ADDR: {e}"))?;

    let router = app(bridge_url.clone());
    let listener = tokio::net::TcpListener::bind(listen_addr).await?;

    info!("Sisyphus Web Dashboard listening on http://{listen_addr}");
    info!("Proxying to bridge REST API at {bridge_url}");
    info!(
        "Auth: {}",
        if std::env::var("WEBUI_API_KEY").ok().is_some_and(|k| !k.is_empty()) {
            "enabled (WEBUI_API_KEY)"
        } else {
            "disabled"
        }
    );

    axum::serve(listener, router).await.map_err(|e| {
        error!("Web server error: {e}");
        Box::new(std::io::Error::other(format!("Web server error: {e}")))
            as Box<dyn std::error::Error>
    })
}
