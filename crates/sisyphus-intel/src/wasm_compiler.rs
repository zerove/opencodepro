//! 用于自主工具生成的 WASM 编译器。
//!
//! 早期实现只写了一个 `b"MOCK WASM BINARY"` 占位文件就返回成功，导致
//! 自编程循环里的「编译 → 验证 → 部署」整条链路都是假的：产物不可加载，
//! 验证器却报告通过。这里改成**真的调用 rustc**：
//!
//! 1. 把 LLM 生成的代码写进临时目录；
//! 2. 追加 ABI 适配层（见 [`ABI_WRAPPER`]），把 `run` 包装成沙箱要求的
//!    `execute(i32) -> i32`；
//! 3. `rustc --target wasm32-unknown-unknown --crate-type cdylib` 编译；
//! 4. 用 [`crate::wasm_validate`] 静态校验导出表；
//! 5. 通过后写入 `tools_dir/<tool_id>.wasm`（与沙箱的查找路径一致）。
//!
//! 任何一步失败都返回错误，不写产物、不假装成功。

use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::{wasm_validate, IntelError};

/// 默认工具目录，与 `sandbox.wasm_tools_dir` 的默认值保持一致。
pub const DEFAULT_TOOLS_DIR: &str = "tools";
/// 覆盖工具目录的环境变量（测试与部署用）。
pub const TOOLS_DIR_ENV: &str = "SISYPHUS_WASM_TOOLS_DIR";
/// 编译目标。
pub const WASM_TARGET: &str = "wasm32-unknown-unknown";
/// guest 影子栈大小。
///
/// wasm-ld 默认给 1 MiB，会让模块声明 17 页初始内存，超过
/// `sandbox.max_memory_pages` 的默认 10 页（640 KiB），连实例化都过不去。
pub const WASM_STACK_BYTES: u32 = 262_144;

/// 追加到生成代码末尾的 ABI 适配层。
///
/// 生成代码只需提供 `pub fn run(input: &str) -> Result<String, String>`；
/// 宿主契约（NUL 结尾的输入/输出指针、`memory` 导出）由这一层负责。
const ABI_WRAPPER: &str = r#"
// ─────────────────────────────────────────────────────────────────────
// Sisyphus WASM ABI 适配层 —— 由编译器自动追加，不属于生成代码。
// 宿主契约：execute(i32) -> i32。入参是指向 NUL 结尾输入的指针，
// 返回值是指向 NUL 结尾输出的指针（缓冲区常驻静态区，调用间复用）。
// ─────────────────────────────────────────────────────────────────────
static __SISYPHUS_OUT: ::std::sync::Mutex<::std::vec::Vec<u8>> =
    ::std::sync::Mutex::new(::std::vec::Vec::new());

fn __sisyphus_escape(text: &str) -> ::std::string::String {
    let mut out = ::std::string::String::with_capacity(text.len() + 2);
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&::std::format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[no_mangle]
pub extern "C" fn execute(ptr: i32) -> i32 {
    const MAX_INPUT: usize = 8 * 1024 * 1024;
    let start = ptr as usize;
    let mut len = 0usize;
    // SAFETY: 宿主在调用前把输入写进线性内存的 [ptr, ...) 并以 NUL 结尾，
    // 且在本次调用返回前不会移动或释放该缓冲区。上限 MAX_INPUT 保证
    // 即使宿主传入的指针没有 NUL 终止符也不会越界扫描整块内存。
    let input_bytes = unsafe {
        while *((start + len) as *const u8) != 0 {
            len += 1;
            if len >= MAX_INPUT {
                break;
            }
        }
        ::std::slice::from_raw_parts(start as *const u8, len)
    };
    let input = ::std::str::from_utf8(input_bytes).unwrap_or("");

    let payload = match run(input) {
        Ok(text) => text,
        Err(e) => ::std::format!("{{\"error\":\"{}\"}}", __sisyphus_escape(&e)),
    };

    let mut guard = match __SISYPHUS_OUT.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.clear();
    guard.extend_from_slice(payload.as_bytes());
    guard.push(0);
    guard.as_ptr() as i32
}
"#;

/// 把工具 ID 解析成产物路径，同时挡掉路径穿越。
///
/// 工具 ID 允许包含 `/`（如 `local/log_analyzer`），沙箱也按同样规则拼
/// `tools_path.join("<tool_id>.wasm")`，所以这里必须**原样保留**层级，
/// 只拒绝真正危险的部分：绝对路径、`.`/`..` 组件、反斜杠、NUL。
pub fn artifact_path(tools_dir: &Path, tool_id: &str) -> Result<PathBuf, String> {
    let trimmed = tool_id.trim();
    if trimmed.is_empty() {
        return Err("tool id is empty".into());
    }
    if trimmed.starts_with('/') || trimmed.starts_with('\\') {
        return Err(format!("tool id `{trimmed}` must be a relative path"));
    }
    for component in trimmed.split('/') {
        if component.is_empty() {
            return Err(format!("tool id `{trimmed}` has an empty path component"));
        }
        if component == "." || component == ".." {
            return Err(format!(
                "tool id `{trimmed}` contains an unsafe path component `{component}`"
            ));
        }
        if component.contains('\\') || component.contains('\0') || component.contains(':') {
            return Err(format!(
                "tool id component `{component}` contains a forbidden character (`\\`, NUL or `:`)"
            ));
        }
        if component.starts_with('.') {
            return Err(format!(
                "tool id component `{component}` must not start with a dot"
            ));
        }
    }

    Ok(tools_dir.join(format!("{trimmed}.wasm")))
}

/// 把 Rust 源码编译为 WASM 的工具。
///
/// 工具目录支持运行期改写（[`WasmCompiler::set_tools_dir`]）：bridge 用
/// `sandbox.wasm_tools_dir` 配置它，保证「编译产物落盘位置」与「沙箱加载
/// 位置」永远是同一个目录。
pub struct WasmCompiler {
    tools_dir: parking_lot::RwLock<PathBuf>,
    rustc: String,
}

impl Default for WasmCompiler {
    fn default() -> Self {
        Self::new()
    }
}

impl WasmCompiler {
    /// 用环境变量（[`TOOLS_DIR_ENV`]）或默认目录构造。
    pub fn new() -> Self {
        let tools_dir = std::env::var(TOOLS_DIR_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_TOOLS_DIR));
        Self {
            tools_dir: parking_lot::RwLock::new(tools_dir),
            rustc: "rustc".into(),
        }
    }

    /// 指定产物目录。
    pub fn with_tools_dir(dir: impl Into<PathBuf>) -> Self {
        let compiler = Self::new();
        compiler.set_tools_dir(dir);
        compiler
    }

    /// 指定 rustc 可执行文件（测试或非 PATH 环境用）。
    pub fn with_rustc(mut self, rustc: impl Into<String>) -> Self {
        self.rustc = rustc.into();
        self
    }

    /// 运行期改写产物目录。
    pub fn set_tools_dir(&self, dir: impl Into<PathBuf>) {
        *self.tools_dir.write() = dir.into();
    }

    /// 当前产物目录。
    pub fn tools_dir(&self) -> PathBuf {
        self.tools_dir.read().clone()
    }

    /// 将 Rust 源代码编译为 WASM 二进制，返回落盘路径。
    ///
    /// # Errors
    /// - [`IntelError::CompilationFailed`]：rustc 缺失、编译报错；
    /// - [`IntelError::InvalidModule`]：产物不是合法 WASM 或不满足沙箱导出契约；
    /// - [`IntelError::Internal`]：临时目录 / 文件写入失败。
    pub async fn compile(&self, tool_name: &str, code: &str) -> Result<PathBuf, IntelError> {
        let tools_dir = self.tools_dir();
        let final_path =
            artifact_path(&tools_dir, tool_name).map_err(|reason| IntelError::InvalidModule {
                tool_id: tool_name.into(),
                reason,
            })?;

        let workdir = tempfile::Builder::new()
            .prefix("sisyphus-tool-")
            .tempdir()
            .map_err(|e| IntelError::Internal(format!("failed to create temp dir: {e}")))?;

        let source_path = workdir.path().join("tool.rs");
        let wasm_path = workdir.path().join("tool.wasm");

        let mut source = String::with_capacity(code.len() + ABI_WRAPPER.len());
        source.push_str(code);
        source.push('\n');
        source.push_str(ABI_WRAPPER);
        tokio::fs::write(&source_path, source.as_bytes())
            .await
            .map_err(|e| IntelError::Internal(format!("failed to write generated source: {e}")))?;

        let output = tokio::process::Command::new(&self.rustc)
            .arg("--target")
            .arg(WASM_TARGET)
            .arg("--crate-type")
            .arg("cdylib")
            .arg("--edition")
            .arg("2021")
            .arg("-O")
            // wasm-ld 默认预留 1 MiB 影子栈，于是模块声明 17 页初始内存
            // （1.1 MiB），直接超过 `sandbox.max_memory_pages` 的默认 10 页，
            // 连实例化都过不去。收到 256 KiB 后初始内存降到 5 页。
            .arg(format!("-Clink-arg=-zstack-size={WASM_STACK_BYTES}"))
            .arg("-o")
            .arg(&wasm_path)
            .arg(&source_path)
            .stdin(Stdio::null())
            .output()
            .await
            .map_err(|e| IntelError::CompilationFailed {
                tool_id: tool_name.into(),
                reason: format!("could not run `{}`: {e}", self.rustc),
                stderr: String::new(),
            })?;

        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if !output.status.success() {
            let reason = summarize_rustc_failure(&stderr, output.status.code());
            let reason = match missing_target_hint(&stderr) {
                Some(hint) => format!("{reason} ({hint})"),
                None => reason,
            };
            return Err(IntelError::CompilationFailed {
                tool_id: tool_name.into(),
                reason,
                stderr,
            });
        }
        if !stderr.is_empty() {
            tracing::debug!(tool_id = %tool_name, %stderr, "rustc emitted warnings");
        }

        let bytes =
            tokio::fs::read(&wasm_path).await.map_err(|e| IntelError::CompilationFailed {
                tool_id: tool_name.into(),
                reason: format!("rustc reported success but no artifact was produced: {e}"),
                stderr: stderr.clone(),
            })?;

        wasm_validate::inspect_wasm(&bytes).map_err(|reason| IntelError::InvalidModule {
            tool_id: tool_name.into(),
            reason,
        })?;

        if let Some(parent) = final_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                IntelError::Internal(format!(
                    "failed to create tools directory `{}`: {e}",
                    parent.display()
                ))
            })?;
        }
        tokio::fs::write(&final_path, &bytes).await.map_err(|e| {
            IntelError::Internal(format!("failed to write `{}`: {e}", final_path.display()))
        })?;

        tracing::info!(
            tool_id = %tool_name,
            path = %final_path.display(),
            bytes = bytes.len(),
            "Compiled generated tool to wasm"
        );

        Ok(final_path)
    }
}

/// 从 rustc stderr 里提炼一句面向人的原因。
fn summarize_rustc_failure(stderr: &str, code: Option<i32>) -> String {
    let first_error = stderr
        .lines()
        .find(|line| line.trim_start().starts_with("error"))
        .map(str::trim)
        .unwrap_or("rustc failed without an `error` line");

    match code {
        Some(code) => format!("rustc exited with {code}: {first_error}"),
        None => format!("rustc was terminated by a signal: {first_error}"),
    }
}

/// 识别「编译目标未安装」，给出可执行的修复建议。
fn missing_target_hint(stderr: &str) -> Option<String> {
    let looks_missing = stderr.contains("can't find crate for `std`")
        || stderr.contains("may not be installed")
        || stderr.contains("is not installed for the toolchain");
    looks_missing.then(|| format!("run `rustup target add {WASM_TARGET}`"))
}

/// 目标是否已安装（供测试与诊断使用）。
///
/// `rustc --print target-list` 只列出**所有支持的**目标，与是否安装无关，
/// 因此必须看 sysroot 下是否存在对应 rustlib 目录。
pub async fn wasm_target_installed() -> bool {
    let output =
        match tokio::process::Command::new("rustc").arg("--print").arg("sysroot").output().await {
            Ok(o) if o.status.success() => o,
            _ => return false,
        };
    let sysroot = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if sysroot.is_empty() {
        return false;
    }
    Path::new(&sysroot).join("lib/rustlib").join(WASM_TARGET).join("lib").is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_path_keeps_namespace_but_appends_extension() {
        let p = artifact_path(Path::new("tools"), "local/log_analyzer").expect("valid id");
        assert_eq!(p, Path::new("tools/local/log_analyzer.wasm"));
    }

    #[test]
    fn artifact_path_rejects_traversal() {
        for bad in [
            "../escape",
            "a/../../b",
            "a//b",
            "",
            "  ",
            "/abs",
            "a\\b",
            ".hidden/tool",
        ] {
            assert!(
                artifact_path(Path::new("tools"), bad).is_err(),
                "`{bad}` must be rejected"
            );
        }
    }

    #[test]
    fn artifact_path_allows_dots_inside_names() {
        let p = artifact_path(Path::new("tools"), "local/v1.2_tool").expect("valid id");
        assert_eq!(p, Path::new("tools/local/v1.2_tool.wasm"));
    }

    #[test]
    fn summarize_picks_the_first_error_line() {
        let stderr = "warning: unused\n error[E0425]: cannot find value `x`\nmore";
        let summary = summarize_rustc_failure(stderr, Some(1));
        assert!(summary.contains("E0425"), "{summary}");
        assert!(summary.contains("exited with 1"), "{summary}");
    }

    #[test]
    fn summarize_handles_missing_error_line() {
        let summary = summarize_rustc_failure("", None);
        assert!(summary.contains("signal"), "{summary}");
    }

    /// 端到端：真的调 rustc 编译一个最小工具，再静态校验产物。
    ///
    /// 这是自编程链路的关键保证——它失败就说明「真实编译」这条路径坏了。
    /// 目标未安装的环境会跳过（并在 stderr 说明），因为那是环境缺失，
    /// 不是代码缺陷。
    #[tokio::test]
    async fn compiles_a_real_tool_and_validates_exports() {
        if !wasm_target_installed().await {
            eprintln!("skipping: {WASM_TARGET} is not installed (rustup target add {WASM_TARGET})");
            return;
        }

        let dir = tempfile::tempdir().expect("temp dir");
        let compiler = WasmCompiler::with_tools_dir(dir.path());

        let code = r#"
pub fn run(input: &str) -> Result<String, String> {
    Ok(format!("{{\"echo\":\"{}\"}}", input))
}
"#;
        let path = compiler.compile("local/echo_tool", code).await.expect("compile");
        assert!(path.ends_with("local/echo_tool.wasm"), "{}", path.display());

        let bytes = std::fs::read(&path).expect("artifact readable");
        let info = wasm_validate::inspect_wasm(&bytes).expect("artifact must satisfy the ABI");
        assert!(info.has_export("memory", wasm_validate::ExportKind::Memory));
        assert!(info.has_export("execute", wasm_validate::ExportKind::Func));

        // 默认 1 MiB 影子栈会让初始内存变成 17 页，超出 sandbox 默认的
        // 10 页预算——模块连实例化都过不去。编译参数必须把初始内存压下来。
        let memory = info.memory.expect("compiled module must declare memory");
        assert!(
            memory.min_pages <= 10,
            "initial memory {} pages exceeds the default sandbox budget of 10 pages",
            memory.min_pages
        );
    }

    /// 编译失败必须报错，且不留下产物。
    #[tokio::test]
    async fn compile_failure_is_reported_and_writes_nothing() {
        if !wasm_target_installed().await {
            eprintln!("skipping: {WASM_TARGET} is not installed");
            return;
        }
        let dir = tempfile::tempdir().expect("temp dir");
        let compiler = WasmCompiler::with_tools_dir(dir.path());

        let err = compiler
            .compile(
                "broken_tool",
                "pub fn run(input: &str) -> Result<String, String> { nope }",
            )
            .await
            .expect_err("broken code must not compile");
        match err {
            IntelError::CompilationFailed { ref stderr, .. } => {
                assert!(
                    !stderr.is_empty(),
                    "rustc stderr must be preserved for feedback"
                );
            },
            other => panic!("expected CompilationFailed, got {other:?}"),
        }
        assert!(!dir.path().join("broken_tool.wasm").exists());
    }

    /// 源码缺少 `run` 时必须编译失败（而不是产出空壳模块）。
    #[tokio::test]
    async fn missing_run_entry_point_is_a_compile_error() {
        if !wasm_target_installed().await {
            eprintln!("skipping: {WASM_TARGET} is not installed");
            return;
        }
        let dir = tempfile::tempdir().expect("temp dir");
        let compiler = WasmCompiler::with_tools_dir(dir.path());
        let err = compiler
            .compile("no_entry", "pub fn something_else() -> u32 { 1 }")
            .await
            .expect_err("missing `run` must fail");
        assert!(
            matches!(err, IntelError::CompilationFailed { .. }),
            "{err:?}"
        );
    }
}
