//! 编辑后自动验证（post-edit verification）。
//!
//! 在 `write` / `edit` / `apply_patch` 成功落盘后，按配置执行一次短验证命令，
//! 让"写坏了文件但命令仍报成功"变成可观测的失败信号。
//!
//! ## 配置（环境变量，不引入新的配置 schema）
//!
//! | 变量 | 说明 |
//! |------|------|
//! | `SISYPHUS_POST_EDIT_VERIFY` | 验证命令模板。含 `{files}` 时替换为改动文件列表；`auto` 表示按文件类型推断（`.rs` → `cargo check --quiet`，在最近 Cargo 包根目录执行）；未设置/空串表示关闭 |
//! | `SISYPHUS_POST_EDIT_VERIFY_TIMEOUT_SECS` | 超时秒数，默认 120；非数字或 0 时回落默认值 |
//! | `SISYPHUS_POST_EDIT_VERIFY_STRICT` | `1`/`true`/`yes`/`on` 时验证失败会让命令返回错误；默认仅告警 |
//!
//! ## 设计约束
//!
//! - 验证命令通过 `sh -c` 执行，stdout/stderr 落临时文件，避免管道缓冲区塞满导致死锁。
//! - 超时后 kill 子进程并标记 `TimedOut`，不会无限等待。
//! - 只保留输出尾部若干行，避免把整份编译日志灌进终端。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// 验证命令模板的环境变量名。
pub const ENV_VERIFY: &str = "SISYPHUS_POST_EDIT_VERIFY";
/// 验证超时（秒）的环境变量名。
pub const ENV_TIMEOUT_SECS: &str = "SISYPHUS_POST_EDIT_VERIFY_TIMEOUT_SECS";
/// 严格模式开关的环境变量名。
pub const ENV_STRICT: &str = "SISYPHUS_POST_EDIT_VERIFY_STRICT";

/// 默认验证超时秒数。
pub const DEFAULT_TIMEOUT_SECS: u64 = 120;
/// 失败时保留的输出尾部行数。
const DETAIL_TAIL_LINES: usize = 20;
/// `auto` 模板对 Rust 文件的推断命令。
const AUTO_RUST_COMMAND: &str = "cargo check --quiet";

/// 验证结果状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyStatus {
    /// 命令成功退出。
    Passed,
    /// 命令非零退出。
    Failed,
    /// 超时被终止。
    TimedOut,
    /// 未配置或无法推断出命令。
    Skipped,
}

impl VerifyStatus {
    /// 是否为失败状态（`Failed` 或 `TimedOut`）。
    pub fn is_failure(self) -> bool {
        matches!(self, Self::Failed | Self::TimedOut)
    }

    /// 状态短标签。
    pub fn label(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::TimedOut => "timed_out",
            Self::Skipped => "skipped",
        }
    }
}

/// 一次验证的结果。
#[derive(Debug, Clone)]
pub struct VerifyOutcome {
    /// 状态。
    pub status: VerifyStatus,
    /// 实际执行的命令（跳过时为 `None`）。
    pub command: Option<String>,
    /// 执行目录（跳过时为 `None`）。
    pub workdir: Option<PathBuf>,
    /// 退出码（被信号终止或跳过时为 `None`）。
    pub exit_code: Option<i32>,
    /// 耗时（毫秒）。
    pub duration_ms: u128,
    /// 跳过原因或输出尾部。
    pub detail: String,
}

impl VerifyOutcome {
    /// 构造跳过结果。
    pub fn skipped(reason: impl Into<String>) -> Self {
        Self {
            status: VerifyStatus::Skipped,
            command: None,
            workdir: None,
            exit_code: None,
            duration_ms: 0,
            detail: reason.into(),
        }
    }

    /// 单行摘要（用于终端输出）。
    pub fn summary_line(&self) -> String {
        match self.status {
            VerifyStatus::Passed => format!(
                "✓ post-edit verify passed in {}ms: {}",
                self.duration_ms,
                self.command.as_deref().unwrap_or("<none>")
            ),
            VerifyStatus::Failed => format!(
                "✗ post-edit verify failed (exit {}) in {}ms: {}",
                self.exit_code.map(|c| c.to_string()).unwrap_or_else(|| "signal".to_string()),
                self.duration_ms,
                self.command.as_deref().unwrap_or("<none>")
            ),
            VerifyStatus::TimedOut => format!(
                "✗ post-edit verify timed out after {}ms: {}",
                self.duration_ms,
                self.command.as_deref().unwrap_or("<none>")
            ),
            VerifyStatus::Skipped => {
                format!("- post-edit verify skipped: {}", self.detail.trim())
            },
        }
    }

    /// 是否应当阻断"成功"语义。
    pub fn blocks_success(&self) -> bool {
        self.status.is_failure()
    }
}

/// 验证配置。
#[derive(Debug, Clone)]
pub struct VerifyConfig {
    /// 命令模板（`auto` 或含可选 `{files}` 占位符的 shell 命令）。
    pub template: String,
    /// 超时时间。
    pub timeout: Duration,
    /// 失败是否阻断成功。
    pub strict: bool,
}

impl VerifyConfig {
    /// 直接构造（主要供测试与嵌入方使用）。
    pub fn new(template: impl Into<String>, timeout: Duration, strict: bool) -> Self {
        Self {
            template: template.into(),
            timeout,
            strict,
        }
    }

    /// 从环境变量读取配置；未设置或为空时返回 `None`。
    pub fn from_env() -> Option<Self> {
        Self::from_values(
            std::env::var(ENV_VERIFY).ok(),
            std::env::var(ENV_TIMEOUT_SECS).ok(),
            std::env::var(ENV_STRICT).ok(),
        )
    }

    /// 从显式取值构造配置（避免测试改动进程级环境变量）。
    pub fn from_values(
        template: Option<String>,
        timeout_secs: Option<String>,
        strict: Option<String>,
    ) -> Option<Self> {
        let template = template?.trim().to_string();
        if template.is_empty() {
            return None;
        }
        let timeout = timeout_secs
            .as_deref()
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .filter(|secs| *secs > 0)
            .map(Duration::from_secs)
            .unwrap_or_else(|| Duration::from_secs(DEFAULT_TIMEOUT_SECS));
        let strict = strict.as_deref().map(parse_bool).unwrap_or(false);
        Some(Self {
            template,
            timeout,
            strict,
        })
    }

    /// 解析出实际要执行的命令与工作目录；无法推断时返回 `None`。
    pub fn resolve_command(
        &self,
        files: &[PathBuf],
        default_cwd: &Path,
    ) -> Option<(String, PathBuf)> {
        if self.template.eq_ignore_ascii_case("auto") {
            let rust_file = files.iter().find(|p| is_rust_source(p))?;
            let workdir = cargo_root_for(rust_file).or_else(|| {
                if default_cwd.join("Cargo.toml").is_file() {
                    Some(default_cwd.to_path_buf())
                } else {
                    None
                }
            })?;
            return Some((AUTO_RUST_COMMAND.to_string(), workdir));
        }

        let command = if self.template.contains("{files}") {
            let joined = files
                .iter()
                .map(|p| shell_quote(&p.display().to_string()))
                .collect::<Vec<_>>()
                .join(" ");
            self.template.replace("{files}", &joined)
        } else {
            self.template.clone()
        };
        Some((command, default_cwd.to_path_buf()))
    }

    /// 按配置执行验证。
    pub fn run(&self, files: &[PathBuf], default_cwd: &Path) -> VerifyOutcome {
        match self.resolve_command(files, default_cwd) {
            Some((command, workdir)) => run_shell_command(&command, &workdir, self.timeout),
            None => VerifyOutcome::skipped(if self.template.eq_ignore_ascii_case("auto") {
                "auto 模式下未检测到 Rust 源文件或其所属 Cargo 包根目录".to_string()
            } else {
                "无法解析验证命令".to_string()
            }),
        }
    }
}

/// 读取环境变量中的严格模式开关（供嵌入方独立判断时使用）。
pub fn strict_from_env() -> bool {
    std::env::var(ENV_STRICT).map(|v| parse_bool(&v)).unwrap_or(false)
}

/// 执行一次编辑后验证。返回 `None` 表示未启用（不做任何事）。
///
/// 已经打印摘要；严格模式下失败会返回错误，调用方应据此放弃"成功"语义。
pub fn verify_after_edit(files: &[PathBuf]) -> anyhow::Result<Option<VerifyOutcome>> {
    let Some(config) = VerifyConfig::from_env() else {
        return Ok(None);
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let outcome = config.run(files, &cwd);
    report(&outcome);
    if config.strict && outcome.blocks_success() {
        anyhow::bail!(
            "post-edit 验证未通过（{}），已按 {} 拒绝报告成功",
            outcome.status.label(),
            ENV_STRICT
        );
    }
    Ok(Some(outcome))
}

/// 打印验证摘要；失败时把输出尾部写到 stderr。
pub fn report(outcome: &VerifyOutcome) {
    println!("{}", outcome.summary_line());
    if outcome.blocks_success() && !outcome.detail.trim().is_empty() {
        eprintln!("--- post-edit verify output (tail) ---");
        eprintln!("{}", outcome.detail.trim_end());
        eprintln!("-------------------------------------");
    }
}

fn parse_bool(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn is_rust_source(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
}

fn cargo_root_for(path: &Path) -> Option<PathBuf> {
    let start = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    let file_dir = if start.is_dir() {
        start
    } else {
        start.parent()?.to_path_buf()
    };
    for dir in file_dir.ancestors() {
        if dir.join("Cargo.toml").is_file() {
            return Some(dir.to_path_buf());
        }
    }
    None
}

fn shell_quote(value: &str) -> String {
    let safe = !value.is_empty()
        && value.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '/' | '.' | '_' | '-' | ':' | '@' | '+' | '=' | ',' | '~')
        });
    if safe {
        return value.to_string();
    }
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for c in value.chars() {
        if c == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(c);
        }
    }
    quoted.push('\'');
    quoted
}

fn temp_log_path() -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "sisyphus-post-edit-{}-{stamp}.log",
        std::process::id()
    ))
}

fn tail_of(path: &Path, max_lines: usize) -> String {
    let raw = fs::read(path).unwrap_or_default();
    let text = String::from_utf8_lossy(&raw);
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].join("\n")
}

fn run_shell_command(command: &str, workdir: &Path, timeout: Duration) -> VerifyOutcome {
    let started = Instant::now();
    let log_path = temp_log_path();

    let stdout_file = match fs::File::create(&log_path) {
        Ok(file) => file,
        Err(err) => {
            return VerifyOutcome {
                status: VerifyStatus::Failed,
                command: Some(command.to_string()),
                workdir: Some(workdir.to_path_buf()),
                exit_code: None,
                duration_ms: started.elapsed().as_millis(),
                detail: format!("无法创建临时日志文件 {}: {err}", log_path.display()),
            };
        },
    };
    let stderr_file = match stdout_file.try_clone() {
        Ok(file) => file,
        Err(err) => {
            let _ = fs::remove_file(&log_path);
            return VerifyOutcome {
                status: VerifyStatus::Failed,
                command: Some(command.to_string()),
                workdir: Some(workdir.to_path_buf()),
                exit_code: None,
                duration_ms: started.elapsed().as_millis(),
                detail: format!("无法复制日志句柄: {err}"),
            };
        },
    };

    let spawned = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file))
        .spawn();

    let mut child = match spawned {
        Ok(child) => child,
        Err(err) => {
            let _ = fs::remove_file(&log_path);
            return VerifyOutcome {
                status: VerifyStatus::Failed,
                command: Some(command.to_string()),
                workdir: Some(workdir.to_path_buf()),
                exit_code: None,
                duration_ms: started.elapsed().as_millis(),
                detail: format!("无法启动 sh: {err}"),
            };
        },
    };

    let deadline = Instant::now() + timeout;
    let waited = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(Duration::from_millis(10));
            },
            Err(err) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = fs::remove_file(&log_path);
                return VerifyOutcome {
                    status: VerifyStatus::Failed,
                    command: Some(command.to_string()),
                    workdir: Some(workdir.to_path_buf()),
                    exit_code: None,
                    duration_ms: started.elapsed().as_millis(),
                    detail: format!("等待验证进程失败: {err}"),
                };
            },
        }
    };

    let mut detail = tail_of(&log_path, DETAIL_TAIL_LINES);
    let _ = fs::remove_file(&log_path);
    let duration_ms = started.elapsed().as_millis();

    match waited {
        Some(status) => {
            let status_kind = if status.success() {
                VerifyStatus::Passed
            } else {
                VerifyStatus::Failed
            };
            if status_kind.is_failure() && detail.trim().is_empty() {
                detail = format!("命令以 {status} 退出，无输出");
            }
            VerifyOutcome {
                status: status_kind,
                command: Some(command.to_string()),
                workdir: Some(workdir.to_path_buf()),
                exit_code: status.code(),
                duration_ms,
                detail,
            }
        },
        None => {
            let timeout_note = format!(
                "命令在 {}ms 超时，已终止 (timeout={}s)",
                duration_ms,
                timeout.as_secs()
            );
            if detail.trim().is_empty() {
                detail = timeout_note;
            } else {
                detail.push_str(&format!("\n{timeout_note}"));
            }
            VerifyOutcome {
                status: VerifyStatus::TimedOut,
                command: Some(command.to_string()),
                workdir: Some(workdir.to_path_buf()),
                exit_code: None,
                duration_ms,
                detail,
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn from_values_disabled_when_unset_or_blank() {
        assert!(VerifyConfig::from_values(None, None, None).is_none());
        assert!(VerifyConfig::from_values(Some("   ".into()), None, None).is_none());
    }

    #[test]
    fn from_values_parses_timeout_and_strict() {
        let config = VerifyConfig::from_values(
            Some("cargo check --quiet".into()),
            Some("30".into()),
            Some("yes".into()),
        )
        .expect("config");
        assert_eq!(config.timeout, Duration::from_secs(30));
        assert!(config.strict);

        let fallback =
            VerifyConfig::from_values(Some("true".into()), Some("0".into()), Some("off".into()))
                .expect("config");
        assert_eq!(fallback.timeout, Duration::from_secs(DEFAULT_TIMEOUT_SECS));
        assert!(!fallback.strict);
    }

    #[test]
    fn resolve_literal_template_uses_default_cwd() {
        let config = VerifyConfig::new("cargo check --quiet", Duration::from_secs(5), false);
        let cwd = Path::new("/tmp");
        let (command, workdir) =
            config.resolve_command(&[PathBuf::from("/tmp/a.rs")], cwd).expect("resolved");
        assert_eq!(command, "cargo check --quiet");
        assert_eq!(workdir, cwd);
    }

    #[test]
    fn resolve_files_placeholder_quotes_spaces() {
        let config = VerifyConfig::new("rustfmt --check {files}", Duration::from_secs(5), false);
        let files = vec![PathBuf::from("/tmp/a b.rs"), PathBuf::from("/tmp/c.rs")];
        let (command, _) = config.resolve_command(&files, Path::new("/tmp")).expect("resolved");
        assert_eq!(command, "rustfmt --check '/tmp/a b.rs' /tmp/c.rs");
    }

    #[test]
    fn resolve_auto_skips_non_rust_files() {
        let config = VerifyConfig::new("auto", Duration::from_secs(5), false);
        let outcome = config.run(&[PathBuf::from("/tmp/notes.md")], Path::new("/tmp"));
        assert_eq!(outcome.status, VerifyStatus::Skipped);
        assert!(outcome.detail.contains("Rust"));
    }

    #[test]
    fn resolve_auto_uses_cargo_root() {
        let dir = TempDir::new().expect("tmpdir");
        let crate_a = dir.path().join("crate-a");
        fs::create_dir_all(crate_a.join("src")).expect("mkdir");
        fs::write(crate_a.join("Cargo.toml"), "[package]\nname = \"a\"\n").expect("write");
        let source = crate_a.join("src/lib.rs");
        fs::write(&source, "").expect("write");

        let config = VerifyConfig::new("auto", Duration::from_secs(5), false);
        let (command, workdir) = config.resolve_command(&[source], dir.path()).expect("resolved");
        assert_eq!(command, AUTO_RUST_COMMAND);
        assert_eq!(workdir, crate_a);
    }

    #[test]
    fn run_records_passed_command() {
        let dir = TempDir::new().expect("tmpdir");
        let config = VerifyConfig::new("echo verified", Duration::from_secs(10), false);
        let outcome = config.run(&[], dir.path());
        assert_eq!(outcome.status, VerifyStatus::Passed);
        assert_eq!(outcome.exit_code, Some(0));
        assert!(outcome.detail.contains("verified"));
        assert!(!outcome.blocks_success());
    }

    #[test]
    fn run_records_failure_exit_code_and_tail() {
        let dir = TempDir::new().expect("tmpdir");
        let config = VerifyConfig::new("echo boom >&2; exit 7", Duration::from_secs(10), false);
        let outcome = config.run(&[], dir.path());
        assert_eq!(outcome.status, VerifyStatus::Failed);
        assert_eq!(outcome.exit_code, Some(7));
        assert!(outcome.detail.contains("boom"));
        assert!(outcome.blocks_success());
        assert!(outcome.summary_line().contains("exit 7"));
    }

    #[test]
    fn run_enforces_timeout() {
        let dir = TempDir::new().expect("tmpdir");
        let config = VerifyConfig::new("sleep 5", Duration::from_millis(200), false);
        let outcome = config.run(&[], dir.path());
        assert_eq!(outcome.status, VerifyStatus::TimedOut);
        assert!(outcome.detail.contains("超时"));
        assert!(outcome.summary_line().contains("timed out"));
    }

    #[test]
    fn shell_quote_handles_quotes_and_empty() {
        assert_eq!(shell_quote("/tmp/a.rs"), "/tmp/a.rs");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn skipped_outcome_summary() {
        let outcome = VerifyOutcome::skipped("未启用");
        assert_eq!(outcome.summary_line(), "- post-edit verify skipped: 未启用");
        assert!(!outcome.blocks_success());
    }
}
