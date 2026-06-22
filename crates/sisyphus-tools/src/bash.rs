use std::path::Path;
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use crate::{ToolError, ToolResult};

#[derive(Debug)]
pub struct BashOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

pub async fn execute(command: &str, workdir: Option<&Path>, timeout_ms: u64) -> ToolResult<BashOutput> {
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(command)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    if let Some(dir) = workdir {
        cmd.current_dir(dir);
    }

    let child = cmd.spawn().map_err(ToolError::Io)?;
    
    let result = timeout(Duration::from_millis(timeout_ms), child.wait_with_output()).await;

    match result {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
            let exit_code = output.status.code().unwrap_or(-1);

            if exit_code == 0 {
                Ok(BashOutput { stdout, stderr, exit_code })
            } else {
                Err(ToolError::CommandFailed { code: exit_code, stdout, stderr })
            }
        }
        Ok(Err(e)) => Err(ToolError::Io(e)),
        Err(_) => {
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
    async fn test_execute_stderr() {
        let result = execute("echo stderr_msg >&2; exit 0", None, 5000).await.unwrap();
        assert_eq!(result.stderr.trim(), "stderr_msg");
    }
}
