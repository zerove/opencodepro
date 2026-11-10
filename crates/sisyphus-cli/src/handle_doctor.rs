use std::path::Path;
use std::time::Duration;

use anyhow::Result;

fn ok(msg: impl AsRef<str>) {
    println!("  [OK]   {}", msg.as_ref());
}
fn warn(msg: impl AsRef<str>) {
    println!("  [WARN] {}", msg.as_ref());
}
fn fail(msg: impl AsRef<str>) {
    println!("  [FAIL] {}", msg.as_ref());
}

pub async fn handle_doctor(
    bridge_addr: &str,
    http_base: Option<&str>,
    data_dir: &Path,
    config_path: Option<&str>,
) -> Result<()> {
    println!("Sisyphus doctor（单机诊断）\n");

    println!("== 本地环境 ==");
    println!("  bridge(gRPC): {bridge_addr}");
    if let Some(http) = http_base {
        println!("  bridge(HTTP): {http}");
    } else {
        println!("  bridge(HTTP): (未指定，可用 --http http://127.0.0.1:8080)");
    }
    println!("  data_dir: {}", data_dir.display());
    if let Some(cfg) = config_path {
        println!("  config: {cfg}");
        if Path::new(cfg).exists() {
            ok("配置文件存在");
        } else {
            warn("配置文件路径不存在（将回退默认配置）");
        }
    } else {
        println!("  config: (未指定，使用默认/环境变量)");
    }
    if data_dir.exists() {
        ok("数据目录存在");
    } else {
        warn("数据目录不存在（首次运行命令时可能自动创建）");
    }

    let key_paths = [
        ("scripts/single-node-readiness-check.sh", "就绪检查脚本"),
        ("scripts/single-node-demo-flow.sh", "端到端演示脚本"),
        ("scripts/single-node-contract-smoke.sh", "契约冒烟脚本"),
        ("docs/examples/sop-templates/README.md", "SOP 模板资产说明"),
        (
            "docs/runbooks/single-node-operation-and-verification.md",
            "单机运行与验收文档",
        ),
    ];
    for (p, label) in key_paths {
        if Path::new(p).exists() {
            ok(format!("{label}: {p}"));
        } else {
            warn(format!("{label}缺失: {p}"));
        }
    }

    println!("\n== Bridge gRPC ==");
    match sisyphus_cli::bridge::BridgeClient::connect(bridge_addr).await {
        Ok(mut client) => {
            ok("gRPC 连接成功");
            match client.health_check().await {
                Ok(resp) => ok(format!(
                    "health_check: status={} version={}",
                    resp.status, resp.version
                )),
                Err(e) => fail(format!("health_check 失败: {e}")),
            }
            match client.list_sops().await {
                Ok(sops) => ok(format!("list_sops: {} 个 SOP", sops.len())),
                Err(e) => warn(format!("list_sops 失败: {e}")),
            }
        },
        Err(e) => {
            fail(format!("gRPC 不可达: {e}"));
            println!(
                "  建议: 启动 bridge（例如 `cargo run -p sisyphus-bridge --bin sisyphus-bridge`），并检查 --bridge / SISYPHUS_CONFIG 中 bridge_address。"
            );
        },
    }

    println!("\n== Bridge HTTP（可选） ==");
    if let Some(base) = http_base {
        let base = base.trim_end_matches('/');
        let client = reqwest::Client::builder().timeout(Duration::from_secs(3)).build();
        match client {
            Ok(client) => {
                for (name, path) in [
                    ("health", "/health"),
                    ("health_live", "/health/live"),
                    ("openapi", "/openapi.json"),
                    ("sops", "/api/sops"),
                    ("cluster_status", "/api/cluster/status"),
                ] {
                    let url = format!("{base}{path}");
                    match client.get(&url).send().await {
                        Ok(resp) => {
                            let status = resp.status();
                            if status.is_success() {
                                let body = resp.text().await.unwrap_or_default();
                                let preview: String = body.chars().take(120).collect();
                                ok(format!(
                                    "{name}: {} {}",
                                    status.as_u16(),
                                    preview.replace('\n', " ")
                                ));
                            } else {
                                warn(format!("{name}: HTTP {} ({url})", status.as_u16()));
                            }
                        },
                        Err(e) => warn(format!("{name}: 请求失败: {e} ({url})")),
                    }
                }
            },
            Err(e) => warn(format!("HTTP 客户端初始化失败: {e}")),
        }
    } else {
        println!("  跳过（未提供 --http）。示例: sisyphus doctor --http http://127.0.0.1:8080");
    }

    println!("\n== 建议 ==");
    println!("  - 快速上手: sisyphus quickstart");
    println!("  - 端到端演示: ./scripts/single-node-demo-flow.sh（需 bridge HTTP+gRPC 可达）");
    println!("  - 就绪检查:   ./scripts/single-node-readiness-check.sh");
    println!("  - 快速契约冒烟: ./scripts/single-node-contract-smoke.sh --quick（可选 SMOKE_BRIDGE_HTTP=...）");
    println!("  - 全量契约冒烟: make smoke-single-node（耗时较长）");
    Ok(())
}
