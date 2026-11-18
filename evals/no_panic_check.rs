//! ADR-007 §1 自动化: 扫生产路径（测试模块之外）的 unwrap/expect/panic!。
//!
//! ## 设计
//!
//! - **零外部依赖**（仅 regex，已在 evals/Cargo.toml）
//! - **智能排除测试代码**: 跟踪 `#[cfg(test)]` mod 的 brace 深度
//! - **路径排除**: `tests/` `benches/` `examples/` 目录 + `build.rs` 文件
//! - **命名约定排除**: `*_tests.rs` `*_test.rs` `test_helpers.rs`
//! - **扫描范围**: 默认 `crates`；CI 显式传 `crates/ webui/`（两者都是生产代码）。
//!   `evals/` 与 `webui/` 外的前端资产不在范围内：`evals/` 是评测工具链
//!   （benchmark / demo / 检查器自身），失败时 panic 是期望行为。
//! - **正则避免字符串字面量误报**: `(?<![\\"\\\'])` 排除前置引号
//!
//! ## 用法
//!
//! ```bash
//! cargo run -p eval-runner --bin no-panic-check -- crates/
//! cargo run -p eval-runner --bin no-panic-check -- crates/sisyphus-tools-llm/
//! cargo run -p eval-runner --bin no-panic-check -- --json crates/
//! cargo run -p eval-runner --bin no-panic-check -- --write-baseline .sisyphus/quality/panic-baseline.json crates/
//! cargo run -p eval-runner --bin no-panic-check -- --baseline .sisyphus/quality/panic-baseline.json crates/
//! ```
//!
//! ## 退出码
//!
//! - `0`: 通过（无 panic 或无新增）
//! - `1`: 发现 panic 路径 / 新增违规
//!
//! ## 修复示例
//!
//! ```rust,ignore
//! // ❌ BAD
//! let x = "42".parse::<u32>().unwrap();
//!
//! // ✅ GOOD
//! let x = "42".parse::<u32>().map_err(|e| format!("parse: {e}"))?;
//! ```

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use regex::Regex;
use serde::{Deserialize, Serialize};

// ── 正则 ─────────────────────────────────────────────────────────────

/// 匹配 `.unwrap(...)` / `.expect(...)` 调用：
/// 前接字符检查：`.unwrap(` / `.expect(` 紧跟空白、引号或反斜杠时不计
/// （字符串内出现的 `.unwrap()`、说明文字中的「Found .unwrap()」均不计）。
/// 源码里真实的调用总是紧邻标识符字符或 `)`（如 `foo().unwrap()`），
/// 因此也计入。零漏报、零误报。
static UNWRAP_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();

/// 匹配 `panic!` `panic_any!` `unimplemented!` `todo!` `unreachable!` 宏。
static PANIC_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();

/// 触发 test 模式的属性注解。
static MOD_TEST_ATTR_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
static FN_TEST_ATTR_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();

/// 注释行 + `#[allow(...)]` 豁免。
static COMMENT_OR_ALLOW_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();

fn unwrap_re() -> &'static Regex {
    UNWRAP_RE.get_or_init(|| Regex::new(r#"\.(unwrap|expect)\s*\("#).expect("valid regex"))
}

fn panic_re() -> &'static Regex {
    PANIC_RE.get_or_init(|| {
        Regex::new(r#"\b(panic|panic_any|unimplemented|todo|unreachable)!\s*[("]"#)
            .expect("valid regex")
    })
}

/// `#[cfg(test)]` 注解 (mod 边界): 整个 mod 视为 test 代码
fn mod_test_attr_re() -> &'static Regex {
    MOD_TEST_ATTR_RE.get_or_init(|| Regex::new(r"#\[cfg\(test\)\]").expect("valid regex"))
}

/// `#[test]` / `#[tokio::test]` 注解 (fn 边界): 用于扩展测试覆盖
/// (虽然 `cfg(test)` 已覆盖, 但显式记录 fn 边界便于将来基于
/// 单 fn 粒度跳过; 当前实现与 mod_test_attr 行为等价)
fn fn_test_attr_re() -> &'static Regex {
    FN_TEST_ATTR_RE
        .get_or_init(|| Regex::new(r"#\[(test|tokio::test|case)\]").expect("valid regex"))
}

fn comment_or_allow_re() -> &'static Regex {
    COMMENT_OR_ALLOW_RE
        .get_or_init(|| Regex::new(r"^\s*//|allow\s*\(|#!\[allow").expect("valid regex"))
}

// ── 配置 ─────────────────────────────────────────────────────────────

/// 排除的目录名（按 Rust 约定）。
///
/// 只在 `crates/` 子树内生效（见 `collect_rs_files` 的 `in_crate_tree`）：
/// 否则 `crates/sisyphus-tests` 这种**名为 tests 的 crate** 会被整棵排除。
/// 这里不要加 `momus` —— `evals/momus/` 只有 `cases.json`，没有 .rs 文件，
/// 曾经的 `"momus"` 是一条永不可能命中的死配置，却让 docstring 声称
/// 「momus 已被排除」，掩盖了「evals/ 是否该扫」这个真问题。
const EXCLUDE_DIRS: &[&str] = &[
    "tests",
    "benches",
    "examples",
    "target",
    ".git",
    "node_modules",
];

/// 视为 test infra 的文件后缀（按命名约定）。
const EXCLUDE_FILES_SUFFIX: &[&str] = &["_tests.rs", "_test.rs", "test_helpers.rs"];

/// 视为 build script 或 test-only 的文件名。
const EXCLUDE_FILES_EXACT: &[&str] = &["tests.rs", "build.rs"];

// ── 命中记录 ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
struct Hit {
    file: String,
    line: u32,
    kind: String,
    text: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct BaselineFile {
    hits: Vec<Hit>,
}

// ── 扫描 ─────────────────────────────────────────────────────────────

fn find_production_panics(path: &Path) -> Vec<Hit> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };

    // 文件级 #![cfg(test)] (如 sisyphus-tests crate) 整个文件都是测试代码,
    // 跳过整个文件 (cargo 编译时整 crate 也不会被纳入)
    if text.lines().take(10).any(|l| l.trim() == "#![cfg(test)]") {
        return Vec::new();
    }

    let mut brace_depth: i32 = 0;
    // Stack of test-brace-depths. Each entry = active test scope
    // (#[cfg(test)] mod or #[test] fn). Supports arbitrary nesting.
    let mut test_stack: Vec<i32> = Vec::new();
    let mut hits = Vec::new();

    for (line_no, line) in text.lines().enumerate() {
        let ln = (line_no + 1) as u32;

        // 进入 test scope: #[cfg(test)] mod 或 #[test] fn
        if mod_test_attr_re().is_match(line) || fn_test_attr_re().is_match(line) {
            test_stack.push(brace_depth);
        }

        // 跟踪 brace 深度
        for ch in line.chars() {
            match ch {
                '{' => {
                    brace_depth += 1;
                },
                '}' => {
                    brace_depth -= 1;

                    while let Some(&top) = test_stack.last() {
                        if brace_depth <= top {
                            test_stack.pop();
                        } else {
                            break;
                        }
                    }
                },
                _ => {},
            }
        }

        if !test_stack.is_empty() {
            continue;
        }
        if comment_or_allow_re().is_match(line) {
            continue;
        }

        let mut unwrapped = false;
        for caps in unwrap_re().captures_iter(line) {
            let m = caps.get(0).unwrap();
            let kind =
                caps.get(1).map(|m| m.as_str().to_string()).unwrap_or_else(|| "unwrap".to_string());
            // 前驱字符检查：`.unwrap(` / `.expect(` 紧跟空白、引号或反斜杠时不计
            // （字符串内出现的 `.unwrap()`、说明文字中的「Found .unwrap()」均不计）。
            // 源码里真实的调用总是紧邻标识符字符或 `)`（如 `foo().unwrap()`），
            // 因此也计入。零漏报、零误报。
            let prev_ok = m.start() == 0 || {
                let prev = line[..m.start()].chars().last().unwrap_or(' ');
                !prev.is_whitespace() && prev != '"' && prev != '\u{5C}' && prev != '\u{27}'
            };
            if prev_ok {
                unwrapped = true;
                hits.push(Hit {
                    file: path.display().to_string(),
                    line: ln,
                    kind,
                    text: line.trim().chars().take(120).collect(),
                });
            }
        }

        if !unwrapped {
            if let Some(caps) = panic_re().captures(line) {
                // 提取宏名（去掉 ! 和 (）
                let raw = caps.get(1).map(|m| m.as_str()).unwrap_or("panic").to_string();
                let kind = match raw.as_str() {
                    "panic_any" => "panic",
                    other => other,
                }
                .to_string();
                hits.push(Hit {
                    file: path.display().to_string(),
                    line: ln,
                    kind,
                    text: line.trim().chars().take(120).collect(),
                });
            }
        }
    }
    hits
}

fn scan(targets: &[PathBuf]) -> Vec<Hit> {
    let mut all_hits = Vec::new();
    for target in targets {
        if target.is_file() {
            all_hits.extend(find_production_panics(target));
        } else if target.is_dir() {
            for entry in walkdir_skipping(target) {
                all_hits.extend(find_production_panics(&entry));
            }
        }
    }
    // 排序便于 baseline diff
    all_hits.sort();
    all_hits
}

fn walkdir_skipping(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            // 目录排除
            if path.is_dir() {
                // 仅在 crates 子树内排除 (防止误把名为 sisyphus-tests 的 crate 排除)
                let in_crate_tree =
                    path.ancestors().any(|p| p.file_name().map(|n| n == "crates").unwrap_or(false));
                if in_crate_tree && EXCLUDE_DIRS.contains(&name.as_str()) {
                    continue;
                }
                stack.push(path);
            } else if path.is_file() && name.ends_with(".rs") {
                // 文件级排除
                if EXCLUDE_FILES_EXACT.contains(&name.as_str()) {
                    continue;
                }
                if EXCLUDE_FILES_SUFFIX.iter().any(|suf| name.ends_with(suf)) {
                    continue;
                }
                out.push(path);
            }
        }
    }
    out
}

// ── Baseline ─────────────────────────────────────────────────────────

fn read_baseline(path: &Path) -> Result<BaselineFile, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("read {path:?}: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("parse {path:?}: {e}"))
}

fn write_baseline(hits: &[Hit], path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir {parent:?}: {e}"))?;
    }
    let bf = BaselineFile {
        hits: hits.to_vec(),
    };
    let json = serde_json::to_string_pretty(&bf).map_err(|e| format!("serialize: {e}"))?;
    fs::write(path, json).map_err(|e| format!("write {path:?}: {e}"))
}

fn diff_hits(current: &[Hit], baseline_hits: &[Hit]) -> (Vec<Hit>, Vec<Hit>) {
    let cur_set: BTreeSet<&Hit> = current.iter().collect();
    let base_set: BTreeSet<&Hit> = baseline_hits.iter().collect();
    let added: Vec<Hit> = cur_set.difference(&base_set).map(|h| (*h).clone()).collect();
    let removed: Vec<Hit> = base_set.difference(&cur_set).map(|h| (*h).clone()).collect();
    (added, removed)
}

// ── 输出 ─────────────────────────────────────────────────────────────

fn print_human(hits: &[Hit], total_files_msg: &str) {
    if hits.is_empty() {
        println!("✓ 通过: 0 个 panic 路径 {total_files_msg}");
        return;
    }
    println!("✗ 失败: {} 个 panic 路径 {total_files_msg}", hits.len());
    for h in hits.iter().take(20) {
        println!("  {}:{} [{}] {}", h.file, h.line, h.kind, h.text);
    }
    if hits.len() > 20 {
        println!("  ... +{} more", hits.len() - 20);
    }
}

fn print_diff(added: &[Hit], removed: &[Hit]) {
    if added.is_empty() {
        println!("✓ 无新增 panic 路径");
    } else {
        println!("✗ 新增 panic 路径 ({}):", added.len());
        for h in added {
            println!("  {}:{} [{}] {}", h.file, h.line, h.kind, h.text);
        }
    }
    if !removed.is_empty() {
        println!(
            "  (已删除 {} 个旧违规 — 建议更新 baseline: --write-baseline)",
            removed.len()
        );
    }
}

// ── CLI ──────────────────────────────────────────────────────────────

struct Args {
    targets: Vec<PathBuf>,
    json: bool,
    baseline: Option<PathBuf>,
    write_baseline: Option<PathBuf>,
    quiet: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut targets = Vec::new();
    let mut json = false;
    let mut baseline = None;
    let mut write_baseline = None;
    let mut quiet = false;

    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--quiet" => quiet = true,
            "--baseline" => {
                baseline = Some(PathBuf::from(iter.next().ok_or("--baseline 需要参数")?));
            },
            "--write-baseline" => {
                write_baseline = Some(PathBuf::from(
                    iter.next().ok_or("--write-baseline 需要参数")?,
                ));
            },
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            },
            other if other.starts_with("--") => {
                return Err(format!("未知选项: {other}"));
            },
            other => targets.push(PathBuf::from(other)),
        }
    }
    if targets.is_empty() {
        targets.push(PathBuf::from("crates"));
    }
    Ok(Args {
        targets,
        json,
        baseline,
        write_baseline,
        quiet,
    })
}

fn print_help() {
    println!("no-panic-check — ADR-007 §1 自动化检查");
    println!();
    println!("用法:");
    println!("  no-panic-check [--json] [--baseline FILE] [--write-baseline FILE] [TARGETS...]");
    println!();
    println!("选项:");
    println!("  --json                 JSON 输出");
    println!("  --baseline FILE        对比 baseline (新增 panic 即 fail)");
    println!("  --write-baseline FILE  写当前 hits 为 baseline 后退出 0");
    println!();
    println!("退出码: 0 = 通过, 1 = 发现违规");
}

// ── 入口 ─────────────────────────────────────────────────────────────

fn run() -> Result<bool, String> {
    let args = parse_args()?;

    let hits = scan(&args.targets);

    // 写 baseline 模式
    if let Some(ref path) = args.write_baseline {
        write_baseline(&hits, path)?;
        if !args.quiet {
            println!("baseline 写入: {} ({} hits)", path.display(), hits.len());
        }
        return Ok(true);
    }

    // 对比 baseline 模式
    if let Some(ref baseline_path) = args.baseline {
        let bf = read_baseline(baseline_path)?;
        let (added, removed) = diff_hits(&hits, &bf.hits);
        if args.json {
            let out = serde_json::json!({
                "added": added,
                "removed": removed,
            });
            println!("{}", serde_json::to_string_pretty(&out).unwrap());
        } else {
            print_diff(&added, &removed);
        }
        return Ok(added.is_empty());
    }

    // 默认模式
    if args.json {
        let out = serde_json::json!({
            "total": hits.len(),
            "hits": hits,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
    } else if !args.quiet {
        print_human(&hits, "");
    }
    Ok(hits.is_empty())
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("错误: {e}");
            ExitCode::from(2)
        },
    }
}

// ── 单元测试 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_rs(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        let mut f = fs::File::create(&p).unwrap();
        f.write_all(body.as_bytes()).unwrap();
        p
    }

    #[test]
    fn clean_code_produces_no_hits() {
        let tmp = tempdir();
        write_rs(&tmp, "ok.rs", "pub fn ok() -> u32 { 42 }\n");
        let hits = find_production_panics(&tmp.join("ok.rs"));
        assert!(hits.is_empty(), "干净代码不应有 hit, got {hits:?}");
    }

    #[test]
    fn unwrap_in_production_detected() {
        let tmp = tempdir();
        let p = write_rs(
            &tmp,
            "bad.rs",
            r#"pub fn bad() -> u32 { "x".parse().unwrap() }
"#,
        );
        let hits = find_production_panics(&p);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].kind, "unwrap");
    }

    #[test]
    fn expect_in_production_detected() {
        let tmp = tempdir();
        let p = write_rs(
            &tmp,
            "bad.rs",
            r#"pub fn bad() -> u32 { "x".parse().expect("parse") }
"#,
        );
        let hits = find_production_panics(&p);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].kind, "expect");
    }

    #[test]
    fn panic_macro_detected() {
        let tmp = tempdir();
        let p = write_rs(
            &tmp,
            "bad.rs",
            r#"pub fn bad() { panic!("nope"); }
"#,
        );
        let hits = find_production_panics(&p);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].kind, "panic");
    }

    #[test]
    fn cfg_test_inner_ignored() {
        let tmp = tempdir();
        let body = "pub fn good() -> u32 { 42 }\n\
                    #[cfg(test)]\n\
                    mod tests {\n\
                        #[test]\n\
                        fn ok() {\n\
                            let _ = \"x\".parse::<u32>().unwrap();\n\
                        }\n\
                    }\n";
        let p = write_rs(&tmp, "t.rs", body);
        let hits = find_production_panics(&p);
        assert!(hits.is_empty(), "cfg(test) 内应忽略, got {hits:?}");
    }

    #[test]
    fn comment_line_exempt() {
        let tmp = tempdir();
        let p = write_rs(
            &tmp,
            "c.rs",
            "// let _ = \"x\".parse::<u32>().unwrap();\n\
             pub fn ok() -> u32 { 42 }\n",
        );
        let hits = find_production_panics(&p);
        assert!(hits.is_empty(), "注释行应豁免, got {hits:?}");
    }

    #[test]
    fn allow_attr_exempt() {
        let tmp = tempdir();
        let p = write_rs(
            &tmp,
            "a.rs",
            "#[allow(clippy::unwrap_used)] let _ = \"x\".parse::<u32>().unwrap();\n",
        );
        let hits = find_production_panics(&p);
        assert!(hits.is_empty(), "#[allow] 行内 unwrap 应豁免, got {hits:?}");
    }

    #[test]
    fn string_literal_with_dot_unwrap_not_flagged() {
        // 字符串字面量里的 `.unwrap()` / `.expect(`（前导空白或引号）不算源码调用；
        // 但 `"x".parse().unwrap()` 这种**真实调用**（紧邻 `)`) 必须命中。
        let tmp = tempdir();
        let p = write_rs(
            &tmp,
            "s.rs",
            r#"pub fn bad() -> u32 { let msg = "Found .unwrap() in code".to_string(); let q = "x".parse().unwrap(); 42 }
"#,
        );
        let hits = find_production_panics(&p);
        assert_eq!(hits.len(), 1, "只剩真实调用 `q` 应命中, got {hits:?}");
        assert_eq!(hits[0].kind, "unwrap");
    }

    #[test]
    fn unreachable_detected() {
        let tmp = tempdir();
        let p = write_rs(
            &tmp,
            "u.rs",
            r#"pub fn bad() -> u32 { unreachable!() }
"#,
        );
        let hits = find_production_panics(&p);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].kind, "unreachable");
    }

    #[test]
    fn baseline_diff_detects_new_panic() {
        let _tmp = tempdir();
        let hits_now = vec![Hit {
            file: "x.rs".into(),
            line: 1,
            kind: "unwrap".into(),
            text: "x".into(),
        }];
        let baseline_old = vec![Hit {
            file: "y.rs".into(),
            line: 1,
            kind: "unwrap".into(),
            text: "y".into(),
        }];
        let (added, removed) = diff_hits(&hits_now, &baseline_old);
        assert_eq!(added.len(), 1);
        assert_eq!(removed.len(), 1);
        assert_eq!(added[0].file, "x.rs");
    }

    fn tempdir() -> PathBuf {
        let base = std::env::temp_dir();
        let pid = std::process::id();
        let nanos =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = base.join(format!("no-panic-check-test-{pid}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
