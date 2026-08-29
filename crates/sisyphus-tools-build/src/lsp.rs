//! LSP client for talking to language servers.
//!
//! Spawns a language server process, manages its lifecycle, and provides
//! request/response methods for diagnostics, goto-definition, references,
//! hover, and document symbols.
//!
//! ## Sub-modules
//! - [`types`]: Shared types (LspPosition, LspRange, LspDiagnostic, LspError, etc.).
//! - [`protocol`]: Content-Length framed JSON-RPC message read/write.

pub mod protocol;
pub mod types;

pub use types::{
    extract_hover_text, severity_name, to_file_uri, LspCompletionItem, LspDiagnostic,
    LspDocumentSymbol, LspError, LspHover, LspLocation, LspPosition, LspRange, LspResult,
};

use protocol::{read_message, write_message};
use serde::Deserialize;
use serde_json::Value;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;

// ── LSP Client ─────────────────────────────────────────────

pub struct LspClient {
    child: Option<Child>,
    reader: BufReader<tokio::process::ChildStdout>,
    writer: tokio::io::BufWriter<tokio::process::ChildStdin>,
    next_id: u64,
    capabilities: Value,
    stderr_buf: Arc<Mutex<String>>,
}

impl LspClient {
    /// Spawn.
    pub async fn spawn(command: &str, args: &[&str], root_uri: &str) -> LspResult<Self> {
        let mut child = tokio::process::Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(LspError::Io)?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| LspError::Transport("no stdout".into()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| LspError::Transport("no stdin".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| LspError::Transport("no stderr".into()))?;

        let stderr_buf: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let capture = stderr_buf.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if let Ok(mut buf) = capture.lock() {
                            buf.push_str(&line);
                        }
                    }
                }
            }
        });

        let mut client = Self {
            child: Some(child),
            reader: BufReader::new(stdout),
            writer: tokio::io::BufWriter::new(stdin),
            next_id: 1,
            capabilities: Value::Null,
            stderr_buf,
        };

        client.initialize(root_uri).await?;
        Ok(client)
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    async fn send_request(&mut self, method: &str, params: Value) -> LspResult<Value> {
        let id = self.next_id();
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        write_message(&mut self.writer, &req.to_string()).await?;

        let body = read_message(&mut self.reader).await?;
        let resp: Value = serde_json::from_str(&body)?;

        if let Some(err) = resp.get("error") {
            return Err(LspError::ServerError {
                code: err["code"].as_i64().unwrap_or(-1),
                message: err["message"].as_str().unwrap_or("unknown").to_string(),
            });
        }
        match resp.get("result") {
            Some(val) if !val.is_null() => Ok(val.clone()),
            _ => Err(LspError::Protocol("response missing result".into())),
        }
    }

    async fn send_notification(&mut self, method: &str, params: Value) -> LspResult<()> {
        let notif = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        write_message(&mut self.writer, &notif.to_string()).await
    }

    /// Drain the captured stderr output from the server process.
    pub fn take_stderr(&mut self) -> String {
        self.stderr_buf
            .lock()
            .map(|mut b| std::mem::take(&mut *b))
            .unwrap_or_default()
    }

    /// Capabilities.
    pub fn capabilities(&self) -> &Value {
        &self.capabilities
    }

    // ── Lifecycle ─────────────────────────────────────────

    async fn initialize(&mut self, root_uri: &str) -> LspResult<()> {
        let params = serde_json::json!({
            "processId": null,
            "clientInfo": { "name": "sisyphus-cli", "version": "0.1.0" },
            "rootUri": root_uri,
            "capabilities": {
                "textDocument": {
                    "diagnostics": { "dynamicRegistration": false },
                    "definition": { "dynamicRegistration": false },
                    "references": { "dynamicRegistration": false },
                    "hover": { "dynamicRegistration": false },
                    "documentSymbol": { "dynamicRegistration": false },
                    "completion": { "dynamicRegistration": false }
                },
                "workspace": { "diagnostics": true }
            }
        });
        let result = self.send_request("initialize", params).await?;
        self.capabilities = result.get("capabilities").cloned().unwrap_or(Value::Null);
        self.send_notification("initialized", serde_json::json!({}))
            .await?;
        Ok(())
    }

    /// Open document.
    pub async fn open_document(&mut self, file_path: &str, language_id: &str) -> LspResult<()> {
        let content = tokio::fs::read_to_string(file_path)
            .await
            .map_err(|e| LspError::Transport(format!("read file: {e}")))?;
        let uri = to_file_uri(file_path);
        let params = serde_json::json!({
            "textDocument": {
                "uri": uri,
                "languageId": language_id,
                "version": 1,
                "text": content,
            }
        });
        self.send_notification("textDocument/didOpen", params).await
    }

    /// Shutdown.
    pub async fn shutdown(&mut self) -> LspResult<()> {
        let _ = self.send_request("shutdown", serde_json::json!(null)).await;
        self.send_notification("exit", serde_json::json!({}))
            .await?;
        if let Some(mut child) = self.child.take() {
            let _ = child.wait().await;
        }
        Ok(())
    }

    // ── Diagnostics ───────────────────────────────────────

    /// Get diagnostics.
    pub async fn get_diagnostics(&mut self, file_path: &str) -> LspResult<Vec<LspDiagnostic>> {
        let uri = to_file_uri(file_path);
        let params = serde_json::json!({
            "textDocument": { "uri": uri }
        });
        let result = self.send_request("textDocument/diagnostic", params).await?;
        let items = result.get("items").cloned().unwrap_or(Value::Null);
        let diags: Vec<LspDiagnostic> = serde_json::from_value(items)
            .map_err(|e| LspError::Protocol(format!("deserialize diagnostics: {e}")))?;
        Ok(diags)
    }

    // ── Goto Definition ───────────────────────────────────

    /// Goto definition.
    pub async fn goto_definition(
        &mut self,
        file_path: &str,
        line: u32,
        col: u32,
    ) -> LspResult<Option<LspLocation>> {
        let uri = to_file_uri(file_path);
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": col }
        });
        let result = self.send_request("textDocument/definition", params).await?;
        match result {
            Value::Null => Ok(None),
            Value::Array(arr) if arr.is_empty() => Ok(None),
            Value::Array(arr) => serde_json::from_value(arr[0].clone())
                .map(Some)
                .map_err(|e| LspError::Protocol(format!("deserialize location: {e}"))),
            _ => serde_json::from_value(result)
                .map(Some)
                .map_err(|e| LspError::Protocol(format!("deserialize location: {e}"))),
        }
    }

    // ── Find References ───────────────────────────────────

    /// Find references.
    pub async fn find_references(
        &mut self,
        file_path: &str,
        line: u32,
        col: u32,
    ) -> LspResult<Vec<LspLocation>> {
        let uri = to_file_uri(file_path);
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": col },
            "context": { "includeDeclaration": true }
        });
        let result = self.send_request("textDocument/references", params).await?;
        serde_json::from_value(result)
            .map_err(|e| LspError::Protocol(format!("deserialize references: {e}")))
    }

    // ── Hover ─────────────────────────────────────────────

    /// Get hover.
    pub async fn get_hover(
        &mut self,
        file_path: &str,
        line: u32,
        col: u32,
    ) -> LspResult<Option<LspHover>> {
        let uri = to_file_uri(file_path);
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": col }
        });
        let result = self.send_request("textDocument/hover", params).await?;
        if result.is_null() {
            return Ok(None);
        }
        let contents = &result["contents"];
        let text = extract_hover_text(contents);
        Ok(Some(LspHover { contents: text }))
    }

    // ── Document Symbols ──────────────────────────────────

    /// Get symbols.
    pub async fn get_symbols(&mut self, file_path: &str) -> LspResult<Vec<LspDocumentSymbol>> {
        let uri = to_file_uri(file_path);
        let params = serde_json::json!({
            "textDocument": { "uri": uri }
        });
        let result = self
            .send_request("textDocument/documentSymbol", params)
            .await?;

        if let Ok(symbols) = serde_json::from_value::<Vec<LspDocumentSymbol>>(result.clone()) {
            return Ok(symbols);
        }
        #[derive(Deserialize)]
        struct SymInfo {
            name: String,
            kind: i64,
            location: LspLocation,
        }
        let info: Vec<SymInfo> = serde_json::from_value(result)
            .map_err(|e| LspError::Protocol(format!("deserialize symbols: {e}")))?;
        Ok(info
            .into_iter()
            .map(|s| LspDocumentSymbol {
                name: s.name,
                kind: s.kind,
                range: s.location.range.clone(),
                selection_range: s.location.range,
                children: None,
            })
            .collect())
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
        }
    }
}

// ── Tests ──────────────────────────────────────────────────

#[cfg(test)]
#[path = "lsp_tests.rs"]
mod tests;
