use async_trait::async_trait;
use sisyphus_tools_core::ToolResult;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};

#[async_trait]
/// Mcp Transport.
pub trait McpTransport: Send + 'static {
    /// Send a JSON-RPC message and receive the response.
    async fn request(&mut self, message: &str) -> ToolResult<String>;
    /// Close the transport connection.
    async fn close(&mut self) -> ToolResult<()>;
}

pub struct StdioTransport {
    child: Child,
    reader: BufReader<tokio::process::ChildStdout>,
    writer: tokio::io::BufWriter<tokio::process::ChildStdin>,
}

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

        // Read the response line
        let mut line = String::new();
        self.reader.read_line(&mut line).await.map_err(sisyphus_tools_core::ToolError::Io)?;

        Ok(line.trim_end().to_string())
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

    async fn close(&mut self) -> ToolResult<()> {
        // HTTP transport has no persistent connection to close
        Ok(())
    }
}
