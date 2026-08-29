use crate::platform::normalize_path;
use anyhow::{anyhow, Result};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

#[derive(Debug, Clone)]
pub struct ProcessOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

pub struct CommandBuilder {
    program: String,
    args: Vec<String>,
    cwd: Option<std::path::PathBuf>,
    timeout: Option<Duration>,
}

impl CommandBuilder {
    /// New.
    pub fn new(cmd: &str) -> Self {
        Self {
            program: cmd.to_string(),
            args: Vec::new(),
            cwd: None,
            timeout: None,
        }
    }

    /// Arg.
    pub fn arg(mut self, a: &str) -> Self {
        self.args.push(a.to_string());
        self
    }

    /// Args.
    pub fn args(mut self, list: &[&str]) -> Self {
        self.args.extend(list.iter().map(|s| s.to_string()));
        self
    }

    /// Cwd.
    pub fn cwd(mut self, path: std::path::PathBuf) -> Self {
        self.cwd = Some(path);
        self
    }

    /// Timeout.
    pub fn timeout(mut self, duration: Duration) -> Self {
        self.timeout = Some(duration);
        self
    }

    /// Output.
    pub async fn output(&self) -> Result<ProcessOutput> {
        let mut cmd = self.prepare_command();

        if let Some(ref cwd) = self.cwd {
            cmd.current_dir(cwd);
        }

        let output_result = if let Some(dur) = self.timeout {
            timeout(dur, cmd.output())
                .await
                .map_err(|_| anyhow!("Command timed out after {:?}", dur))?
        } else {
            cmd.output().await
        };

        let output = output_result?;

        Ok(ProcessOutput {
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }

    /// Spawn.
    pub fn spawn(&self) -> Result<tokio::process::Child> {
        let mut cmd = self.prepare_command();

        if let Some(ref cwd) = self.cwd {
            cmd.current_dir(cwd);
        }

        cmd.spawn()
            .map_err(|e| anyhow!("Failed to spawn command: {e}"))
    }

    fn prepare_command(&self) -> Command {
        if cfg!(windows) {
            // 在 Windows 上使用 powershell 执行命令
            let mut full_cmd = self.program.clone();
            for arg in &self.args {
                full_cmd.push(' ');
                full_cmd.push_str(arg);
            }

            // 规范化命令字符串中的路径
            full_cmd = normalize_path(&full_cmd);

            let mut cmd = Command::new("powershell.exe");
            cmd.arg("-Command");
            cmd.arg(full_cmd);
            cmd
        } else {
            // 在 Unix 上直接使用程序
            let mut cmd = Command::new(&self.program);
            cmd.args(&self.args);
            cmd
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_command_builder_basic() {
        let output = CommandBuilder::new("echo").arg("hello").output().await;

        assert!(output.is_ok());
        let res = output.unwrap();
        assert!(res.stdout.contains("hello"));
    }

    #[tokio::test]
    async fn test_command_timeout() {
        let output = if cfg!(windows) {
            CommandBuilder::new("powershell.exe")
                .arg("-Command")
                .arg("Start-Sleep -Seconds 2")
                .timeout(Duration::from_millis(100))
                .output()
                .await
        } else {
            CommandBuilder::new("sleep")
                .arg("2")
                .timeout(Duration::from_millis(100))
                .output()
                .await
        };

        assert!(output.is_err());
        assert!(output.unwrap_err().to_string().contains("timed out"));
    }
}
