//! 示例 1：从环境变量 + 默认值构造 Sisyphus 配置，演示 env 覆盖 + 校验流程。
//!
//! 运行：
//!   cargo run -p sisyphus-core --example config_basic
//!   # 或者带 env override：
//!   SISYPHUS_BRIDGE_ADDRESS=http://0.0.0.0:50051 cargo run -p sisyphus-core --example config_basic
//!
//! 期望输出：
//!   [INFO] config = Config { ... }
//!   [INFO] bridge_address = http://[::1]:50051
//!   [INFO] env override applied: SISYPHUS_BRIDGE_ADDRESS=http://0.0.0.0:50051

use sisyphus_types::config::Config;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. 从环境变量 + 默认值构造配置（不依赖 TOML 文件）
    eprintln!("[INFO] constructing Config from defaults + env vars");
    let mut config = Config::default();
    eprintln!("[INFO] bridge_address = {}", config.bridge_address);
    eprintln!("[INFO] database_url = {}", config.cache.database_url);

    // 2. 应用环境变量覆盖 (SISYPHUS_* 前缀)
    config.apply_env_overrides();
    eprintln!(
        "[INFO] after env overrides: bridge_address = {}",
        config.bridge_address
    );
    if let Ok(v) = std::env::var("SISYPHUS_BRIDGE_ADDRESS") {
        eprintln!("[INFO] SISYPHUS_BRIDGE_ADDRESS = {v}");
    }

    // 3. 验证配置合法性
    config.validate().map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    eprintln!("[INFO] config validated successfully");

    // 4. 序列化为 JSON 便于查看（截断部分字段保持输出紧凑）
    let json = serde_json::to_string_pretty(&serde_json::json!({
        "bridge_address": config.bridge_address,
        "cache": {
            "database_url": config.cache.database_url,
        },
        "sandbox": {
            "wasm_tools_dir": config.sandbox.wasm_tools_dir,
        },
        "llm": {
            "embedding": {
                "allow_dummy": config.llm.embedding.allow_dummy_embeddings,
            },
        },
    }))?;
    println!("\n--- Config summary (JSON) ---\n{json}");

    Ok(())
}
