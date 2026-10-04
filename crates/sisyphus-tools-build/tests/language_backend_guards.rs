//! 三种语言后端 `test()` 的「零测试被执行」守卫：真实工具链端到端回归。
//!
//! 背景：`LanguageBackend::test()` 的解析器只认自己的通过/失败标记，遇到
//! **构建期失败**（编译错误 / 套件加载失败 / 测试收集报错）时会如实给出
//! `passed=0 / failed=0`。唯一的生产消费点 `sisyphus-cli/src/build.rs` 是
//! `if r.failed == 0 { "ok" }`，于是构建失败会被显示成「测试全部通过」。
//! 三个后端为此各自加了守卫，本文件锁住这些守卫的判据。
//!
//! **这些用例真的拉起 go / pytest / vitest 子进程**，因此：
//! - 需要对应工具链在 PATH 上；
//! - 因此统一标 `#[ignore = "requires real toolchain"]`，不进默认 `cargo test`。
//!   与 `bridge_integration_test` 用 `#[ignore]` 的理由不同——那些是因为 OOM，
//!   这些只是慢（pytest 约 0.2s、vitest 约 0.4s、go 首次约 1s）且依赖外部工具。
//!
//! 运行方式：
//!
//! ```sh
//! cargo test -p sisyphus-tools-build --test language_backend_guards \
//!     -- --ignored --test-threads=1
//! ```
//!
//! 每个探针工程都在测试内用 `TempDir` 现造，不依赖任何外部目录或环境变量。

use sisyphus_tools_build::build::golang::GoBackend;
use sisyphus_tools_build::build::java::JavaBackend;
use sisyphus_tools_build::build::python::PythonBackend;
use sisyphus_tools_build::build::LanguageBackend;
use std::fs;
use std::path::Path;

// ── 探针工程构造 ────────────────────────────────────────

/// 写一个单包 Go 模块；`test_go` 为 `ok_test.go`，`None` 时写一个编译不过的
/// `main.go`。
fn go_module(dir: &Path, test_go: Option<&str>) {
    fs::write(dir.join("go.mod"), "module probe\n\ngo 1.21\n").unwrap();
    match test_go {
        Some(t) => fs::write(dir.join("ok_test.go"), t).unwrap(),
        None => fs::write(
            dir.join("main.go"),
            "package main\n\nfunc main() { undefinedFunc() }\n",
        )
        .unwrap(),
    }
}

fn go_package(dir: &Path, name: &str, src: &str) {
    let p = dir.join(name);
    fs::create_dir_all(&p).unwrap();
    // 文件名必须带 `.go` 后缀，否则 `go test ./...` 会直接跳过这个目录，
    // 探针就测不到「多包里只有一个包构建失败」这个场景了。
    fs::write(p.join(format!("{name}.go")), src).unwrap();
}

const GO_PASSING_TEST: &str = r#"package probe

import "testing"

func TestOK(t *testing.T) {
	if 1+1 != 2 {
		t.Fatal("bad math")
	}
}
"#;

const GO_BROKEN_SRC: &str = r#"package badpkg

func Broken() { undefinedFunc() }
"#;

// ── Go ─────────────────────────────────────────────────

/// 单包编译失败 → Err，且错误信息带上 go 原始诊断。
#[test]
#[ignore = "requires real go toolchain"]
fn go_build_failure_returns_err() {
    let dir = tempfile::tempdir().unwrap();
    go_module(dir.path(), None);
    let err = GoBackend
        .test(None, None, false, Some(dir.path()))
        .expect_err("go 包编译失败必须返回 Err，不能伪装成 0 passed / 0 failed");
    let msg = err.to_string();
    println!("err = {msg}");
    assert!(
        msg.contains("构建失败"),
        "错误信息应说明有包构建失败，实际: {msg}"
    );
    // 原始诊断必须透出：包路径与 undefined 符号都要能看到。
    assert!(
        msg.contains("undefined: undefinedFunc"),
        "错误信息必须含 go 原始诊断，实际: {msg}"
    );
}

/// 多包工程里**只有一个包**构建失败 → 仍必须 Err。
///
/// 这是本文件最重要的一条。`go test ./...` 在这种情况下会照常执行其余能
/// 编译的包，stdout 里同时出现 `--- PASS: TestGood` 与
/// `FAIL\tprobe/badpkg [build failed]`。
///
/// 守卫若按「输出里没有 `--- PASS` 行」判定（修正前的写法）就会在这里
/// **漏判**：那个构建失败的包被彻底吞掉，CLI 仍然报 ok。
#[test]
#[ignore = "requires real go toolchain"]
fn go_partial_build_failure_still_returns_err() {
    let dir = tempfile::tempdir().unwrap();
    go_module(dir.path(), Some(GO_PASSING_TEST));
    go_package(dir.path(), "badpkg", GO_BROKEN_SRC);
    go_package(
        dir.path(),
        "goodpkg",
        r#"package goodpkg

import "testing"

func TestGood(t *testing.T) {
	if 1+1 != 2 {
		t.Fatal("bad math")
	}
}
"#,
    );
    // goodpkg 必须带 _test.go 后缀才会真的执行 TestGood。
    fs::write(
        dir.path().join("goodpkg").join("good_test.go"),
        r#"package goodpkg

import "testing"

func TestGood(t *testing.T) {
	if 1+1 != 2 {
		t.Fatal("bad math")
	}
}
"#,
    )
    .unwrap();

    let err = GoBackend
        .test(None, None, false, Some(dir.path()))
        .expect_err("多包场景下单个包构建失败也必须返回 Err");
    let msg = err.to_string();
    println!("err = {msg}");
    assert!(
        msg.contains("构建失败"),
        "错误信息应说明有包构建失败，实际: {msg}"
    );
    assert!(
        msg.contains("undefined: undefinedFunc"),
        "错误信息必须指向真正失败的包，实际: {msg}"
    );
}

/// 全部通过 → Ok，且 passed 正常计数（守卫不得误伤）。
#[test]
#[ignore = "requires real go toolchain"]
fn go_passing_project_is_not_rejected() {
    let dir = tempfile::tempdir().unwrap();
    go_module(dir.path(), Some(GO_PASSING_TEST));
    let r = GoBackend
        .test(None, None, false, Some(dir.path()))
        .expect("全部通过的 Go 项目不得被守卫拒绝");
    println!(
        "passed={} failed={} failures={:?}",
        r.passed,
        r.failed,
        r.failures.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
    assert_eq!(r.passed, 1);
    assert_eq!(r.failed, 0);
}

/// 用例真的失败（而非构建失败）→ 必须照常解析出失败用例，**不得**被守卫
/// 拦成 Err。
///
/// 守卫的全部意义就是区分「一个测试都没跑」与「测试跑了并失败」。这条是
/// 防呆：一旦守卫写得太宽，这里就会失败。
#[test]
#[ignore = "requires real go toolchain"]
fn go_real_test_failure_is_parsed_not_rejected() {
    let dir = tempfile::tempdir().unwrap();
    go_module(
        dir.path(),
        Some(
            r#"package probe

import "testing"

func TestFails(t *testing.T) { t.Fatal("boom") }
"#,
        ),
    );
    let r = GoBackend
        .test(None, None, false, Some(dir.path()))
        .expect("测试真的失败时必须照常解析，而不是返回 Err");
    println!("passed={} failed={}", r.passed, r.failed);
    assert_eq!(r.failed, 1, "失败用例必须被解析出来");
    assert_eq!(r.passed, 0);
    assert_eq!(r.failures.len(), 1);
    assert!(r.failures[0].name.contains("TestFails"));
}

// ── Python / pytest ────────────────────────────────────

fn python_backend() -> PythonBackend {
    PythonBackend::new().expect("需要 python3")
}

/// 收集阶段语法错误 → Err，且错误信息带上 pytest 原始诊断。
#[test]
#[ignore = "requires real python3 + pytest"]
fn pytest_collection_error_returns_err() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("test_x.py"), "def test_broken(:\n").unwrap();
    let err = python_backend()
        .test(None, None, false, Some(dir.path()))
        .expect_err("pytest 收集失败必须返回 Err，不能伪装成 0 passed / 0 failed");
    let msg = err.to_string();
    println!("err = {msg}");
    assert!(
        msg.contains("收集阶段出错"),
        "错误信息应说明收集阶段出错，实际: {msg}"
    );
    assert!(
        msg.contains("SyntaxError"),
        "错误信息必须含 pytest 原始诊断，实际: {msg}"
    );
}

/// **假阳性防线**：fixture 在 setup 阶段抛错时，测试用例确实被收集并执行了
/// （只是 setup 失败）。
///
/// 修正前的守卫用 `stdout.contains("ERROR ") && !stdout.contains("passed")`
/// 判定。fixture 错误的输出是 `ERROR test_use.py::test_x` +
/// `1 error in 1.2s`，**恰好不含** `passed`，于是被误报成「收集阶段出错」
/// 并返回 Err。真实失败与收集失败对调用方是完全不同的信号，不能混为一谈。
#[test]
#[ignore = "requires real python3 + pytest"]
fn pytest_fixture_error_is_not_misreported_as_collection_error() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("conftest.py"),
        "import pytest\n\n@pytest.fixture\ndef broken():\n    raise RuntimeError(\"fixture blew up\")\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("test_use.py"),
        "def test_uses_fixture(broken):\n    assert True\n",
    )
    .unwrap();

    let r = python_backend()
        .test(None, None, false, Some(dir.path()))
        .expect("fixture 报错时测试确实被跑过，必须照常返回 Ok(TestResults)");
    println!("passed={} failed={}", r.passed, r.failed);
    assert_eq!(r.passed, 0, "setup 失败的用例不算通过");
    // 关键：不得被守卫拦成 Err。
}

/// **假阳性防线**：工程路径里带 `passed` 字样时，真正的收集错误不得漏判。
///
/// 修正前的守卫带 `&& !stdout.contains("passed")`。若项目目录名含 `passed`
/// （例如 `pytest_named_passed/`），pytest 会把绝对路径打进
/// `rootdir: .../pytest_named_passed`，于是 `passed` 出现，真正的收集错误
/// 反而被**漏判**成 0/0 假成功。
#[test]
#[ignore = "requires real python3 + pytest"]
fn pytest_collection_error_is_detected_even_when_path_contains_passed() {
    // 直接在被 gitignore 的临时目录里造一个名字含 `passed` 的父目录。
    let base = tempfile::tempdir().unwrap();
    let dir = base.path().join("probe_passed");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("test_x.py"), "def test_broken(:\n").unwrap();

    let err = python_backend()
        .test(None, None, false, Some(&dir))
        .expect_err("路径含 passed 不得让收集错误漏判成假成功");
    let msg = err.to_string();
    println!("err = {msg}");
    assert!(
        msg.contains("收集阶段出错"),
        "错误信息应说明收集阶段出错，实际: {msg}"
    );
}

/// 全部通过 → Ok，且 passed 正常计数。
///
/// 这条同时锁住一个真回归：旧解析器用 `strip_suffix(" passed")`，
/// 而 pytest 单项全过时输出是 `1 passed in 0.36s`，整行结尾是 ` in 0.36s`，
/// 后缀根本不匹配，于是 passed=0。全通过的项目反而被报成零测试。
#[test]
#[ignore = "requires real python3 + pytest"]
fn pytest_passing_project_is_not_rejected() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("test_ok.py"),
        "def test_ok():\n    assert 1 + 1 == 2\n",
    )
    .unwrap();
    let r = python_backend()
        .test(None, None, false, Some(dir.path()))
        .expect("全部通过的 Python 项目不得被守卫拒绝");
    println!("passed={} failed={}", r.passed, r.failed);
    assert_eq!(r.passed, 1);
    assert_eq!(r.failed, 0);
}

/// 混合结果 → passed / failed 都取到确切数字。
///
/// 真实输出为 `1 failed, 1 passed in 0.21s`，同一行既有 failed 又有
/// passed，取数时不能只认其中一个。
#[test]
#[ignore = "requires real python3 + pytest"]
fn pytest_mixed_run_reports_both_passed_and_failed() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("test_ok.py"),
        "def test_ok():\n    assert 1 + 1 == 2\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("test_bad.py"),
        "def test_bad():\n    raise ValueError(\"boom\")\n",
    )
    .unwrap();
    let r = python_backend()
        .test(None, None, false, Some(dir.path()))
        .expect("用例真失败必须照常解析成 Ok(有 failed)，不得被守卫拦成 Err");
    println!("passed={} failed={}", r.passed, r.failed);
    assert_eq!(r.passed, 1);
    assert_eq!(r.failed, 1);
}

/// 跳过 → 计入 ignored，不得混入 passed。
#[test]
#[ignore = "requires real python3 + pytest"]
fn pytest_skipped_is_counted_as_ignored() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("test_sk.py"),
        "import pytest\n\n@pytest.mark.skip\ndef test_skipped():\n    pass\n",
    )
    .unwrap();
    let r = python_backend()
        .test(None, None, false, Some(dir.path()))
        .expect("全部跳过的 Python 项目不得被守卫拒绝");
    println!(
        "passed={} failed={} ignored={}",
        r.passed, r.failed, r.ignored
    );
    assert_eq!(r.passed, 0);
    assert_eq!(r.ignored, 1);
}

/// 用例真的失败 → 必须照常解析，不得被守卫拦成 Err。
#[test]
#[ignore = "requires real python3 + pytest"]
fn pytest_real_failure_is_parsed_not_rejected() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("test_bad.py"),
        "def test_bad():\n    raise ValueError(\"boom\")\n",
    )
    .unwrap();
    let r = python_backend()
        .test(None, None, false, Some(dir.path()))
        .expect("测试真的失败时必须照常解析，而不是返回 Err");
    println!("passed={} failed={}", r.passed, r.failed);
    assert_eq!(r.failed, 1, "失败用例必须被解析出来");
    assert_eq!(r.passed, 0);
}

// ── Java / Maven ────────────────────────────────────────

/// 写一个最小可用的 Maven 工程；`broken` 为 true 时额外放一个编译不过的类。
///
/// Maven 比 Go/Python 慢很多（首次要拉取插件与依赖，实测联网约 70s，
/// 之后靠 `~/.m2` 缓存），所以两条用例都标 `#[ignore]`。
fn maven_project(dir: &Path, broken: bool) {
    fs::write(
        dir.join("pom.xml"),
        r#"<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>probe</groupId>
  <artifactId>probe</artifactId>
  <version>1.0</version>
  <packaging>jar</packaging>
  <properties>
    <maven.compiler.source>21</maven.compiler.source>
    <maven.compiler.target>21</maven.compiler.target>
    <project.build.sourceEncoding>UTF-8</project.build.sourceEncoding>
  </properties>
  <dependencies>
    <dependency>
      <groupId>org.junit.jupiter</groupId>
      <artifactId>junit-jupiter</artifactId>
      <version>5.10.2</version>
      <scope>test</scope>
    </dependency>
  </dependencies>
  <build>
    <plugins>
      <plugin>
        <groupId>org.apache.maven.plugins</groupId>
        <artifactId>maven-surefire-plugin</artifactId>
        <version>3.2.5</version>
      </plugin>
    </plugins>
  </build>
</project>
"#,
    )
    .unwrap();

    let main = dir.join("src/main/java/probe");
    let test = dir.join("src/test/java/probe");
    fs::create_dir_all(&main).unwrap();
    fs::create_dir_all(&test).unwrap();

    if broken {
        fs::write(
            main.join("Broken.java"),
            "package probe;\n\npublic class Broken {\n    void boom() {\n        undefinedFunc();\n    }\n}\n",
        )
        .unwrap();
    }
    fs::write(
        test.join("OkTest.java"),
        "package probe;\n\nimport org.junit.jupiter.api.Test;\nimport static org.junit.jupiter.api.Assertions.assertEquals;\n\npublic class OkTest {\n    @Test\n    void passes() {\n        assertEquals(2, 1 + 1);\n    }\n}\n",
    )
    .unwrap();
}

fn maven_backend() -> JavaBackend {
    JavaBackend::new_maven()
}

/// Maven 编译失败 → Err。
///
/// 实测（mvn 3.10 + surefire 3.2.5）：编译失败时**一条 `Tests run` 都不打印**，
/// 只有 `[ERROR] COMPILATION ERROR :`。解析器对这种输出只能给 0/0，
/// 再经 `if r.failed == 0 { "ok" }` 就会被显示成「0 个测试全部通过」。
#[test]
#[ignore = "requires real mvn + jdk (slow: downloads plugins/deps)"]
fn maven_compile_failure_returns_err() {
    let dir = tempfile::tempdir().unwrap();
    maven_project(dir.path(), true);
    let err = maven_backend()
        .test(None, None, false, Some(dir.path()))
        .expect_err("Maven 编译失败必须返回 Err，不能伪装成 0 passed / 0 failed");
    let msg = err.to_string();
    println!("err = {msg}");
    assert!(
        msg.contains("编译阶段失败"),
        "错误信息应说明编译阶段失败，实际: {msg}"
    );
    assert!(
        msg.contains("cannot find symbol"),
        "错误信息必须透出 javac 原始诊断，实际: {msg}"
    );
}

/// Maven 全部通过 → Ok，且 passed 正常计数。
///
/// 真实输出每行都带 `[INFO] ` 前缀（`[INFO] Tests run: 1, ...`），
/// 旧的 `starts_with("Tests run:")` 一条都匹配不上，会把通过的项目报成 0。
#[test]
#[ignore = "requires real mvn + jdk (slow: downloads plugins/deps)"]
fn maven_passing_project_counts_passed() {
    let dir = tempfile::tempdir().unwrap();
    maven_project(dir.path(), false);
    let r = maven_backend()
        .test(None, None, false, Some(dir.path()))
        .expect("全部通过的 Maven 项目不得被守卫拒绝");
    println!("passed={} failed={}", r.passed, r.failed);
    assert_eq!(r.passed, 1, "真实 mvn 输出带 [INFO] 前缀，必须能取到计数");
    assert_eq!(r.failed, 0);
}
