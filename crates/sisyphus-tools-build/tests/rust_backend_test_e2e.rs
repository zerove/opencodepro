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
//!
//! ## 为什么必须串行
//!
//! 本文件每条用例都会拉起一个**完整的嵌套 `cargo test`**（真跑 rustc +
//! 链接器）。`cargo_raw()` 已给子进程注入 `CARGO_BUILD_JOBS=1`，但那只限制
//! 了**单个** cargo 内部的并行度，**限制不了同时有几个 cargo 在跑**——
//! 而本文件的 7 条用例默认由 libtest **并行**调度，于是会有多个链接器同时
//! 争抢提交内存。在 `vm.overcommit_memory=2` 的严格模式（`CommitLimit ≈
//! RAM × 80%`）下实测约 1/3 概率撞墙：
//!
//! ```text
//! LLVM ERROR: out of memory
//! collect2: fatal error: ld terminated with signal 6 [Aborted]
//! ```
//!
//! 这正是「实测把 `cargo_raw` 修好之后仍然复现」的原因——**守卫的层级错了**：
//! 它设在单个 cargo 内部，而瓶颈在 cargo 之间。
//!
//! 故此处用一个**跨用例的全局闸门**把并发嵌套 cargo 限为 1。
//!
//! ## 实测证据
//!
//! 同一条压测命令（`--test-threads=8` 刻意制造并行），对照两种代码：
//!
//! | 版本 | 轮次 | 失败 | 失败时证据 |
//! |------|-----:|-----:|-----------|
//! | 修复前 | 10 | 1 | `LLVM ERROR: out of memory` + `signal 6 [Aborted]` |
//! | 修复后 | 20 | 0 | —（含上表同一压测脚本） |
//!
//! 对照组的失败特征与 `make test-all` 下的失败**完全一致**，可确认同一根因。
//!
//! **但不要把「20/20」读成「概率已归零」**：若真实失败率仍是 10%，
//! 连过 20 次的概率约 12%。所以此处只声明「把并发嵌套 cargo 限为 1 后
//! 未再复现」，不声明已根治；根因仍是全机性的提交内存紧张（见
//! `cargo_raw` 注释），本闸门只消除了**本测试文件**内的并发放大。
//!
//! （同一现象此前曾以「并行时 3 条失败、`--test-threads=1` 后 7 条全过」
//! 记录在 `cargo_raw` 的注释里——那条观察是对的，但**归因归错了**：
//! `--test-threads=1` 起作用是因为它限制了 cargo 的**个数**，
//! 而不是因为 `cargo_raw` 里的 `CARGO_BUILD_JOBS`。）

use sisyphus_tools_build::build::rust::RustBackend;
use sisyphus_tools_build::build::{LanguageBackend, TestResults};
use std::fs;
use std::path::Path;
use std::sync::{Mutex, MutexGuard, OnceLock};

/// 闸门：整个进程内**同时只允许一个**嵌套 `cargo` 在跑。
///
/// `std::sync::Mutex` 被 poison 过一次（某个用例 panic）后仍必须能被后续用例
/// 使用，否则一个失败会把后续全部用例二次毒化、把计数彻底污染。故用
/// `unwrap_or_else(|e| e.into_inner())` 显式恢复。
fn gate() -> MutexGuard<'static, ()> {
    static GATE: OnceLock<Mutex<()>> = OnceLock::new();
    GATE.get_or_init(|| Mutex::new(())).lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 取得闸门并跑闭包，把并发嵌套 cargo 限为 1。
fn serialized<T>(f: impl FnOnce() -> T) -> T {
    let _guard = gate();
    f()
}

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

/// 跑一次真实 `cargo test`；全程持闸门，杜绝多个链接器同时抢提交内存。
fn run(dir: &Path) -> TestResults {
    serialized(|| RustBackend.test(None, None, false, Some(dir)).unwrap())
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

/// 真实编译失败：应返回 Err，而非伪装成「0 个测试全部通过」。
///
/// 实测结论（此前从未在真实路径上验证过，只有一条基于**合成输入**的
/// 单测）：编译失败时 cargo 的 **stdout 为 0 字节**，根本不含任何
/// `test result:` 行；stderr 里只有 rustc 诊断与 `could not compile`。
///
/// `parse_test_results` 对此如实给出 passed=0 / failed=0。但仅凭这两个
/// 计数无法区分「编译失败」与「0 个测试通过」——CLI 的
/// `if r.failed == 0 { "ok" }` 会把编译失败显示成 `Test result: ok. 0 passed;
/// 0 failed`。故 `RustBackend::test()` 现在对「退出码非零且无任何
/// `test result:` 行」直接返回 Err。
#[test]
fn real_compile_error_returns_err_instead_of_fake_success() {
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
    let err = serialized(|| {
        RustBackend
            .test(None, None, false, Some(dir.path()))
            .expect_err("编译失败必须返回 Err，不能伪装成 0 passed / 0 failed")
    });
    let msg = err.to_string();
    println!("err = {msg}");
    assert!(
        msg.contains("未能执行任何测试"),
        "错误信息应说明未执行任何测试，实际: {msg}"
    );
    // 原始诊断必须透出，调用方才能判断真实原因。
    assert!(
        msg.contains("could not compile"),
        "错误信息必须含 cargo 原始诊断，实际: {msg}"
    );
}

/// 名称过滤无匹配：CLI 的 `-n` 参数常用场景，退出码 0 且有
/// `test result:` 行，**绝不能**被上面的编译失败守卫误伤。
///
/// 实测：stdout 含两段 `test result: ok. 0 passed; 0 failed; ...; 2 filtered out;`。
#[test]
fn real_filter_with_no_match_is_not_treated_as_compile_error() {
    let dir = tempfile::tempdir().unwrap();
    probe(
        dir.path(),
        "filt",
        r#"
#[test]
fn alpha() { assert!(true); }

#[test]
fn beta() { assert!(true); }
"#,
    );
    let r = serialized(|| {
        RustBackend
            .test(None, Some("nonexistent_test_name"), false, Some(dir.path()))
            .expect("过滤无匹配是正常退出（码 0），必须解析成功而非返回 Err")
    });
    assert_eq!(r.failed, 0);
    assert_eq!(
        r.filtered_out, 2,
        "被过滤掉的 2 个测试必须如实计入 filtered_out"
    );
}

/// `--package` 指定的包不存在：同样是非零退出码 + stdout 0 字节。
///
/// 守卫应返回 Err（确实没执行任何测试），但诊断须透出真实原因
/// `did not match any packages`，而不是含糊的「编译失败」。
#[test]
fn real_missing_package_returns_err_with_real_reason() {
    let dir = tempfile::tempdir().unwrap();
    probe(
        dir.path(),
        "nosuch",
        r#"
#[test]
fn only_one() { assert!(true); }
"#,
    );
    let err = serialized(|| {
        RustBackend
            .test(Some("nosuchpkg"), None, false, Some(dir.path()))
            .expect_err("包不存在时不得返回 Ok(0 passed)")
    });
    let msg = err.to_string();
    assert!(
        msg.contains("did not match any packages"),
        "错误信息必须透出真实原因，实际: {msg}"
    );
}

/// **零测试目标**（只有 `[[bin]]`、无 lib/集成测试）：退出码 0，
/// stdout 仍含一段 `test result: ok. 0 passed; 0 failed; 0 ignored;
/// 0 measured; 0 filtered out;`。
///
/// 这是「项目里一个测试都没有」的合法工程，守卫绝不能把它误判成编译失败。
/// （注：本条一度被我写成「无 `test result:` 行」，那是 `head -3` 截断输出
/// 造成的误读，已按完整 stdout 更正。）
#[test]
fn real_zero_test_target_is_not_treated_as_compile_error() {
    let dir = tempfile::tempdir().unwrap();
    // 只声明 bin，且不写任何 #[test] —— 显式排除 lib 测试目标。
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[workspace]\n\n[package]\nname = \"notarget\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[[bin]]\nname = \"notarget\"\npath = \"src/main.rs\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("src/main.rs"),
        "fn main() { println!(\"hi\"); }\n",
    )
    .unwrap();

    let r =
        serialized(|| RustBackend.test(None, None, false, Some(dir.path()))).unwrap_or_else(|e| {
            panic!(
                "零测试目标是合法工程（cargo 退出码 0），不得报编译失败: {}",
                e.to_string().chars().take(160).collect::<String>()
            )
        });
    assert_eq!(r.passed, 0);
    assert_eq!(r.failed, 0);
}
