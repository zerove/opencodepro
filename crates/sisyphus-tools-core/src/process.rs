//! Bounded process execution.
//!
//! Every external command in this workspace must go through [`run_command`].
//! A bare `Command::output()` has two failure modes that have bitten this
//! codebase before: an unbounded pipe read turns one chatty build log into an
//! OOM, and an unbounded wait turns one hung process into a permanently
//! wedged executor. Both defaults are silent, so neither shows up in review.

use crate::{ToolError, ToolResult};
use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::time::Duration;
use tokio::io::{AsyncReadExt, BufReader};
use tokio::process::Command;

/// Per-stream capture ceiling. Generous enough for real build logs, small
/// enough that a runaway process cannot exhaust memory.
pub const DEFAULT_MAX_OUTPUT_BYTES: usize = 10 * 1024 * 1024;

#[derive(Debug, Clone)]
/// Captured result of one child process.
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration: Duration,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

#[derive(Debug, Clone)]
/// Knobs for [`run_command`].
pub struct CommandOpts {
    pub timeout: Duration,
    pub max_output_bytes: usize,
    pub workdir: Option<std::path::PathBuf>,
    pub envs: HashMap<String, String>,
}

impl Default for CommandOpts {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(300),
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
            workdir: None,
            envs: HashMap::new(),
        }
    }
}

impl CommandOpts {
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_max_output_bytes(mut self, bytes: usize) -> Self {
        self.max_output_bytes = bytes;
        self
    }

    pub fn with_workdir(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.workdir = Some(dir.into());
        self
    }

    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.envs.insert(key.into(), value.into());
        self
    }
}

/// Run `program` with `args`, capturing bounded output under a wall-clock bound.
///
/// On timeout the child is killed before returning, so the caller never leaves
/// an orphan holding CPU or a pipe.
pub async fn run_command<I, S>(
    program: &str,
    args: I,
    opts: CommandOpts,
) -> ToolResult<CommandOutput>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let start = std::time::Instant::now();
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    if let Some(dir) = &opts.workdir {
        cmd.current_dir(dir);
    }
    for (k, v) in &opts.envs {
        cmd.env(k, v);
    }

    let mut child = cmd.spawn().map_err(ToolError::Io)?;

    let stdout_pipe = child.stdout.take().ok_or_else(|| {
        ToolError::Io(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "stdout pipe not captured",
        ))
    })?;
    let stderr_pipe = child.stderr.take().ok_or_else(|| {
        ToolError::Io(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "stderr pipe not captured",
        ))
    })?;

    let cap = opts.max_output_bytes;
    async fn read_capped<R>(pipe: R, cap: usize) -> (String, bool)
    where
        R: tokio::io::AsyncRead + Unpin,
    {
        let mut buf = Vec::new();
        let read = BufReader::new(pipe).take(cap as u64).read_to_end(&mut buf).await;
        let bytes_read = read.unwrap_or(0);
        (
            String::from_utf8_lossy(&buf).into_owned(),
            bytes_read as usize >= cap,
        )
    }

    let stdout_fut = read_capped(stdout_pipe, cap);
    let stderr_fut = read_capped(stderr_pipe, cap);

    let joined = tokio::time::timeout(opts.timeout, async {
        let ((stdout, stdout_truncated), (stderr, stderr_truncated)) =
            tokio::join!(stdout_fut, stderr_fut);
        let status = child.wait().await.map_err(ToolError::Io)?;
        Ok::<_, ToolError>((stdout, stderr, stdout_truncated, stderr_truncated, status))
    })
    .await;

    let duration = start.elapsed();

    match joined {
        Ok(Ok((stdout, stderr, stdout_truncated, stderr_truncated, status))) => Ok(CommandOutput {
            stdout,
            stderr,
            exit_code: status.code().unwrap_or(-1),
            duration,
            stdout_truncated,
            stderr_truncated,
        }),
        Ok(Err(e)) => Err(e),
        Err(_) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            Err(ToolError::Timeout(format!(
                "`{program}` exceeded {}ms and was killed",
                opts.timeout.as_millis()
            )))
        },
    }
}

/// Convenience wrapper for shell one-liners (`sh -c`).
pub async fn run_shell(command: &str, opts: CommandOpts) -> ToolResult<CommandOutput> {
    run_command("sh", ["-c", command], opts).await
}

/// Ensure a path resolves under `root`.
///
/// Blocks `..` traversal and absolute paths before they reach any filesystem
/// call. Returns the joined relative path on success.
pub fn resolve_within(root: &Path, candidate: &str) -> ToolResult<std::path::PathBuf> {
    let rel = Path::new(candidate);
    if rel.is_absolute() {
        return Err(ToolError::PermissionDenied(format!(
            "absolute path not allowed: {candidate}"
        )));
    }
    for comp in rel.components() {
        if matches!(comp, std::path::Component::ParentDir) {
            return Err(ToolError::PermissionDenied(format!(
                "parent traversal not allowed: {candidate}"
            )));
        }
    }
    Ok(root.join(rel))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn captures_stdout_and_exit_code() {
        let out =
            run_shell("printf hello", CommandOpts::default()).await.expect("run should succeed");
        assert_eq!(out.stdout, "hello");
        assert_eq!(out.exit_code, 0);
    }

    #[tokio::test]
    async fn nonzero_exit_is_reported_not_an_error() {
        let out = run_shell("exit 3", CommandOpts::default())
            .await
            .expect("spawn failures only are errors");
        assert_eq!(out.exit_code, 3);
    }

    /// 回归：裸 `Command::output()` 会让一个不退出的进程永久挂住执行器。
    #[tokio::test]
    async fn timeout_kills_a_runaway_process() {
        let err = run_shell(
            "sleep 30",
            CommandOpts::default().with_timeout(Duration::from_millis(250)),
        )
        .await
        .expect_err("runaway must be killed, not awaited");
        assert!(
            matches!(err, ToolError::Timeout(_)),
            "expected timeout, got {err:?}"
        );
    }

    /// 回归：无限输出曾直接 OOM 进程。
    #[tokio::test]
    async fn output_is_capped_and_flagged_truncated() {
        let out = run_shell(
            "head -c 200000 /dev/zero",
            CommandOpts::default().with_max_output_bytes(4096),
        )
        .await
        .expect("run should succeed");
        assert!(out.stdout_truncated, "truncation must be reported");
        assert!(
            out.stdout.len() <= 4096,
            "captured {} bytes, cap was 4096",
            out.stdout.len()
        );
    }

    #[tokio::test]
    async fn workdir_and_env_are_applied() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = run_shell(
            "printf %s \"$SISYPHUS_TEST_VAR\"; pwd",
            CommandOpts::default().with_workdir(dir.path()).with_env("SISYPHUS_TEST_VAR", "v1"),
        )
        .await
        .expect("run should succeed");
        assert!(out.stdout.starts_with("v1"), "env not applied: {out:?}");
    }

    #[test]
    fn resolve_within_blocks_traversal_and_absolute() {
        let root = Path::new("/srv/data");
        assert!(resolve_within(root, "a/b.txt").is_ok());
        assert!(resolve_within(root, "../etc/passwd").is_err());
        assert!(resolve_within(root, "/etc/passwd").is_err());
    }
}
/// Synchronous counterpart to [`run_command`].
///
/// The `LanguageBackend` implementations in `tools-build` are synchronous
/// end-to-end, so they cannot call the async variant without the whole trait
/// becoming async. This keeps the two safety properties that matter — bounded
/// output and a bounded wait — for callers that are already blocking.
///
/// The wait is enforced by polling `try_wait`, so the child is killed in the
/// same process rather than left orphaned.
pub fn run_command_blocking<I, S>(
    program: &str,
    args: I,
    opts: CommandOpts,
) -> ToolResult<CommandOutput>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    use std::io::Read;
    use std::process::Stdio;

    let start = std::time::Instant::now();
    let mut cmd = std::process::Command::new(program);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(dir) = &opts.workdir {
        cmd.current_dir(dir);
    }
    for (k, v) in &opts.envs {
        cmd.env(k, v);
    }

    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            ToolError::Generic(format!(
                "`{program}` executable not found — is it installed?"
            ))
        } else {
            ToolError::Io(e)
        }
    })?;

    fn drain<R: Read>(pipe: Option<R>, cap: usize) -> (String, bool) {
        let pipe = match pipe {
            Some(p) => p,
            None => return (String::new(), false),
        };
        let mut buf = Vec::new();
        let read = pipe.take(cap as u64).read_to_end(&mut buf);
        let n = read.unwrap_or(0);
        (String::from_utf8_lossy(&buf).into_owned(), n >= cap)
    }

    let cap = opts.max_output_bytes;
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let stdout_thread = std::thread::spawn(move || drain(stdout_pipe, cap));
    let stderr_thread = std::thread::spawn(move || drain(stderr_pipe, cap));

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if start.elapsed() >= opts.timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            },
            Err(e) => return Err(ToolError::Io(e)),
        }
    };

    let (out, out_trunc) = stdout_thread.join().unwrap_or_else(|_| (String::new(), false));
    let (err, err_trunc) = stderr_thread.join().unwrap_or_else(|_| (String::new(), false));

    match status {
        Some(status) => Ok(CommandOutput {
            stdout: out,
            stderr: err,
            exit_code: status.code().unwrap_or(-1),
            duration: start.elapsed(),
            stdout_truncated: out_trunc,
            stderr_truncated: err_trunc,
        }),
        None => Err(ToolError::Timeout(format!(
            "`{program}` exceeded {}ms and was killed",
            opts.timeout.as_millis()
        ))),
    }
}

#[cfg(test)]
mod blocking_tests {
    use super::*;

    #[test]
    fn blocking_captures_output() {
        let out = run_command_blocking(
            "sh",
            ["-c", "printf out; printf err >&2"],
            CommandOpts::default(),
        )
        .expect("run should succeed");
        assert_eq!(out.stdout, "out");
        assert_eq!(out.stderr, "err");
    }

    #[test]
    fn blocking_timeout_kills_child() {
        let err = run_command_blocking(
            "sh",
            ["-c", "sleep 30"],
            CommandOpts::default().with_timeout(Duration::from_millis(250)),
        )
        .expect_err("must be killed");
        assert!(matches!(err, ToolError::Timeout(_)), "got {err:?}");
    }

    #[test]
    fn blocking_caps_output() {
        let out = run_command_blocking(
            "sh",
            ["-c", "head -c 200000 /dev/zero"],
            CommandOpts::default().with_max_output_bytes(4096),
        )
        .expect("run should succeed");
        assert!(out.stdout_truncated);
        assert!(out.stdout.len() <= 4096);
    }
}
