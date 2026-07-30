// Sisyphus Zed Extension
// Provides: LSP diagnostics bridge via language_server_command,
// slash commands for status/execution, and detailed installation guidance.

use std::collections::HashMap;
use zed_extension_api::{self as zed, lsp, Command, Result};

/// Bridge URL for the Sisyphus cognitive engine REST API.
const DEFAULT_BRIDGE_URL: &str = "http://localhost:8080";
const BRIDGE_TIMEOUT_SECS: u64 = 10;

struct SisyphusExtension;

impl SisyphusExtension {
    fn new() -> Self {
        Self
    }

    /// Fetch JSON from the Sisyphus bridge and deserialize.
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
                // Use a generous timeout; some endpoints (execute) may be slow.
                secs: BRIDGE_TIMEOUT_SECS + 20,
                millis: 0,
            },
        )
        .map_err(|e| format!("Bridge request failed: {}", e))?;

        serde_json::from_slice(&resp.body)
            .map_err(|e| format!("JSON decode failed: {}", e))
    }

    /// Post JSON to the Sisyphus bridge and deserialize the response.
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

    // ── LSP Diagnostics Bridge ──────────────────────
    //
    // `language_server_command` configures an LSP server that proxies
    // diagnostics from the Sisyphus bridge.  Because Sisyphus does not
    // ship a native LSP binary, we provide the command specification
    // here so Zed can start it when available (or a user-supplied
    // shim).
    //
    // Language server ID: "sisyphus-lsp"
    //
    // When no binary is installed the server simply won't start; no
    // error is raised to the user.

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
                    "change": 1,        // full document sync
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
        // Pass through LSP completions with default rendering.
        // Sisyphus-specific completions are annotated with a [S] prefix.
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

    // ── Slash Commands ──────────────────────────────
    //
    // Inline commands available via the Zed assistant (/sisyphus-status,
    // /sisyphus-exec, /sisyphus-search).

    fn complete_slash_command_argument(
        &self,
        command: zed::SlashCommand,
        _args: Vec<String>,
    ) -> Result<Vec<zed::SlashCommandArgumentCompletion>, String> {
        match command.name.as_str() {
            "sisyphus-exec" => {
                // Suggest SOP IDs
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
                // No argument completion for free-text search
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

    // ── Installation Message ────────────────────────

    fn installation_message(&self) -> Option<zed::InstallationMessage> {
        Some(zed::InstallationMessage {
            heading: "Install Sisyphus Cognitive Engine".into(),
            message: format!(
                "\
Sisyphus requires a running instance of the cognitive engine bridge.

## Quick Start

```sh
# Install the bridge (requires Rust toolchain)
cargo install sisyphus-bridge

# Start the bridge (default http://localhost:8080)
sisyphus-bridge serve

# (Optional) Install the LSP shim for inline diagnostics
cargo install sisyphus-lsp
```

## Configuration

By default the extension connects to `{}`.  To use a custom address,
set the `SISYPHUS_BRIDGE_URL` environment variable before launching
Zed:

```sh
export SISYPHUS_BRIDGE_URL=\"http://127.0.0.1:9090\"
zed
```

## Slash Commands (Zed Assistant)

| Command | Description |
|---|---|
| `/sisyphus-status` | Show bridge health, version, session count |
| `/sisyphus-exec <sop-id>` | Execute a SOP by ID |
| `/sisyphus-search <query>` | Search Sisyphus memory |

## LSP Diagnostics

If `sisyphus-lsp` (or `sisyphus`) is on your PATH, Zed will start it
as a language server and display diagnostics from the Sisyphus engine
inline in your editor.",

                DEFAULT_BRIDGE_URL,
            )
            .into(),
            check: Box::new(|| {
                // Check if either `sisyphus` or `sisyphus-lsp` is available
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

// ── Command Helpers ────────────────────────────────

impl SisyphusExtension {
    /// `/sisyphus-status`: show bridge health, version, session count.
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
            "## Sisyphus Status\n\n\
             - **Version**: {}\n\
             - **Status**: {}\n\
             - **Uptime**: {:.0}s\n\
             - **Sessions**: {} ({} active)\n",
            health.version,
            health.status,
            health.uptime_secs,
            sessions.len(),
            active,
        ))
    }

    /// `/sisyphus-exec <sop_id>`: execute a SOP and return the result.
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
            format!("\n\n**Summary**: {}", result.summary)
        };

        Ok(format!(
            "## SOP Execution: `{}`\n\n\
             - **Status**: {}\n\
             - **Iterations**: {}{}",
            sop_id, result.status, result.iterations, summary,
        ))
    }

    /// `/sisyphus-search <query>`: search Sisyphus memory.
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
            return Ok(format!("## Search: `{}`\n\nNo results found.", query));
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

        Ok(format!("## Search: `{}`\n\n{}", query, rows))
    }
}

zed::register_extension!(SisyphusExtension);
