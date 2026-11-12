use async_trait::async_trait;
use sisyphus_tools_core::ToolResult;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};

#[async_trait]
/// Mcp Transport.
pub trait McpTransport: Send + 'static {
    /// Send a JSON-RPC message and receive the response.
    async fn request(&mut self, message: &str) -> ToolResult<String>;
    /// Send a JSON-RPC **notification**（按协议不会收到响应）。
    ///
    /// 默认实现仍然走 `request`，对「任何消息都回一行」的传输是安全的；
    /// stdio/http 必须覆写：MCP 通知是 fire-and-forget，若同步等响应会一直
    /// 阻塞到超时（实测 `initialize` 的 `notifications/initialized` 会卡满
    /// 120s 才报错，合规服务器根本不会回它）。
    async fn notify(&mut self, message: &str) -> ToolResult<()> {
        let _ = self.request(message).await?;
        Ok(())
    }
    /// Close the transport connection.
    async fn close(&mut self) -> ToolResult<()>;
}

pub struct StdioTransport {
    child: Child,
    reader: BufReader<tokio::process::ChildStdout>,
    writer: tokio::io::BufWriter<tokio::process::ChildStdin>,
}

/// Cap on retained server stderr.
const STDERR_TAIL_CAP: usize = 64 * 1024;
/// Ceiling on one JSON-RPC line, so a runaway server cannot exhaust memory.
const MAX_RESPONSE_LINE: usize = 64 * 1024 * 1024;
/// Upper bound on one request/response exchange.
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

impl StdioTransport {
    /// Spawn.
    pub fn spawn(command: &str, args: &[&str]) -> ToolResult<Self> {
        let mut child = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(sisyphus_tools_core::ToolError::Io)?;

        let stdout = child.stdout.take().ok_or_else(|| {
            sisyphus_tools_core::ToolError::Io(std::io::Error::other("failed to capture stdout"))
        })?;
        let stdin = child.stdin.take().ok_or_else(|| {
            sisyphus_tools_core::ToolError::Io(std::io::Error::other("failed to capture stdin"))
        })?;

        // The pipe must be drained. Leaving stderr unread lets the server fill
        // the pipe buffer and block forever, while we sit waiting on stdout for
        // a response that will then never arrive.
        if let Some(stderr) = child.stderr.take() {
            let sink = Arc::new(parking_lot::Mutex::new(String::new()));
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr);
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            let mut buf = sink.lock();
                            buf.push_str(&line);
                            if buf.len() > STDERR_TAIL_CAP {
                                let drop_to = buf.len() - STDERR_TAIL_CAP;
                                let keep = drop_to
                                    + (buf.as_bytes()[drop_to..]
                                        .iter()
                                        .position(|&b| b == b'\n')
                                        .map(|i| i + 1)
                                        .unwrap_or(0));
                                buf.drain(..keep);
                            }
                        },
                    }
                }
            });
        }

        Ok(Self {
            child,
            reader: BufReader::new(stdout),
            writer: tokio::io::BufWriter::new(stdin),
        })
    }
}

#[async_trait]
impl McpTransport for StdioTransport {
    async fn request(&mut self, message: &str) -> ToolResult<String> {
        // Write the message followed by a newline (MCP stdio uses newline-delimited JSON)
        self.writer
            .write_all(message.as_bytes())
            .await
            .map_err(sisyphus_tools_core::ToolError::Io)?;
        self.writer.write_all(b"\n").await.map_err(sisyphus_tools_core::ToolError::Io)?;
        self.writer.flush().await.map_err(sisyphus_tools_core::ToolError::Io)?;

        let mut line = String::new();
        tokio::time::timeout(REQUEST_TIMEOUT, self.reader.read_line(&mut line))
            .await
            .map_err(|_| {
                sisyphus_tools_core::ToolError::Io(std::io::Error::other(format!(
                    "MCP server did not respond within {}s",
                    REQUEST_TIMEOUT.as_secs()
                )))
            })?
            .map_err(sisyphus_tools_core::ToolError::Io)?;

        if line.len() > MAX_RESPONSE_LINE {
            return Err(sisyphus_tools_core::ToolError::Io(std::io::Error::other(
                format!("MCP response exceeds {MAX_RESPONSE_LINE} byte cap"),
            )));
        }

        Ok(line.trim_end().to_string())
    }

    /// 通知：不读响应（协议规定服务器不会回）。
    async fn notify(&mut self, message: &str) -> ToolResult<()> {
        self.writer
            .write_all(message.as_bytes())
            .await
            .map_err(sisyphus_tools_core::ToolError::Io)?;
        self.writer.write_all(b"\n").await.map_err(sisyphus_tools_core::ToolError::Io)?;
        self.writer.flush().await.map_err(sisyphus_tools_core::ToolError::Io)?;
        Ok(())
    }

    async fn close(&mut self) -> ToolResult<()> {
        self.child.kill().await.map_err(sisyphus_tools_core::ToolError::Io)?;
        self.child.wait().await.map_err(sisyphus_tools_core::ToolError::Io)?;
        Ok(())
    }
}

/// Http Transport.
pub struct HttpTransport {
    base_url: String,
    client: reqwest::Client,
}

impl HttpTransport {
    /// New.
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl McpTransport for HttpTransport {
    async fn request(&mut self, message: &str) -> ToolResult<String> {
        let resp = self
            .client
            .post(&self.base_url)
            .header("Content-Type", "application/json")
            .body(message.to_string())
            .send()
            .await
            .map_err(|e| sisyphus_tools_core::ToolError::WebError(e.to_string()))?;

        let body = resp
            .text()
            .await
            .map_err(|e| sisyphus_tools_core::ToolError::WebError(e.to_string()))?;

        Ok(body)
    }

    /// 通知：POST 出去即可，不等响应体（协议规定无响应）。
    async fn notify(&mut self, message: &str) -> ToolResult<()> {
        self.client
            .post(&self.base_url)
            .header("Content-Type", "application/json")
            .body(message.to_string())
            .send()
            .await
            .map_err(|e| sisyphus_tools_core::ToolError::WebError(e.to_string()))?;
        Ok(())
    }

    async fn close(&mut self) -> ToolResult<()> {
        // HTTP transport has no persistent connection to close
        Ok(())
    }
}
