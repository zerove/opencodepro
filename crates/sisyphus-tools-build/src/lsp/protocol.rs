//! LSP wire protocol: Content-Length framed JSON-RPC messages.

use super::types::LspResult;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

/// Read a single LSP message from a buffered reader.
///
/// Expects `Content-Length: <N>\r\n\r\n<body_of_N_bytes>` framing.
pub async fn read_message(
    reader: &mut BufReader<tokio::process::ChildStdout>,
) -> LspResult<String> {
    let mut header = String::new();
    loop {
        header.clear();
        let n = reader
            .read_line(&mut header)
            .await
            .map_err(|e| super::types::LspError::Transport(e.to_string()))?;
        if n == 0 {
            return Err(super::types::LspError::ServerDied);
        }
        let trimmed = header.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some(len_str) = trimmed.strip_prefix("Content-Length: ") {
            let len: usize = len_str.trim().parse().map_err(|_| {
                super::types::LspError::Protocol(format!("invalid Content-Length: {len_str}"))
            })?;
            let mut buf = vec![0u8; len];
            reader
                .read_exact(&mut buf)
                .await
                .map_err(|e| super::types::LspError::Transport(e.to_string()))?;
            return String::from_utf8(buf).map_err(|e| {
                super::types::LspError::Protocol(format!("invalid UTF-8 in body: {e}"))
            });
        }
    }
    Err(super::types::LspError::Protocol(
        "no Content-Length header".into(),
    ))
}

/// Write a single LSP message using Content-Length framing.
pub async fn write_message(
    writer: &mut tokio::io::BufWriter<tokio::process::ChildStdin>,
    body: &str,
) -> LspResult<()> {
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    writer
        .write_all(header.as_bytes())
        .await
        .map_err(|e| super::types::LspError::Transport(e.to_string()))?;
    writer
        .write_all(body.as_bytes())
        .await
        .map_err(|e| super::types::LspError::Transport(e.to_string()))?;
    writer
        .flush()
        .await
        .map_err(|e| super::types::LspError::Transport(e.to_string()))?;
    Ok(())
}
