use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};

// ── Minimal LSP types (no lsp-types dependency) ────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspPosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspRange {
    pub start: LspPosition,
    pub end: LspPosition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspDiagnostic {
    pub range: LspRange,
    #[serde(default)]
    pub severity: Option<i64>,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspLocation {
    pub uri: String,
    pub range: LspRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspHover {
    pub contents: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspDocumentSymbol {
    pub name: String,
    pub kind: i64,
    pub range: LspRange,
    pub selection_range: LspRange,
    #[serde(default)]
    pub children: Option<Vec<LspDocumentSymbol>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspCompletionItem {
    pub label: String,
    #[serde(default)]
    pub detail: Option<String>,
}

pub fn severity_name(s: i64) -> &'static str {
    match s {
        1 => "error",
        2 => "warning",
        3 => "info",
        4 => "hint",
        _ => "?",
    }
}

// ── Error ──────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum LspError {
    #[error("LSP transport error: {0}")]
    Transport(String),
    #[error("LSP protocol error: {0}")]
    Protocol(String),
    #[error("LSP server error (code {code}): {message}")]
    ServerError { code: i64, message: String },
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serde error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("Language server exited unexpectedly")]
    ServerDied,
}

type LspResult<T> = Result<T, LspError>;

// ── Content-Length framing ─────────────────────────────────

async fn read_message(reader: &mut BufReader<tokio::process::ChildStdout>) -> LspResult<String> {
    let mut header = String::new();
    loop {
        header.clear();
        let n = reader
            .read_line(&mut header)
            .await
            .map_err(|e| LspError::Transport(e.to_string()))?;
        if n == 0 {
            return Err(LspError::ServerDied);
        }
        let trimmed = header.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some(len_str) = trimmed.strip_prefix("Content-Length: ") {
            let len: usize = len_str
                .trim()
                .parse()
                .map_err(|_| LspError::Protocol(format!("invalid Content-Length: {len_str}")))?;
            let mut buf = vec![0u8; len];
            reader
                .read_exact(&mut buf)
                .await
                .map_err(|e| LspError::Transport(e.to_string()))?;
            return String::from_utf8(buf)
                .map_err(|e| LspError::Protocol(format!("invalid UTF-8 in body: {e}")));
        }
    }
    Err(LspError::Protocol("no Content-Length header".into()))
}

async fn write_message(writer: &mut tokio::io::BufWriter<tokio::process::ChildStdin>, body: &str) -> LspResult<()> {
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    writer
        .write_all(header.as_bytes())
        .await
        .map_err(|e| LspError::Transport(e.to_string()))?;
    writer
        .write_all(body.as_bytes())
        .await
        .map_err(|e| LspError::Transport(e.to_string()))?;
    writer
        .flush()
        .await
        .map_err(|e| LspError::Transport(e.to_string()))?;
    Ok(())
}

// ── LSP Client ─────────────────────────────────────────────

pub struct LspClient {
    #[allow(dead_code)]
    child: Option<Child>,
    reader: BufReader<tokio::process::ChildStdout>,
    writer: tokio::io::BufWriter<tokio::process::ChildStdin>,
    next_id: u64,
    #[allow(dead_code)]
    capabilities: Value,
    stderr_buf: Arc<Mutex<String>>,
}

impl LspClient {
    pub async fn spawn(command: &str, args: &[&str], root_uri: &str) -> LspResult<Self> {
        let mut child = Command::new(command)
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
        self.stderr_buf.lock().map(|mut b| std::mem::take(&mut *b)).unwrap_or_default()
    }

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
        self.send_notification("initialized", serde_json::json!({})).await?;
        Ok(())
    }

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

    pub async fn shutdown(&mut self) -> LspResult<()> {
        let _ = self.send_request("shutdown", serde_json::json!(null)).await;
        self.send_notification("exit", serde_json::json!({})).await?;
        if let Some(mut child) = self.child.take() {
            let _ = child.wait().await;
        }
        Ok(())
    }

    // ── Diagnostics ───────────────────────────────────────

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

    pub async fn get_symbols(&mut self, file_path: &str) -> LspResult<Vec<LspDocumentSymbol>> {
        let uri = to_file_uri(file_path);
        let params = serde_json::json!({
            "textDocument": { "uri": uri }
        });
        let result = self.send_request("textDocument/documentSymbol", params).await?;

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

    #[allow(dead_code)]
    pub async fn get_completions(
        &mut self,
        file_path: &str,
        line: u32,
        col: u32,
    ) -> LspResult<Vec<LspCompletionItem>> {
        let uri = to_file_uri(file_path);
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": col },
            "context": { "triggerKind": 1 }
        });
        let result = self.send_request("textDocument/completion", params).await?;

        if let Ok(items) = serde_json::from_value::<Vec<LspCompletionItem>>(result.clone()) {
            return Ok(items);
        }
        let list: Value = result;
        let items = list.get("items").cloned().unwrap_or(Value::Null);
        serde_json::from_value(items)
            .map_err(|e| LspError::Protocol(format!("deserialize completions: {e}")))
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
        }
    }
}

// ── Helpers ────────────────────────────────────────────────

fn extract_hover_text(contents: &Value) -> String {
    match contents {
        Value::String(s) => s.clone(),
        Value::Object(map) => {
            if let Some(val) = map.get("value").and_then(|v| v.as_str()) {
                return val.to_string();
            }
            contents.to_string()
        }
        Value::Array(arr) => arr
            .iter()
            .map(extract_hover_text)
            .collect::<Vec<_>>()
            .join("\n"),
        _ => contents.to_string(),
    }
}

fn to_file_uri(path: &str) -> String {
    let p = std::path::Path::new(path);
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        let Ok(cwd) = std::env::current_dir() else {
            return format!("file://{path}");
        };
        cwd.join(path)
    };
    // Use to_string_lossy for a clean path string (no double-slash from components join).
    let canonical = abs.to_string_lossy().replace('\\', "/");
    if canonical.starts_with('/') {
        format!("file://{canonical}")
    } else {
        format!("file:///{canonical}")
    }
}

// ── Tests ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_file_uri_absolute() {
        let uri = to_file_uri("/home/user/file.rs");
        assert_eq!(uri, "file:///home/user/file.rs");
    }

    #[test]
    fn test_to_file_uri_relative_does_not_crash() {
        let uri = to_file_uri("src/lib.rs");
        assert!(uri.starts_with("file://"));
        assert!(uri.contains("src/lib.rs"));
    }

    #[test]
    fn test_severity_name() {
        assert_eq!(severity_name(1), "error");
        assert_eq!(severity_name(2), "warning");
        assert_eq!(severity_name(3), "info");
        assert_eq!(severity_name(4), "hint");
        assert_eq!(severity_name(99), "?");
    }

    #[test]
    fn test_extract_hover_markup() {
        let json = serde_json::json!({"kind": "markdown", "value": "hello **world**"});
        assert_eq!(extract_hover_text(&json), "hello **world**");
    }

    #[test]
    fn test_extract_hover_string() {
        let json = serde_json::json!("simple text");
        assert_eq!(extract_hover_text(&json), "simple text");
    }

    #[test]
    fn test_extract_hover_array() {
        let json = serde_json::json!(["a", "b", "c"]);
        assert_eq!(extract_hover_text(&json), "a\nb\nc");
    }

    #[test]
    fn test_deserialize_diagnostic() {
        let json = serde_json::json!({
            "range": { "start": { "line": 1, "character": 5 }, "end": { "line": 1, "character": 10 } },
            "severity": 1,
            "message": "unused variable"
        });
        let d: LspDiagnostic = serde_json::from_value(json).unwrap();
        assert_eq!(d.message, "unused variable");
        assert_eq!(d.severity, Some(1));
        assert_eq!(d.range.start.line, 1);
        assert_eq!(d.range.end.character, 10);
    }

    #[test]
    fn test_deserialize_location() {
        let json = serde_json::json!({
            "uri": "file:///test.rs",
            "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 5 } }
        });
        let loc: LspLocation = serde_json::from_value(json).unwrap();
        assert_eq!(loc.uri, "file:///test.rs");
    }

    #[test]
    fn test_serde_lsp_position() {
        let json = serde_json::json!({ "line": 42, "character": 7 });
        let pos: LspPosition = serde_json::from_value(json).unwrap();
        assert_eq!(pos.line, 42);
        assert_eq!(pos.character, 7);
    }

    #[test]
    fn test_serde_lsp_range() {
        let json = serde_json::json!({
            "start": { "line": 1, "character": 2 },
            "end": { "line": 3, "character": 4 }
        });
        let r: LspRange = serde_json::from_value(json).unwrap();
        assert_eq!(r.start.line, 1);
        assert_eq!(r.end.character, 4);
    }

    #[test]
    fn test_serde_lsp_completion_item() {
        let json = serde_json::json!({ "label": "foo", "detail": "fn()" });
        let item: LspCompletionItem = serde_json::from_value(json).unwrap();
        assert_eq!(item.label, "foo");
        assert_eq!(item.detail.unwrap(), "fn()");
    }

    #[test]
    fn test_serde_lsp_completion_item_no_detail() {
        let json = serde_json::json!({ "label": "bar" });
        let item: LspCompletionItem = serde_json::from_value(json).unwrap();
        assert_eq!(item.label, "bar");
        assert!(item.detail.is_none());
    }

    #[test]
    fn test_serde_lsp_document_symbol() {
        let json = serde_json::json!({
            "name": "my_func",
            "kind": 12,
            "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 5, "character": 0 } },
            "selection_range": { "start": { "line": 0, "character": 0 }, "end": { "line": 5, "character": 0 } }
        });
        let sym: LspDocumentSymbol = serde_json::from_value(json).unwrap();
        assert_eq!(sym.name, "my_func");
        assert_eq!(sym.kind, 12);
        assert!(sym.children.is_none());
    }

    #[test]
    fn test_deserialize_diagnostic_no_severity() {
        let json = serde_json::json!({
            "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 1 } },
            "message": "info message"
        });
        let d: LspDiagnostic = serde_json::from_value(json).unwrap();
        assert_eq!(d.message, "info message");
        assert_eq!(d.severity, None);
    }

    #[test]
    fn test_to_file_uri_windows_style() {
        let uri = to_file_uri("C:\\Users\\test\\file.rs");
        assert!(uri.starts_with("file:///"));
        assert!(uri.contains("file.rs"));
    }

    #[test]
    fn test_to_file_uri_with_spaces() {
        let uri = to_file_uri("/home/user/my project/main.rs");
        assert_eq!(uri, "file:///home/user/my project/main.rs");
    }

    #[test]
    fn test_to_file_uri_root() {
        let uri = to_file_uri("/");
        assert_eq!(uri, "file:///");
    }

    #[test]
    fn test_extract_hover_markup_with_kind() {
        let json = serde_json::json!({"kind": "plaintext", "value": "fn foo()"});
        assert_eq!(extract_hover_text(&json), "fn foo()");
    }

    #[test]
    fn test_extract_hover_object_no_value() {
        let json = serde_json::json!({"language": "rust", "foo": "bar"});
        let text = extract_hover_text(&json);
        assert!(!text.is_empty());
    }

    #[test]
    fn test_extract_hover_mixed_array() {
        let json = serde_json::json!(["header", {"kind": "markdown", "value": "body"}]);
        let text = extract_hover_text(&json);
        assert_eq!(text, "header\nbody");
    }

    #[test]
    fn test_extract_hover_null_and_number() {
        assert_eq!(extract_hover_text(&Value::Null), "null");
        assert_eq!(extract_hover_text(&serde_json::json!(42)), "42");
    }

    #[test]
    fn test_lsp_error_display() {
        let e = LspError::Transport("connection refused".into());
        assert!(e.to_string().contains("connection refused"));

        let e = LspError::Protocol("bad header".into());
        assert!(e.to_string().contains("bad header"));

        let e = LspError::ServerError { code: -32601, message: "method not found".into() };
        let s = e.to_string();
        assert!(s.contains("-32601"));
        assert!(s.contains("method not found"));

        let e = LspError::ServerDied;
        assert!(e.to_string().contains("unexpectedly"));
    }
}
