use anyhow::Result;

pub fn handle_quickstart(bridge_addr: &str, http_addr: &str) -> Result<()> {
    println!("Sisyphus 单机快速上手\n");
    println!("== 1. 诊断与就绪 ==");
    println!("  sisyphus doctor --http {http_addr}");
    println!("  BASE={http_addr} ./scripts/single-node-readiness-check.sh");
    println!();
    println!("== 2. 启动服务（如需 bridge/WebUI 能力） ==");
    println!(
        "  # 配置文件建议使用顶层字段: bridge_address / http_address（不要放在 [bridge] 子表）"
    );
    println!("  # 示例: bridge_address=\"http://127.0.0.1:50051\", http_address=\"http://127.0.0.1:8080\"");
    println!("  SISYPHUS_CONFIG=./config.toml cargo run -p sisyphus-bridge --bin sisyphus-bridge");
    println!();
    println!("== 3. 发现与执行（SOP） ==");
    println!("  sisyphus --bridge {bridge_addr} sop list");
    println!("  sisyphus sop show <sop-id> --http {http_addr}");
    println!("  sisyphus --bridge {bridge_addr} sop run <sop-id>");
    println!("  curl -s {http_addr}/api/sops");
    println!("  # 端到端演示: BASE={http_addr} BRIDGE_GRPC={bridge_addr} SOP_ID=verify_sop ./scripts/single-node-demo-flow.sh");
    println!();
    println!("== 4. 本地会话与工具（无需 bridge） ==");
    println!("  sisyphus --data-dir ./.sisyphus-data session create demo");
    println!("  sisyphus read README.md");
    println!("  sisyphus exec 'echo ok'");
    println!("  sisyphus tool --help   # 工具注册/列表/自举");
    println!("  sisyphus mcp --help    # MCP 连接/调用/serve");
    println!("  sisyphus skill list");
    println!();
    println!("== 5. 文档与样例 ==");
    println!("  docs/runbooks/single-node-operation-and-verification.md");
    println!("  docs/runbooks/single-node-typical-workflow.md");
    println!("  docs/runbooks/single-node-sop-e2e-guide.md");
    println!("  docs/examples/single-node-sdk-examples.md");
    println!("  docs/examples/sop-templates/README.md");
    println!();
    println!("提示: `POST /api/execute/goal` 为目标驱动路径，可能依赖 LLM 配置；CLI `sop run` 为直接 SOP 执行路径。");
    Ok(())
}
