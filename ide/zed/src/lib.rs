// Sisyphus Zed 扩展
// 提供: 通过 language_server_command 的 LSP 诊断桥接、
// 状态/执行的斜杠命令，以及详细的安装指引。

use std::collections::HashMap;
use zed_extension_api::{self as zed, lsp, Command, Result};

/// Sisyphus 认知引擎 REST API 的桥接 URL。
const DEFAULT_BRIDGE_URL: &str = "http://localhost:8080";
const BRIDGE_TIMEOUT_SECS: u64 = 10;

struct SisyphusExtension;

impl SisyphusExtension {
    fn new() -> Self {
        Self
    }

    /// 从 Sisyphus bridge 获取 JSON 并反序列化。
    fn fetch_bridge<T: serde::de::DeserializeOwned>(path: &str) -> Result<T> {
        let url = format!("{}{}", DEFAULT_BRIDGE_URL, path);
        let resp = zed::http_client::fetch(
            &zed::http_client::HttpRequest {
                method: "GET".into(),
                url,
                headers: HashMap::from([
                    ("Content-Type".into(), "application/json".into()),
                ]),
                body: None,
            },
            zed::http_client::HttpTimeout {
                // 使用较宽裕的超时；部分端点（如 execute）可能较慢。
                secs: BRIDGE_TIMEOUT_SECS + 20,
                millis: 0,
            },
        )
        .map_err(|e| format!("Bridge request failed: {}", e))?;

        serde_json::from_slice(&resp.body)
            .map_err(|e| format!("JSON decode failed: {}", e))
    }

    /// 向 Sisyphus bridge 发送 JSON 并反序列化响应。
    fn post_bridge<T: serde::de::DeserializeOwned>(path: &str, body: &str) -> Result<T> {
        let url = format!("{}{}", DEFAULT_BRIDGE_URL, path);
        let resp = zed::http_client::fetch(
            &zed::http_client::HttpRequest {
                method: "POST".into(),
                url,
                headers: HashMap::from([
                    ("Content-Type".into(), "application/json".into()),
                ]),
                body: Some(body.as_bytes().to_vec()),
            },
            zed::http_client::HttpTimeout {
                secs: BRIDGE_TIMEOUT_SECS + 20,
                millis: 0,
            },
        )
        .map_err(|e| format!("Bridge POST failed: {}", e))?;

        serde_json::from_slice(&resp.body)
            .map_err(|e| format!("JSON decode failed: {}", e))
    }
}

impl zed::Extension for SisyphusExtension {
    fn new() -> Self {
        Self::new()
    }

    fn label(&self) -> &str {
        "Sisyphus"
    }

    // ── LSP 诊断桥接 ──────────────────────
    //
    // `language_server_command` 配置一个 LSP 服务器，代理来自
    // Sisyphus bridge 的诊断。由于 Sisyphus 不附带原生 LSP 二进制，
    // 我们在此提供命令规格，Zed 可在可用时启动它（或使用用户提供的
    // shim）。
    //
    // 语言服务器 ID: "sisyphus-lsp"
    //
    // 未安装二进制时服务器不会启动；不会向用户报错。

    fn language_server_command(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<Command> {
        let sisyphus_bin = worktree
            .which("sisyphus-lsp")
            .or_else(|| worktree.which("sisyphus"))
            .ok_or_else(|| {
                format!(
                    "Sisyphus LSP not found. Install via:\n  cargo install sisyphus-bridge\n\
                     Then symlink or alias to `sisyphus-lsp` or ensure `sisyphus` is on PATH."
                )
            })?;

        Ok(Command {
            command: sisyphus_bin,
            args: vec!["lsp".into()],
            env: worktree.shell_env(),
        })
    }

    fn language_server_workspace_configuration(
        &mut self,
        _language_server_id: &zed::LanguageServerId,
        _worktree: &zed::Worktree,
    ) -> Result<Option<serde_json::Value>> {
        Ok(Some(serde_json::json!({
            "bridge_url": DEFAULT_BRIDGE_URL,
            "fetch_interval_secs": 30,
        })))
    }

    fn language_server_initialization_options(
        &mut self,
        _language_server_id: &zed::LanguageServerId,
        _worktree: &zed::Worktree,
    ) -> Result<Option<serde_json::Value>> {
        Ok(Some(serde_json::json!({
            "capabilities": {
                "textDocumentSync": {
                    "change": 1,        // 全文同步
                    "openClose": true,
                },
                "diagnosticProvider": {
                    "interFileDependencies": false,
                    "workspaceDiagnostics": false,
                },
            },
            "bridgeUrl": DEFAULT_BRIDGE_URL,
        })))
    }

    fn label_for_completion(
        &self,
        _language_server_id: &zed::LanguageServerId,
        completion: lsp::Completion,
    ) -> Option<zed::CodeLabel> {
        // 透传 LSP 补全并使用默认渲染。
        // Sisyphus 专属补全带有 [S] 前缀。
        let label = completion.label;
        if label.starts_with("[S]") {
            Some(zed::CodeLabel {
                code: label,
                tooltip: Some("Sisyphus completion".into()),
                filter_range: (0, label.len()).into(),
            })
        } else {
            None
        }
    }

    // ── 斜杠命令 ──────────────────────────────
    //
    // 通过 Zed 助手可用的内联命令 (/sisyphus-status、
    // /sisyphus-exec、/sisyphus-search)。

    fn complete_slash_command_argument(
        &self,
        command: zed::SlashCommand,
        _args: Vec<String>,
    ) -> Result<Vec<zed::SlashCommandArgumentCompletion>, String> {
        match command.name.as_str() {
            "sisyphus-exec" => {
                // 建议 SOP ID
                let sops: Vec<String> = Self::fetch_bridge::<Vec<String>>("/api/sops/list")
                    .unwrap_or_default();
                Ok(sops
                    .into_iter()
                    .map(|id| zed::SlashCommandArgumentCompletion {
                        label: id.clone(),
                        new_text: id,
                        after_completion: " ".into(),
                    })
                    .collect())
            }
            "sisyphus-search" => {
                // 自由文本搜索不做参数补全
                Ok(vec![])
            }
            _ => Ok(vec![]),
        }
    }

    fn run_slash_command(
        &self,
        command: zed::SlashCommand,
        args: Vec<String>,
        _worktree: Option<&zed::Worktree>,
    ) -> Result<zed::SlashCommandOutput, String> {
        let output_text = match command.name.as_str() {
            "sisyphus-status" => self.cmd_status(),
            "sisyphus-exec" => {
                let sop_id = args.first().map(|s| s.as_str()).unwrap_or("default");
                self.cmd_exec_sop(sop_id)
            }
            "sisyphus-search" => {
                let query = args.join(" ");
                self.cmd_search(&query)
            }
            _ => Err(format!("Unknown command: /{}", command.name)),
        }?;

        Ok(zed::SlashCommandOutput {
            text: output_text,
            sections: vec![zed::SlashCommandOutputSection {
                range: (0, output_text.len()).into(),
                label: "Sisyphus".into(),
            }],
        })
    }

    // ── 安装信息 ────────────────────────

    fn installation_message(&self) -> Option<zed::InstallationMessage> {
        Some(zed::InstallationMessage {
            heading: "安装 Sisyphus 认知引擎".into(),
            message: format!(
                "\
Sisyphus 需要运行中的认知引擎 bridge 实例。

## 快速开始

```sh
# 安装 bridge（需要 Rust 工具链）
cargo install sisyphus-bridge

# 启动 bridge (默认 http://localhost:8080)
sisyphus-bridge serve

# (可选) 安装 LSP shim 以获取内联诊断
cargo install sisyphus-lsp
```

## 配置

默认情况下扩展连接到 `{}`。要使用自定义地址，请在启动
Zed 前设置 `SISYPHUS_BRIDGE_URL` 环境变量：

```sh
export SISYPHUS_BRIDGE_URL=\"http://127.0.0.1:9090\"
zed
```

## 斜杠命令（Zed 助手）

| 命令 | 说明 |
|---|---|
| `/sisyphus-status` | 显示 bridge 健康状态、版本、会话数 |
| `/sisyphus-exec <sop-id>` | 按 ID 执行一个 SOP |
| `/sisyphus-search <query>` | 搜索 Sisyphus 记忆 |

## LSP 诊断

如果 `sisyphus-lsp`（或 `sisyphus`）在您的 PATH 中，Zed 会将其作为
语言服务器启动，并将 Sisyphus 引擎的诊断内联显示在编辑器中。",

                DEFAULT_BRIDGE_URL,
            )
            .into(),
            check: Box::new(|| {
                // 检查 `sisyphus` 或 `sisyphus-lsp` 是否可用
                std::process::Command::new("sisyphus")
                    .arg("--version")
                    .output()
                    .map(|o| o.status.success())
                    .or_else(|_| {
                        std::process::Command::new("sisyphus-lsp")
                            .arg("--version")
                            .output()
                            .map(|o| o.status.success())
                    })
                    .unwrap_or(false)
            }),
        })
    }
}

// ── 命令辅助函数 ────────────────────────────────

impl SisyphusExtension {
    /// `/sisyphus-status`: 显示 bridge 健康状态、版本、会话数。
    fn cmd_status(&self) -> Result<String, String> {
        #[derive(serde::Deserialize)]
        struct Health {
            status: String,
            version: String,
            uptime_secs: f64,
        }
        #[derive(serde::Deserialize)]
        struct Session {
            id: String,
            status: String,
        }

        let health: Health = Self::fetch_bridge("/health")?;
        let sessions: Vec<Session> = Self::fetch_bridge("/api/sessions").unwrap_or_default();

        let active = sessions.iter().filter(|s| s.status == "active").count();
        Ok(format!(
            "## Sisyphus 状态\n\n\
             - **版本**: {}\n\
             - **状态**: {}\n\
             - **运行时长**: {:.0}s\n\
             - **会话数**: {} ({} 个活跃)\n",
            health.version,
            health.status,
            health.uptime_secs,
            sessions.len(),
            active,
        ))
    }

    /// `/sisyphus-exec <sop_id>`: 执行一个 SOP 并返回结果。
    fn cmd_exec_sop(&self, sop_id: &str) -> Result<String, String> {
        #[derive(serde::Deserialize)]
        struct ExecResult {
            status: String,
            iterations: u64,
            #[serde(default)]
            summary: String,
        }

        let body = serde_json::json!({ "sop_id": sop_id, "context": {} });
        let result: ExecResult = Self::post_bridge(
            "/api/execute",
            &serde_json::to_string(&body).map_err(|e| e.to_string())?,
        )?;

        let summary = if result.summary.is_empty() {
            String::new()
        } else {
            format!("\n\n**摘要**: {}", result.summary)
        };

        Ok(format!(
            "## SOP 执行: `{}`\n\n\
             - **状态**: {}\n\
             - **迭代次数**: {}{}",
            sop_id, result.status, result.iterations, summary,
        ))
    }

    /// `/sisyphus-search <query>`: 搜索 Sisyphus 记忆。
    fn cmd_search(&self, query: &str) -> Result<String, String> {
        if query.is_empty() {
            return Err("Usage: /sisyphus-search <query>".into());
        }

        #[derive(serde::Deserialize)]
        struct SearchHit {
            id: String,
            score: f64,
            #[serde(default)]
            content: String,
        }

        let body = serde_json::json!({ "query": query, "top_k": 5 });
        let hits: Vec<SearchHit> = Self::post_bridge(
            "/api/search",
            &serde_json::to_string(&body).map_err(|e| e.to_string())?,
        )?;

        if hits.is_empty() {
            return Ok(format!("## 搜索: `{}`\n\n未找到结果。", query));
        }

        let rows: String = hits
            .iter()
            .enumerate()
            .map(|(i, h)| {
                let snippet = if h.content.len() > 150 {
                    format!("{}...", &h.content[..150])
                } else {
                    h.content.clone()
                };
                format!("{}. **{}** (score: {:.3})\n   {}\n", i + 1, h.id, h.score, snippet)
            })
            .collect();

        Ok(format!("## 搜索: `{}`\n\n{}", query, rows))
    }
}

zed::register_extension!(SisyphusExtension);
