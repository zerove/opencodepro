use anyhow::Result;
use std::path::PathBuf;

use crate::tools::build::TestResults;

/// 从 `--lang` 选项字符串解析语言后端，并在未指定时按项目特征自动检测。
///
/// 曾以三份副本存在于 `build.rs` / `run.rs` / `diagnose.rs`（36 行逐字复制，
/// 仅差一行注释）。新增语言需同步改三处，漏改即产生行为分叉——与
/// `sisyphus-bridge/src/peer/fallback.rs` 复制 `service.rs` 的转换函数、
/// 导致跨节点经验同步从未生效，是同一类漂移风险。故收敛到此一处。
pub(crate) fn resolve_backend(
    lang: Option<&str>,
    cwd: Option<&std::path::Path>,
) -> Result<Box<dyn crate::tools::build::LanguageBackend>> {
    match lang {
        Some("rust") | None
            if cwd.map(|d| d.join("Cargo.toml").exists()).unwrap_or(false)
                || lang == Some("rust") =>
        {
            Ok(Box::new(crate::tools::build::rust::RustBackend))
        },
        Some("node") | Some("ts") | Some("js") => {
            Ok(Box::new(crate::tools::build::node::NodeBackend::new()?))
        },
        Some("python") | Some("py") => {
            Ok(Box::new(crate::tools::build::python::PythonBackend::new()?))
        },
        Some("go") | Some("golang") => Ok(Box::new(crate::tools::build::golang::GoBackend)),
        Some("java") | Some("maven") => {
            Ok(Box::new(crate::tools::build::java::JavaBackend::new_maven()))
        },
        Some("gradle") => Ok(Box::new(
            crate::tools::build::java::JavaBackend::new_gradle(),
        )),
        Some("fe") | Some("frontend") | Some("css") | Some("html") => {
            Ok(Box::new(crate::tools::build::frontend::FrontendBackend))
        },
        Some(other) => {
            anyhow::bail!("Unknown language backend: {other}. Supported: rust, node/ts/js, python/py, go/golang, java/maven, gradle, fe/frontend/css/html")
        },
        None => {
            let dir = cwd.unwrap_or(std::path::Path::new("."));
            Ok(crate::tools::build::detect_backend(dir)?)
        },
    }
}

fn cwd_or_current(cwd: Option<PathBuf>) -> Option<PathBuf> {
    Some(cwd.unwrap_or_else(|| std::env::current_dir().unwrap_or_default()))
}

fn print_diags(label: &str, diags: &[crate::tools::build::Diag]) {
    if diags.is_empty() {
        return;
    }
    println!("  {label}:");
    for d in diags {
        let loc = match (&d.file, d.line) {
            (Some(f), Some(l)) => format!("{f}:{l}"),
            (Some(f), None) => f.clone(),
            _ => String::new(),
        };
        let code = d.code.as_deref().unwrap_or("");
        if loc.is_empty() {
            println!("    [{code}] {}", d.message);
        } else {
            println!("    {loc}  [{code}] {}", d.message);
        }
    }
}

/// Run check.
pub fn run_check(
    package: Option<String>,
    all: bool,
    cwd: Option<PathBuf>,
    lang: Option<String>,
) -> Result<()> {
    let dir = cwd.as_deref().or_else(|| Some(std::path::Path::new(".")));
    let backend = resolve_backend(lang.as_deref(), dir)?;
    let dir_ref = cwd_or_current(cwd);
    let out = backend.check(package.as_deref(), all, dir_ref.as_deref())?;
    if out.success {
        println!("Check passed (no errors).");
    } else {
        println!("Check FAILED:");
    }
    print_diags("Errors", &out.errors);
    print_diags("Warnings", &out.warnings);
    if !out.success {
        anyhow::bail!(
            "{} error(s), {} warning(s)",
            out.errors.len(),
            out.warnings.len()
        );
    }
    Ok(())
}

/// Run build.
pub fn run_build(
    package: Option<String>,
    release: bool,
    cwd: Option<PathBuf>,
    lang: Option<String>,
) -> Result<()> {
    let dir = cwd.as_deref().or_else(|| Some(std::path::Path::new(".")));
    let backend = resolve_backend(lang.as_deref(), dir)?;
    let dir_ref = cwd_or_current(cwd);
    let out = backend.build(package.as_deref(), release, dir_ref.as_deref())?;
    if out.success {
        println!("Build succeeded.");
    } else {
        println!("Build FAILED:");
    }
    print_diags("Errors", &out.errors);
    print_diags("Warnings", &out.warnings);
    if !out.success {
        anyhow::bail!(
            "{} error(s), {} warning(s)",
            out.errors.len(),
            out.warnings.len()
        );
    }
    Ok(())
}

/// 状态标签：有用例失败。
pub const STATUS_FAILED: &str = "FAILED";
/// 状态标签：有用例通过。
pub const STATUS_OK: &str = "ok";
/// 状态标签：跑完了，但零用例。
pub const STATUS_NO_TESTS: &str = "no tests ran";

/// 根据测试结果判定展示用状态标签。
///
/// 判定**不能只看 `failed == 0`**。零失败有三种截然不同的情形：
///   - 有用例通过      → `ok`
///   - 跑完了但零用例  → 不能叫 `ok`（空工程、没有测试类、被 `-Dtest` 过滤掉）
///   - 根本没跑起来    → 各后端守卫已在 `test()` 层返回 `Err`，走不到这里
///
/// 历史上这里写的是 `if r.failed == 0 { "ok" }`，于是空工程会打印出
/// `Test result: ok. 0 passed`——正是多个后端解析缺陷的共同表象。
///
/// 「零用例」本身不算失败（`frontend` 后端就没有测试步骤，空 rust/go/python 工程
/// 也合法），所以只改**展示**、不改退出码；但绝不能再谎报为 `ok`。
fn test_status_label(r: &TestResults) -> &'static str {
    if r.failed > 0 {
        return STATUS_FAILED;
    }
    if r.passed == 0 && r.ignored == 0 && r.measured == 0 {
        return STATUS_NO_TESTS;
    }
    STATUS_OK
}

/// Run test.
pub fn run_test(
    package: Option<String>,
    name: Option<String>,
    all: bool,
    cwd: Option<PathBuf>,
    lang: Option<String>,
) -> Result<()> {
    let dir = cwd.as_deref().or_else(|| Some(std::path::Path::new(".")));
    let backend = resolve_backend(lang.as_deref(), dir)?;
    let dir_ref = cwd_or_current(cwd);
    let r = backend.test(package.as_deref(), name.as_deref(), all, dir_ref.as_deref())?;

    // 状态不能只看 `failed == 0`，见 `test_status_label` 的文档。
    let status = test_status_label(&r);
    println!(
        "Test result: {status}. {} passed; {} failed; {} ignored; {} measured; {} filtered out",
        r.passed, r.failed, r.ignored, r.measured, r.filtered_out
    );

    if status == STATUS_NO_TESTS {
        println!(
            "提示：零用例。可能是空工程、没有测试文件，或过滤器（如 -k/-Dtest）没命中任何用例。"
        );
    }

    if !r.failures.is_empty() {
        println!("\nFailures:");
        for f in &r.failures {
            println!("    {}", f.name);
        }
    }

    if r.failed > 0 {
        anyhow::bail!("{} test(s) failed", r.failed);
    }
    Ok(())
}

/// Run lint.
pub fn run_lint(package: Option<String>, cwd: Option<PathBuf>, lang: Option<String>) -> Result<()> {
    let dir = cwd.as_deref().or_else(|| Some(std::path::Path::new(".")));
    let backend = resolve_backend(lang.as_deref(), dir)?;
    let dir_ref = cwd_or_current(cwd);
    let out = backend.lint(package.as_deref(), dir_ref.as_deref())?;
    if out.success {
        println!("Lint passed (no warnings).");
    } else {
        println!("Lint issued warnings:");
    }
    print_diags("Warnings", &out.warnings);
    print_diags("Errors", &out.errors);
    Ok(())
}

/// Run fmt.
pub fn run_fmt(check_mode: bool, cwd: Option<PathBuf>, lang: Option<String>) -> Result<()> {
    let dir = cwd.as_deref().or_else(|| Some(std::path::Path::new(".")));
    let backend = resolve_backend(lang.as_deref(), dir)?;
    let dir_ref = cwd_or_current(cwd);
    if check_mode {
        let ok = backend.fmt_check(dir_ref.as_deref())?;
        if ok {
            println!("All files formatted correctly.");
        } else {
            anyhow::bail!("Some files are not formatted — run `sisyphus build fmt` to fix.");
        }
    } else {
        backend.fmt_write(dir_ref.as_deref())?;
        println!("Formatted all files.");
    }
    Ok(())
}

/// Run doc.
pub fn run_doc(package: Option<String>, cwd: Option<PathBuf>, lang: Option<String>) -> Result<()> {
    let dir = cwd.as_deref().or_else(|| Some(std::path::Path::new(".")));
    let backend = resolve_backend(lang.as_deref(), dir)?;
    let dir_ref = cwd_or_current(cwd);
    let out = backend.doc(package.as_deref(), dir_ref.as_deref())?;
    println!("{out}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::build::TestFailure;

    fn res(passed: usize, failed: usize, ignored: usize, measured: usize) -> TestResults {
        TestResults {
            passed,
            failed,
            ignored,
            measured,
            filtered_out: 0,
            failures: Vec::<TestFailure>::new(),
            raw_stdout: String::new(),
            raw_stderr: String::new(),
        }
    }

    #[test]
    fn any_failure_is_failed() {
        assert_eq!(test_status_label(&res(5, 1, 0, 0)), STATUS_FAILED);
        assert_eq!(test_status_label(&res(0, 1, 0, 0)), STATUS_FAILED);
    }

    #[test]
    fn passing_run_is_ok() {
        assert_eq!(test_status_label(&res(2, 0, 0, 0)), STATUS_OK);
    }

    /// 核心回归：旧逻辑 `if failed == 0 { "ok" }` 会把零用例谎报为 ok，
    /// 这正是空工程、过滤器全滤掉、以及多个后端解析缺陷的共同表象。
    #[test]
    fn zero_tests_is_not_ok() {
        assert_eq!(test_status_label(&res(0, 0, 0, 0)), STATUS_NO_TESTS);
    }

    /// 全部被 skip 仍是 `ok`：跳过是测试作者的**显式意图**，cargo 与 pytest
    /// 同样报 `test result: ok. 0 passed; 0 failed; 3 ignored`。把它降级成
    /// `no tests ran` 反而会在 CI 上制造噪声信号。
    #[test]
    fn all_skipped_is_ok() {
        assert_eq!(test_status_label(&res(0, 0, 3, 0)), STATUS_OK);
    }

    #[test]
    fn measured_only_is_ok() {
        assert_eq!(test_status_label(&res(0, 0, 0, 2)), STATUS_OK);
    }

    #[test]
    fn skipped_with_passes_is_ok() {
        assert_eq!(test_status_label(&res(1, 0, 3, 0)), STATUS_OK);
    }
}
