//! Sisyphus CLI — 认知引擎命令行界面。
//!
//! 提供 40+ 子命令、TUI 交互模式、桥接配置与会话管理。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use clap::Parser;

mod commands;
mod handlers;
use commands::*;

#[derive(Parser)]
#[command(name = "sisyphus", version, about = "Sisyphus 认知引擎 CLI")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// 启用交互式 TUI 模式
    #[arg(short = 'i', long = "interactive")]
    interactive: bool,

    /// 桥接地址（默认：`http://[::1]:50051`）
    #[arg(short = 'b', long = "bridge", env = "SISYPHUS_BRIDGE_ADDRESS")]
    bridge: Option<String>,

    /// 待办/会话存储的数据目录
    #[arg(long = "data-dir", env = "SISYPHUS_DATA_DIR")]
    data_dir: Option<PathBuf>,

    /// Sisyphus TOML 配置文件路径
    #[arg(long = "config", env = "SISYPHUS_CONFIG")]
    config: Option<PathBuf>,
}

// ── 主流程 ─────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let platform_info = sisyphus_cli::platform::detect_platform();
    tracing::info!(
        "Starting Sisyphus CLI on {} (shell: {})",
        platform_info.os,
        platform_info.shell
    );

    let cli = Cli::parse();

    // 解析配置：先文件，再环境变量，最后默认值
    let sisyphus_cfg = if let Some(ref config_path) = cli.config {
        match sisyphus_types::Config::from_file(&config_path.to_string_lossy()) {
            Ok(cfg) => cfg,
            Err(e) => {
                eprintln!(
                    "Warning: failed to load config '{}': {e}",
                    config_path.display()
                );
                sisyphus_types::Config::default()
            },
        }
    } else {
        sisyphus_types::Config::default()
    };

    let data_dir = cli.data_dir.clone().unwrap_or_else(|| {
        let sqlite = PathBuf::from(&sisyphus_cfg.cache.database_url);
        sqlite.parent().unwrap_or(Path::new(".sisyphus")).to_path_buf()
    });
    let bridge_addr = cli.bridge.clone().unwrap_or_else(|| sisyphus_cfg.bridge_address.clone());

    // 交互式 TUI 模式
    if cli.interactive || cli.command.is_none() {
        sisyphus_cli::tui::run(data_dir, bridge_addr)?;
        return Ok(());
    }

    // 确保数据目录存在
    std::fs::create_dir_all(&data_dir)?;

    let cli_perms = sisyphus_cli::permission::CliPermission::load(None).unwrap_or_default();

    let todo_db = data_dir.join("todos.json");
    let session_db = data_dir.join("sessions.json");

    // 为代理命令从配置创建 LLM 客户端
    let llm_client: Arc<dyn sisyphus_cli::tools::metis::LlmClient> = {
        let endpoint = sisyphus_cfg.llm.endpoint.clone();
        let is_default_endpoint = endpoint == "http://localhost:8000/v1/chat/completions";
        if is_default_endpoint {
            Arc::new(sisyphus_cli::tools::DummyLlmClient)
        } else {
            let llm_config = Arc::new(std::sync::RwLock::new(sisyphus_cfg.clone()));
            Arc::new(sisyphus_cli::tools::llm::DynamicLlmClient::new(llm_config))
        }
    };

    handlers::dispatch(
        &cli,
        &sisyphus_cfg,
        &todo_db,
        &session_db,
        &cli_perms,
        llm_client,
        &bridge_addr,
    )
    .await
}
