use super::*;
use std::fs;
use tempfile::TempDir;

fn write_toml(dir: &Path, content: &str) {
    // Ensure standalone project: append empty [workspace] so parent workspace
    // (e.g. .jcode/Cargo.toml) doesn't interfere with cargo commands.
    let toml = if content.contains("[workspace]") {
        content.to_string()
    } else {
        format!("{content}\n[workspace]\n")
    };
    fs::write(dir.join("Cargo.toml"), toml).unwrap();
}

fn write_src(dir: &Path, content: &str) {
    let src = dir.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("lib.rs"), content).unwrap();
}

#[test]
fn test_parse_test_results_all_ok() {
    let stdout = "\n     Running unittests src/lib.rs\n\nrunning 3 tests\ntest a ... ok\ntest b ... ok\ntest c ... ok\n\ntest result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n\n";
    let r = parse_test_results(stdout, "");
    assert_eq!(r.passed, 3);
    assert_eq!(r.failed, 0);
    assert!(r.failures.is_empty());
}

#[test]
fn test_parse_test_results_with_failures() {
    let stdout = "running 2 tests\ntest a ... ok\ntest b ... FAILED\n\ntest result: FAILED. 1 passed; 1 failed; 0 ignored\n\n--- Failures ---\n\nfailures:\n    my_test::test_b\n\n";
    let r = parse_test_results(stdout, "");
    assert_eq!(r.passed, 1);
    assert_eq!(r.failed, 1);
    assert_eq!(r.failures.len(), 1);
    assert_eq!(r.failures[0].name, "my_test::test_b");
}

#[test]
fn test_parse_test_results_with_ignored() {
    let stdout = "test result: ok. 512 passed; 0 failed; 4 ignored; 0 measured; 0 filtered out\n";
    let r = parse_test_results(stdout, "");
    assert_eq!(r.passed, 512);
    assert_eq!(r.failed, 0);
    assert_eq!(r.ignored, 4);
}

#[test]
fn test_extract_diags_errors() {
    let stderr = "error[E0425]: cannot find value `x` in this scope\n  --> src/lib.rs:10:5\n   |\n10 |     let y = x;\n   |             ^ not found\n\nerror: aborting due to 1 previous error\n";
    let (errors, warnings) = extract_diags(stderr);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code.as_deref(), Some("E0425"));
    assert_eq!(errors[0].message, "cannot find value `x` in this scope");
    assert_eq!(errors[0].file.as_deref(), Some("src/lib.rs"));
    assert_eq!(errors[0].line, Some(10));
    assert_eq!(errors[0].col, Some(5));
    assert!(warnings.is_empty());
}

#[test]
fn test_extract_diags_with_ansi_colors() {
    // CI（dtolnay/rust-toolchain）设置 CARGO_TERM_COLOR=always，cargo 输出带 ANSI 色码
    let stderr = "\u{1b}[1m\u{1b}[92m    Checking\u{1b}[0m bad-crate\n\u{1b}[1m\u{1b}[91merror[E0425]\u{1b}[0m\u{1b}[1m: cannot find value `does_not_exist` in this scope\u{1b}[0m\n \u{1b}[1m\u{1b}[94m--> \u{1b}[0msrc/lib.rs:1:34\n  \u{1b}[1m\u{1b}[94m|\u{1b}[0m\n1 | pub fn hello() -> &'static str { does_not_exist }\n  \u{1b}[1m\u{1b}[91m^^^^^^^^^^^^^^\u{1b}[0m \u{1b}[1m\u{1b}[91mnot found in this scope\u{1b}[0m\n\n\u{1b}[1m\u{1b}[91merror\u{1b}[0m\u{1b}[1m:\u{1b}[0m could not compile `bad-crate` (lib) due to 1 previous error\n";
    let (errors, warnings) = extract_diags(stderr);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code.as_deref(), Some("E0425"));
    assert_eq!(
        errors[0].message,
        "cannot find value `does_not_exist` in this scope"
    );
    assert_eq!(errors[0].file.as_deref(), Some("src/lib.rs"));
    assert_eq!(errors[0].line, Some(1));
    assert_eq!(errors[0].col, Some(34));
    assert!(warnings.is_empty());
}

#[test]
fn test_extract_diags_warnings() {
    let stderr = "warning: unused variable `x`\n  --> src/lib.rs:5:9\n   |\n5 |     let x = 1;\n   |         ^ help: if you intend to use it: `_x`\n";
    let (errors, warnings) = extract_diags(stderr);
    assert!(errors.is_empty());
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].level, "warning");
    assert_eq!(warnings[0].file.as_deref(), Some("src/lib.rs"));
    assert_eq!(warnings[0].line, Some(5));
    assert_eq!(warnings[0].col, Some(9));
}

#[test]
fn test_extract_diags_multiple() {
    let stderr = "error[E0308]: mismatched types\n  --> src/main.rs:20:15\n   |\n20 |     let x: i32 = \"hello\";\n   |                  ^^^^^^^ expected i32, found &str\n\nwarning: unused import\n  --> src/main.rs:1:1\n   |\n1 | use std::collections::HashMap;\n   |\n";
    let (errors, warnings) = extract_diags(stderr);
    assert_eq!(errors.len(), 1);
    assert_eq!(warnings.len(), 1);
}

#[test]
fn test_check_on_valid_project() {
    let dir = TempDir::new().unwrap();
    let path = dir.path();
    write_toml(
        path,
        r#"[package]
name = "test-crate"
version = "0.1.0"
edition = "2021"
"#,
    );
    write_src(path, "pub fn hello() -> &'static str { \"hello\" }\n");
    let backend = RustBackend;
    let result = backend.check(None, false, Some(path)).unwrap();
    assert!(result.success);
    assert!(result.errors.is_empty());
}

#[test]
fn test_check_on_invalid_project() {
    let dir = TempDir::new().unwrap();
    let path = dir.path();
    write_toml(
        path,
        r#"[package]
name = "bad-crate"
version = "0.1.0"
edition = "2021"
"#,
    );
    write_src(path, "pub fn hello() -> &'static str { does_not_exist }\n");
    let backend = RustBackend;
    let result = backend.check(None, false, Some(path)).unwrap();
    assert!(!result.success);
    assert!(!result.errors.is_empty());
}

#[test]
fn test_fmt_check_formatted() {
    let dir = TempDir::new().unwrap();
    let path = dir.path();
    write_toml(
        path,
        r#"[package]
name = "fmt-crate"
version = "0.1.0"
edition = "2021"
"#,
    );
    write_src(path, "pub fn foo() -> u8 {\n    42\n}\n");
    let backend = RustBackend;
    let ok = backend.fmt_check(Some(path)).unwrap();
    assert!(ok);
}

#[test]
fn test_fmt_check_unformatted() {
    let dir = TempDir::new().unwrap();
    let path = dir.path();
    write_toml(
        path,
        r#"[package]
name = "bad-fmt-crate"
version = "0.1.0"
edition = "2021"
"#,
    );
    write_src(path, "pub fn foo() -> u8 { 42 }\n");
    let backend = RustBackend;
    // First check it
    backend.fmt_check(Some(path)).unwrap();
    // Then break formatting
    fs::write(path.join("src/lib.rs"), "pub fn foo() -> u8 {\n42\n}\n").unwrap();
    let ok = backend.fmt_check(Some(path)).unwrap();
    assert!(!ok);
}

#[test]
fn test_build_valid() {
    let dir = TempDir::new().unwrap();
    let path = dir.path();
    write_toml(
        path,
        r#"[package]
name = "build-crate"
version = "0.1.0"
edition = "2021"
"#,
    );
    write_src(path, "pub fn works() -> i32 { 1 + 2 }\n");
    let backend = RustBackend;
    let result = backend.build(None, false, Some(path)).unwrap();
    assert!(result.success);
}

#[test]
fn test_test_simple() {
    let dir = TempDir::new().unwrap();
    let path = dir.path();
    write_toml(
        path,
        r#"[package]
name = "test-crate"
version = "0.1.0"
edition = "2021"
"#,
    );
    let src = path.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("lib.rs"),
        r#"
#[test]
fn it_works() { assert_eq!(2 + 2, 4); }

#[test]
fn it_also_works() { assert!(true); }
"#,
    )
    .unwrap();
    let backend = RustBackend;
    let results = backend.test(None, None, false, Some(path)).unwrap();
    // 失败时必须带上 cargo 的原始输出。这些用例真实调用 `cargo test`，
    // 断言 `passed == 2` 只说明"数量不对"，不说明"为什么不对"。曾有一次
    // 整包运行失败（56 次里 1 次），却因断言信息只有 left/right 而完全
    // 无法定位——cargo 可能根本没真正执行测试（此时它输出
    // `test result: ok. 0 passed` 而非报错），也可能是编译失败或输出被截断，
    // 三种情况在断言处看起来完全一样。
    assert_eq!(results.passed, 2, "cargo stdout:\n{}", results.raw_stdout);
    assert_eq!(results.failed, 0, "cargo stderr:\n{}", results.raw_stderr);
}

#[test]
fn test_test_with_failures() {
    let dir = TempDir::new().unwrap();
    let path = dir.path();
    write_toml(
        path,
        r#"[package]
name = "fail-crate"
version = "0.1.0"
edition = "2021"
"#,
    );
    let src = path.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("lib.rs"),
        r#"
#[test]
fn passing() { assert!(true); }

#[test]
fn failing() { assert!(false, "intentional failure"); }
"#,
    )
    .unwrap();
    let backend = RustBackend;
    let results = backend.test(None, None, false, Some(path)).unwrap();
    assert_eq!(results.passed, 1);
    assert_eq!(results.failed, 1);
    assert_eq!(results.failures.len(), 1);
}

#[test]
fn test_backend_name() {
    let backend = RustBackend;
    assert_eq!(backend.name(), "rust");
}

// ── 退化输出：cargo 未真正执行测试时的真实输出 ─────────────
//
// `test_test_simple` 曾在整包运行中概率失败（约 0.4%），而调用方只看
// `results.passed`。下列用例把真实观察到的退化输出固化为测试，使「为什么
// passed 会不等于预期」这件事有据可查，而不是只能靠猜。
//
// 这些输出均取自实测，不是构造出来的理想情况：
// - `ok. 0 passed`：crate 只有 src/main.rs 而无 src/lib.rs 时，cargo 输出
//   `running 0 tests` + `ok. 0 passed`，且**退出码为 0**（不报错）。
// - 编译失败：cargo 输出 `test result: FAILED. 0 passed; 1 failed`。
#[test]
fn test_parse_test_results_zero_passed_is_not_success() {
    // cargo 在没有可测目标时仍报 ok。这是「看起来成功、实际什么都没跑」
    // 的典型形态，调用方必须靠 passed 计数而非退出码来判断。
    let stdout = "running 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n";
    let r = parse_test_results(stdout, "");
    assert_eq!(
        r.passed, 0,
        "无测试可跑时 passed 必须如实为 0，不能被当成全通过"
    );
    assert_eq!(r.failed, 0);
}

#[test]
fn test_parse_test_results_compile_failure_yields_zero_passed() {
    // 输入是**实测抓取的真实编译失败输出**（不是构造的）。
    //
    // 本测试原先断言「编译失败时 cargo 输出
    // `test result: FAILED. 0 passed; 1 failed`」——那是**合成输入**，与
    // 现实不符。实测：编译失败时 cargo 的 **stdout 为 0 字节**，根本不含
    // 任何 `test result:` 行；stderr 里只有 rustc 诊断与
    // `could not compile`。
    //
    // 因此 `failed` 同样是 0：解析器如实反映「没有任何测试被执行」，
    // 而不是凭空造出一个失败计数。调用方要判断编译是否失败，应看
    // `raw_stderr`（实测含完整 338 字节诊断）或退出码——
    // `RustBackend::test()` 目前丢弃了退出码 `_success`，这属已知缺口。
    let stdout = "";
    let stderr = "error[E0425]: cannot find value `x` in this scope\n --> src/lib.rs:5:13\n  |\n5 |     let y = x;\n  |             ^ not found\n\nerror: aborting due to 1 previous error\n\nerror: could not compile `cerr` (lib test) due to 1 previous error\n";
    let r = parse_test_results(stdout, stderr);
    assert_eq!(r.passed, 0);
    assert_eq!(
        r.failed, 0,
        "真实编译失败输出里没有 test result 行，failed 应为 0 而非 1"
    );
    assert!(r.failures.is_empty());
    // 诊断信息本身必须完整保留，供调用方判断真实原因。
    assert!(
        r.raw_stderr.contains("could not compile"),
        "raw_stderr 必须保留 cargo 的原始诊断"
    );
}

// ── 文档测试结果不得混入 passed ─────────────────────────
//
// 下面两个测试的输入均**实测抓取自真实的 `cargo test` 双流**，不是构造的
// 理想情况。关键在于流的分离：
//
// - `Running` / `Doc-tests` 分段标记由cargo 写入 **stderr**；
// - `running N tests` / `test <路径> - <条目> (line N)` / `test result:`
//   由 libtest 写入 **stdout**。
//
// `parse_test_results` 把两流拼成 `format!("{stdout}\n{stderr}")`，因此所有
// 分段标记必然排在所有 `test result:` 行之后。**基于标记的区间判断在此
// 拼接顺序下原理上不可能生效**——这一点已被实测确认（第一次尝试的修复
// 正是这样写的，编译通过、逻辑在独立复现中正确，但在真实路径上毫无作用）。
//
// 可用的判别特征只在 stdout 内部：libtest 对文档测试条目使用
// `test <路径> - <条目> (line N)` 命名。故按 stdout 的 `running N test(s)`
// 块归组，块内命中该约定则整块跳过。
#[test]
fn test_parse_test_results_excludes_doc_tests_from_passed() {
    // stdout 实测抓取：两个单元测试 + 一个文档测试，共两个 `test result:`。
    let stdout = "\n\
running 2 tests\n\
test it_also_works ... ok\n\
test it_works ... ok\n\
\n\
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n\
\n\
\n\
running 1 test\n\
test src/lib.rs - documented (line 8) ... ok\n\
\n\
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.46s\n\
";
    // stderr 实测抓取：分段标记确实在这里，且位于所有 test result 行之后。
    let stderr = "   Compiling test-crate v0.1.0 (/tmp/x)\n    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.02s\n     Running unittests src/lib.rs (target/debug/deps/test_crate-1111111111111111)\n   Doc-tests test-crate\n";
    let r = parse_test_results(stdout, stderr);
    // 单元测试 2 个通过；文档测试的 1 个通过不得计入。
    assert_eq!(
        r.passed, 2,
        "文档测试的 passed 不得累加进单元测试计数\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert_eq!(r.failed, 0);
}

#[test]
fn test_parse_test_results_doc_test_failure_not_counted() {
    // 文档测试失败同样不得污染单元测试的计数与失败列表。
    //
    // 这里的 stdout 是**实测抓取的真实输出**，包含 libtest 的一个易被忽略
    // 的结构：文档测试失败时会出现**两个** `failures:` 标记。
    //
    //   - 第一个紧跟失败条目行之后（无缩进），它下面挂的是
    //     `---- <名称> stdout ----` 与 panic 文本；
    //   - 第二个才是真正的失败清单，条目带缩进。
    //
    // 早先本测试的输入把两者简化成了一个 `failures:`，于是无论
    // `failures` 是否混入文档测试条目，断言都会通过——一次假绿灯。
    // 输入不真实，等于没测。真实 panic 文本（`left: 2` / `right: 3` 等）
    // 也必须保留：它们正是会被误当成失败条目收进列表的那些行。
    let stdout = "\n\
running 2 tests\n\
test it_also_works ... ok\n\
test it_works ... ok\n\
\n\
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n\
\n\
\n\
running 1 test\n\
test src/lib.rs - documented (line 7) ... FAILED\n\
\n\
failures:\n\
\n\
---- src/lib.rs - documented (line 7) stdout ----\n\
Test executable failed (exit status: 101).\n\
\n\
stderr:\n\
\n\
thread 'main' panicked at src/lib.rs:3:1:\n\
assertion `left == right` failed\n\
  left: 2\n\
 right: 3\n\
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n\
\n\
\n\
\n\
failures:\n\
    src/lib.rs - documented (line 7)\n\
\n\
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.57s\n\
";
    // stderr 实测抓取：分段标记在这里，且排在所有 test result 行之后。
    let stderr = "     Running unittests src/lib.rs (target/debug/deps/docfail-9028b339931dbea8)\n   Doc-tests docfail\n";
    let r = parse_test_results(stdout, stderr);
    assert_eq!(r.passed, 2, "文档测试不得计入 passed\nstdout:\n{stdout}");
    assert_eq!(
        r.failed, 0,
        "文档测试的失败不得计入单元测试 failed\nstdout:\n{stdout}"
    );
    // 计数与条目列表必须同口径。只查 failed 会漏掉列表里混入的条目——
    // CLI 会把它打印成一条既没被计入 failed、又无法解释的失败。
    assert!(
        r.failures.is_empty(),
        "文档测试的失败条目不得进入 failures，实际混入: {:?}",
        r.failures.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
}

#[test]
fn test_parse_test_results_real_mixed_targets_with_unit_failure() {
    // 输入是**实测抓取的真实 `cargo test` 输出**，场景为一个同时含
    // 失败单元测试、通过集成测试、零文档测试的 crate（`--no-fail-fast`）。
    //
    // 这个场景同时压三件事：
    // 1. `running 0 tests` 的文档测试块——无条目可判类型，但其
    //    `test result:` 恒为 `0 passed`，跳过与否都不影响 `passed`；
    // 2. 单元测试块里 libtest 同样输出**两个** `failures:`（失败条目行
    //    之后一个、真正清单之前一个），中间夹 `---- failing stdout ----`
    //    与 panic 文本——这些行都不得被当成失败条目；
    // 3. 集成测试块必须正常累加，不得因文档测试块的存在而漏计。
    //
    // 实测解析结果：passed=2 failed=1，failures 恰为 ["failing"]。
    let stdout = "\n\
running 2 tests\n\
test failing ... FAILED\n\
test passing ... ok\n\
\n\
failures:\n\
\n\
---- failing stdout ----\n\
\n\
thread 'failing' panicked at src/lib.rs:5:16:\n\
assertion `left == right` failed: intentional\n\
  left: 2\n\
 right: 3\n\
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n\
\n\
\n\
failures:\n\
    failing\n\
\n\
test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n\
\n\
\n\
running 1 test\n\
test it_passes ... ok\n\
\n\
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n\
\n\
\n\
running 0 tests\n\
\n\
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n\
";
    let stderr = "     Running unittests src/lib.rs (target/debug/deps/failut-1609747)\n   Doc-tests failut\n     Running tests/it.rs (target/debug/deps/it-1610231)\n";
    let r = parse_test_results(stdout, stderr);

    // 2 个单元测试里 1 过 1 败 + 1 个集成测试通过 = passed 2 / failed 1。
    assert_eq!(
        r.passed, 2,
        "单元测试 1 个通过 + 集成测试 1 个通过；文档测试的 0 passed 不影响\nstdout:\n{stdout}"
    );
    assert_eq!(r.failed, 1, "失败的单元测试必须如实计入\nstdout:\n{stdout}");
    // panic 文本、`---- ... ----` 标题等都不得混入失败条目。
    assert_eq!(
        r.failures.len(),
        1,
        "失败条目应恰为 [failing]，实际混入: {:?}",
        r.failures.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
    assert_eq!(r.failures[0].name, "failing");
}

#[test]
fn test_parse_test_results_no_test_result_line_yields_zero() {
    // cargo 进程异常退出（如被 OOM kill）时，输出里根本没有 test result 行。
    // 此时解析结果全为 0，调用方的断言会以 left:0 right:2 失败——这是正确
    // 行为：把「什么都没跑」如实暴露，而不是静默当成通过。
    let stdout = "";
    let stderr = "error: could not compile `test-crate` (lib test) due to 1 previous error\n";
    let r = parse_test_results(stdout, stderr);
    assert_eq!(r.passed, 0);
    assert_eq!(r.failed, 0);
    assert!(r.failures.is_empty());
}
