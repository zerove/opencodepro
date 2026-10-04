//! `RustBackend::test()` 真实路径的端到端回归。
//!
//! 与 `rust_tests.rs` 里的解析器单测不同，这里**真的调用 `cargo test`**
//! 并经由 `RustBackend::test()` 产出 `TestResults`，因此能覆盖单测覆盖不到
//! 的两点：
//!
//! 1. `RustBackend::test()` **不带 `--no-fail-fast`**，cargo 遇首个失败即
//!    停止，后续目标（集成测试、文档测试）**根本没运行**，输出里没有它们
//!    的 `test result:` 行。解析必须如实反映这一点，而不是凭空补数。
//! 2. `ignored` / `measured` / `filtered_out` 字段在真实路径下是否仍被正确
//!    统计——修复 `passed` 时极易连带破坏这三个字段。
//!
//! 每个探针 crate 都在测试内用 `TempDir` 现造，不依赖任何外部目录或环境变量。

use sisyphus_tools_build::build::rust::RustBackend;
use sisyphus_tools_build::build::{LanguageBackend, TestResults};
use std::fs;
use std::path::Path;

/// 写一个可编译运行的探针 crate，`lib` 为其 `src/lib.rs` 内容。
fn probe(dir: &Path, name: &str, lib: &str) {
    fs::write(
        dir.join("Cargo.toml"),
        format!(
            "[workspace]\n\n[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"
        ),
    )
    .unwrap();
    let src = dir.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("lib.rs"), lib).unwrap();
}

fn run(dir: &Path) -> TestResults {
    RustBackend.test(None, None, false, Some(dir)).unwrap()
}

const TWO_DOC_OK: &str = r#"
#[test]
fn pass_a() { assert_eq!(2 + 2, 4); }

#[test]
fn pass_b() { assert!(true); }

#[test]
#[ignore = "演示 ignored"]
fn ignored_one() { assert!(true); }

/// ```
/// assert_eq!(1 + 1, 2);
/// ```
/// ```
/// assert_eq!(2 + 2, 4);
/// ```
pub fn documented() {}
"#;

/// 单元测试 2 过 1 ignored，另有 **2 个可运行的文档示例**。
///
/// 关键判据：`passed` 不得把文档测试计入。实测缺陷版本在此返回 4
/// （2 单元 + 2 文档测试），修复后为 2。
#[test]
fn doc_tests_excluded_from_passed() {
    let dir = tempfile::tempdir().unwrap();
    probe(dir.path(), "docok", TWO_DOC_OK);
    let r = run(dir.path());
    println!(
        "passed={} failed={} ignored={} measured={} filtered={} failures={:?}",
        r.passed,
        r.failed,
        r.ignored,
        r.measured,
        r.filtered_out,
        r.failures.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
    assert_eq!(r.passed, 2, "仅 2 个单元测试通过；2 个文档测试不得累加");
    assert_eq!(r.failed, 0);
    assert_eq!(r.ignored, 1, "ignored 必须如实计入");
    assert_eq!(r.measured, 0);
    assert_eq!(r.filtered_out, 0);
    assert!(
        r.failures.is_empty(),
        "全通过场景不应有失败条目，实际: {:?}",
        r.failures.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
}

/// 单元测试 2 过 1 ignored，另有 1 个通过 + 1 个失败的文档示例。
///
/// 判据：文档测试的**失败**同样不得污染单元测试口径——`failed` 为 0，
/// `failures` 不含 `bad_doc`。
#[test]
fn doc_test_failure_does_not_pollute_unit_results() {
    let dir = tempfile::tempdir().unwrap();
    probe(
        dir.path(),
        "docfail2",
        r#"
#[test]
fn pass_a() { assert_eq!(2 + 2, 4); }

#[test]
fn pass_b() { assert!(true); }

/// ```
/// assert_eq!(1 + 1, 2);
/// ```
pub fn good_doc() {}

/// ```
/// assert_eq!(1 + 1, 3);
/// ```
pub fn bad_doc() {}
"#,
    );
    let r = run(dir.path());
    println!(
        "passed={} failed={} failures={:?}",
        r.passed,
        r.failed,
        r.failures.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
    assert_eq!(r.passed, 2, "仅 2 个单元测试通过");
    assert_eq!(r.failed, 0, "文档测试的 failed 不得计入");
    assert!(
        r.failures.is_empty(),
        "文档测试条目不得进入 failures，实际: {:?}",
        r.failures.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
}

/// 单元测试 2 过 1 败 1 ignored，另有集成测试与文档测试。
///
/// 判据：`RustBackend::test()` 不带 `--no-fail-fast`，cargo 遇首个失败即
/// 停止，集成测试与文档测试**根本没运行**。`passed` 因此只含单元测试里
/// 通过的 2 个。这一点曾被写成 3（误以为集成测试也会跑），由真实路径证伪。
#[test]
fn unit_failure_stops_at_first_target() {
    let dir = tempfile::tempdir().unwrap();
    probe(
        dir.path(),
        "richfail",
        r#"
#[test]
fn pass_a() { assert_eq!(2 + 2, 4); }

#[test]
fn pass_b() { assert!(true); }

#[test]
#[ignore = "演示 ignored"]
fn ignored_one() { assert!(true); }

#[test]
fn failing() { assert_eq!(1 + 1, 3, "intentional"); }

/// ```
/// assert_eq!(1 + 1, 2);
/// ```
pub fn documented() {}
"#,
    );
    // 集成测试目标：因 fail-fast 不会被执行，但它的存在使探针更接近真实项目。
    let it = dir.path().join("tests");
    fs::create_dir_all(&it).unwrap();
    fs::write(
        it.join("it.rs"),
        "#[test]\nfn it_passes() { assert!(true); }\n",
    )
    .unwrap();

    let r = run(dir.path());
    println!(
        "passed={} failed={} ignored={} failures={:?}",
        r.passed,
        r.failed,
        r.ignored,
        r.failures.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
    assert_eq!(r.passed, 2, "集成与文档测试因 fail-fast 未运行");
    assert_eq!(r.failed, 1);
    assert_eq!(r.ignored, 1, "ignored 必须如实计入");
    assert_eq!(r.failures.len(), 1, "failures 恰为 [failing]");
    assert_eq!(r.failures[0].name, "failing");
}

/// 真实编译失败。
///
/// 实测结论（此前从未在真实路径上验证过，只有一条基于**合成输入**的
/// 单测）：编译失败时 cargo 的 **stdout 为 0 字节**，根本不含任何
/// `test result:` 行；stderr 里只有 rustc 诊断与 `could not compile`。
///
/// 因此 `passed` 与 `failed` **都是 0**，`failures` 为空。这如实反映
/// 「没有任何测试被执行」，而不是凭空造出一个失败计数。
///
/// 判据重点：`raw_stderr` 必须完整保留诊断信息——调用方据此才能判断
/// 真实原因是编译失败而非测试失败。
#[test]
fn real_compile_error_yields_all_zero_but_keeps_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    probe(
        dir.path(),
        "cerr",
        r#"
#[test]
fn pass_a() { assert_eq!(2 + 2, 4); }

#[test]
fn broken() { let y = x; }
"#,
    );
    let r = run(dir.path());
    println!(
        "passed={} failed={} failures={:?}\nraw_stdout_len={}\nraw_stderr:\n{}",
        r.passed,
        r.failed,
        r.failures.iter().map(|f| &f.name).collect::<Vec<_>>(),
        r.raw_stdout.len(),
        r.raw_stderr
    );
    assert_eq!(r.passed, 0, "编译失败时没有任何测试被执行");
    assert_eq!(
        r.failed, 0,
        "真实编译失败输出不含 test result 行，failed 应为 0"
    );
    assert!(r.failures.is_empty());
    assert_eq!(r.raw_stdout.len(), 0, "实测 stdout 为 0 字节");
    // 诊断必须完整保留，否则调用方无从判断真实原因。
    assert!(
        r.raw_stderr.contains("E0425") && r.raw_stderr.contains("could not compile"),
        "raw_stderr 必须保留完整编译诊断，实际:\n{}",
        r.raw_stderr
    );
}
