use std::path::Path;
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use tokio::io::{AsyncReadExt, BufReader};
use crate::{ToolError, ToolResult};
use std::time::Instant;

const MAX_OUTPUT_BYTES: usize = 10 * 1024 * 1024; // 10MB limit

#[derive(Debug)]
pub struct BashOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration: Duration,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

pub async fn execute(command: &str, workdir: Option<&Path>, timeout_ms: u64) -> ToolResult<BashOutput> {
    let start = Instant::now();
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(command)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    if let Some(dir) = workdir {
        cmd.current_dir(dir);
    }

    let mut child = cmd.spawn().map_err(ToolError::Io)?;
    
    let stdout_pipe = child.stdout.take().expect("stdout pipe not captured");
    let stderr_pipe = child.stderr.take().expect("stderr pipe not captured");

    let stdout_fut = async {
        let reader = BufReader::new(stdout_pipe);
        let mut buf = Vec::new();
        let mut limit = reader.take(MAX_OUTPUT_BYTES as u64);
        let bytes_read = limit.read_to_end(&mut buf).await.unwrap_or(0);
        let truncated = bytes_read == MAX_OUTPUT_BYTES;
        let stdout = String::from_utf8_lossy(&buf).into_owned();
        (stdout, truncated)
    };

    let stderr_fut = async {
        let reader = BufReader::new(stderr_pipe);
        let mut buf = Vec::new();
        let mut limit = reader.take(MAX_OUTPUT_BYTES as u64);
        let bytes_read = limit.read_to_end(&mut buf).await.unwrap_or(0);
        let truncated = bytes_read == MAX_OUTPUT_BYTES;
        let stderr = String::from_utf8_lossy(&buf).into_owned();
        (stderr, truncated)
    };

    let result = timeout(Duration::from_millis(timeout_ms), async {
        let ((stdout, stdout_truncated), (stderr, stderr_truncated)) = tokio::join!(stdout_fut, stderr_fut);
        let exit_status = child.wait().await.map_err(ToolError::Io)?;
        Ok::<_, ToolError>((stdout, stderr, stdout_truncated, stderr_truncated, exit_status))
    }).await;

    let total_duration = start.elapsed();

    match result {
        Ok(Ok((stdout, stderr, stdout_truncated, stderr_truncated, exit_status))) => {
            let exit_code = exit_status.code().unwrap_or(-1);

            if exit_code == 0 {
                Ok(BashOutput { 
                    stdout, 
                    stderr, 
                    exit_code, 
                    duration: total_duration,
                    stdout_truncated,
                    stderr_truncated,
                })
            } else {
                Err(ToolError::CommandFailed { code: exit_code, stdout, stderr })
            }
        }
        Ok(Err(e)) => Err(e),
        Err(_) => {
            // Try to kill the child process on timeout
            let _ = child.kill().await;
            Err(ToolError::Timeout("Command timed out".to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_execute_echo() {
        let result = execute("echo hello world", None, 5000).await.unwrap();
        assert_eq!(result.stdout.trim(), "hello world");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_execute_with_workdir() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let result = execute("pwd", Some(dir.path()), 5000).await.unwrap();
        assert_eq!(result.stdout.trim(), dir.path().to_string_lossy());
    }

    #[tokio::test]
    async fn test_execute_failure() {
        let result = execute("false", None, 5000).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ToolError::CommandFailed { code, .. } => assert_eq!(code, 1),
            _ => panic!("expected CommandFailed"),
        }
    }

    #[tokio::test]
    async fn test_execute_timeout() {
        let result = execute("sleep 10", None, 100).await;
        assert!(matches!(result.unwrap_err(), ToolError::Timeout(_)));
    }

    #[tokio::test]
    async fn test_execute_duration() {
        let result = execute("sleep 0.1", None, 1000).await.unwrap();
        assert!(result.duration.as_millis() >= 100);
    }
}
