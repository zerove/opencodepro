//! ADR-007 §4 自动化: 量化 sisyphus_metrics 覆盖率, 识别未埋点的关键 crate。
//!
//! 输出每个生产 crate 的 metrics 使用情况 (helper 调用 / 内部 macro / tracing span),
//! 并对没有埋点的 crate 给出警告, 列出值得埋点的关键 operation 类型。
//!
//! 运行:
//! ```text
//! cargo run -p eval-runner --bin metrics-coverage -- crates/
//! cargo run -p eval-runner --bin metrics-coverage -- --json crates/ | jq
//! ```
//!
//! 退出码:
//! - 0: 通过 (没有新增未埋点的关键 crate)
//! - 1: 发现应该埋点但未埋点的关键 crate
//!
//! ADR-007 §4 要求每个 production crate 至少有一条 metrics 埋点 (counter/gauge/histogram
//! 或 sisyphus_metrics::* helper), 用于 Prometheus 抓取。

use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Default, Serialize)]
struct CrateReport {
    name: String,
    has_metrics: bool,
    helper_calls: usize,
    macro_calls: usize,
    tracing_spans: usize,
    /// 关键 operation 缺失的埋点 (建议)
    missing_instrumentation: Vec<&'static str>,
    /// 文件路径列表 (有 metrics 调用的)
    files: Vec<String>,
}

/// 关键 operation 模式, 每个 crate 至少应该有其中之一的埋点
const KEY_OPS: &[(&str, &[&str])] = &[
    ("sisyphus-bridge", &["sop_execution", "bridge_request", "shutdown"]),
    ("sisyphus-core", &["cognitive_loop", "dag_execution", "reflection"]),
    ("sisyphus-intel", &["simulation", "reflection", "evolution"]),
    ("sisyphus-mem", &["cache_hit", "cache_miss", "sql_query", "embedding_store"]),
    ("sisyphus-tools-llm", &["llm_request", "token_usage", "error_rate"]),
    ("sisyphus-tools-executor", &["command_execution", "tool_invoke"]),
    ("sisyphus-tools-mcp", &["mcp_request", "mcp_error"]),
    ("sisyphus-sandbox", &["wasm_execution", "sandbox_init"]),
    ("sisyphus-sandboxd", &["daemon_request", "wasm_load"]),
    ("sisyphus-bridge", &["grpc_request", "auth_check"]),
    ("sisyphus-knowledge", &["asset_query", "asset_save"]),
    ("sisyphus-roadmap", &["milestone_update", "goal_complete"]),
    ("sisyphus-cli", &["tui_event", "command_invoke"]),
    ("sisyphus-types", &[]),
    ("sisyphus-metrics", &[]),
];

fn analyze_crate(crate_path: &Path) -> CrateReport {
    let name = crate_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("?")
        .to_string();

    let mut report = CrateReport {
        name: name.clone(),
        ..Default::default()
    };

    // 遍历 src/ 找 metrics 调用
    let src_dir = crate_path.join("src");
    if !src_dir.exists() {
        return report;
    }

    let helper_re = regex::Regex::new(r"\bsisyphus_metrics::\w+").unwrap();
    let macro_re = regex::Regex::new(r"\b(counter|gauge|histogram|describe_counter|describe_gauge|describe_histogram)!\s*\(").unwrap();
    let span_re = regex::Regex::new(r"#\[[^\]]*tracing::instrument[^\]]*\]").unwrap();

    for entry in walkdir::WalkDir::new(&src_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }
        let Ok(s) = std::fs::read_to_string(p) else { continue };

        let n_helper = helper_re.find_iter(&s).count();
        let n_macro = macro_re.find_iter(&s).count();
        let n_span = span_re.find_iter(&s).count();

        if n_helper + n_macro + n_span > 0 {
            report.helper_calls += n_helper;
            report.macro_calls += n_macro;
            report.tracing_spans += n_span;
            report
                .files
                .push(p.strip_prefix(crate_path).unwrap_or(p).display().to_string());
        }
    }

    report.has_metrics = report.helper_calls + report.macro_calls + report.tracing_spans > 0;

    // 关键 operation 缺失检查
    if let Some((_, expected_ops)) = KEY_OPS.iter().find(|(c, _)| *c == name) {
        for op in *expected_ops {
            // 简化: 如果 has_metrics = false, 全部列为 missing
            if !report.has_metrics {
                report.missing_instrumentation.push(op);
            }
        }
    }

    report
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let json_mode = args.iter().any(|a| a == "--json");
    let target_dir = args
        .iter()
        .skip(1)
        .find(|a| !a.starts_with("--"))
        .map(String::as_str)
        .unwrap_or("crates/");

    let mut reports: BTreeMap<String, CrateReport> = BTreeMap::new();
    let mut no_metrics: Vec<String> = Vec::new();

    for entry in walkdir::WalkDir::new(target_dir)
        .max_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let p = entry.path();
        if !p.is_dir() || p == Path::new(target_dir) {
            continue;
        }
        // 跳过非 crate
        if !p.join("Cargo.toml").exists() {
            continue;
        }
        let r = analyze_crate(p);
        if !r.has_metrics && !r.missing_instrumentation.is_empty() {
            no_metrics.push(r.name.clone());
        }
        reports.insert(r.name.clone(), r);
    }

    if json_mode {
        println!("{}", serde_json::to_string_pretty(&reports.values().collect::<Vec<_>>())?);
    } else {
        println!("=== ADR-007 §4 Metrics 覆盖率报告 ===\n");
        println!(
            "{:<28} {:>8} {:>8} {:>8} {:>8}  状态",
            "Crate", "helper", "macro", "span", "total"
        );
        println!("{}", "-".repeat(78));
        let total: usize = reports.len();
        let with: usize = reports.values().filter(|r| r.has_metrics).count();
        for r in reports.values() {
            let tot = r.helper_calls + r.macro_calls + r.tracing_spans;
            let status = if r.has_metrics { "✓" } else { "✗" };
            println!(
                "{:<28} {:>8} {:>8} {:>8} {:>8}  {}",
                r.name, r.helper_calls, r.macro_calls, r.tracing_spans, tot, status
            );
        }
        println!("\n汇总: {}/{} crate 有 metrics 埋点 ({}%)", with, total, with * 100 / total.max(1));

        if !no_metrics.is_empty() {
            println!("\n⚠️  以下 crate 应埋点但未埋点 (ADR-007 §4 违反):");
            for c in &no_metrics {
                if let Some(r) = reports.get(c) {
                    println!("  - {} (建议: {})", c, r.missing_instrumentation.join(", "));
                }
            }
        }
    }

    // 退出码: 有"关键 op 应埋点但未埋点"则 1
    if no_metrics.is_empty() {
        std::process::exit(0);
    } else {
        std::process::exit(1);
    }
}
