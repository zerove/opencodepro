//! Golden SOP 库评测历史趋势聚合 (P3-10 第二步辅助工具)
//!
//! 读取 `.sisyphus/eval-history/` 下所有 JSONL 文件（结构 `golden-sop-*.jsonl`
//! 与执行 `golden-sop-exec-*.jsonl`），按时间排序并打印趋势。
//!
//! 用途：nightly 跑完 `golden-sop-eval` + `golden-sop-exec` 后，
//! 用本工具做"最近 N 次的趋势对比"，把 JSONL 转成可读表格。
//!
//! 运行：`cargo run -p eval-runner --bin golden-sop-trend`

use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Default)]
struct TrendRow {
    timestamp: u64,
    total: usize,
    passed: usize,
    failed: usize,
    pass_rate: f64,
    extra: HashMap<String, f64>,
}

fn parse_structural(line: &str) -> Option<TrendRow> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    Some(TrendRow {
        timestamp: v.get("timestamp")?.as_u64()?,
        total: v.get("total")?.as_u64()? as usize,
        passed: v.get("passed")?.as_u64()? as usize,
        failed: v.get("failed")?.as_u64()? as usize,
        pass_rate: v.get("pass_rate")?.as_f64()?,
        extra: HashMap::new(),
    })
}

fn parse_exec(line: &str) -> Option<TrendRow> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let mut extra = HashMap::new();
    if let Some(n) = v.get("avg_node_jaccard").and_then(|x| x.as_f64()) {
        extra.insert("node_jaccard".to_string(), n);
    }
    if let Some(e) = v.get("avg_edge_jaccard").and_then(|x| x.as_f64()) {
        extra.insert("edge_jaccard".to_string(), e);
    }
    if let Some(l) = v.get("avg_synth_latency_ms").and_then(|x| x.as_u64()) {
        extra.insert("synth_latency_ms".to_string(), l as f64);
    }
    Some(TrendRow {
        timestamp: v.get("timestamp")?.as_u64()?,
        total: v.get("total")?.as_u64()? as usize,
        passed: v.get("passed")?.as_u64()? as usize,
        failed: v.get("failed")?.as_u64()? as usize,
        pass_rate: v.get("pass_rate")?.as_f64()?,
        extra,
    })
}

fn collect(dir: &Path, prefix: &str, parser: fn(&str) -> Option<TrendRow>) -> Vec<TrendRow> {
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with(prefix) || !name.ends_with(".jsonl") {
                continue;
            }
            if let Ok(content) = fs::read_to_string(entry.path()) {
                for line in content.lines() {
                    if let Some(row) = parser(line) {
                        out.push(row);
                    }
                }
            }
        }
    }
    out.sort_by_key(|r| r.timestamp);
    out
}

fn print_table(label: &str, rows: &[TrendRow]) {
    println!("\n=== {label} trend ({} records) ===", rows.len());
    if rows.is_empty() {
        println!("  (no data)");
        return;
    }
    let extra_keys: Vec<String> = rows
        .iter()
        .flat_map(|r| r.extra.keys().cloned())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    print!("  {:<14} {:>6} {:>6} {:>6} {:>8}", "timestamp", "total", "pass", "fail", "rate");
    for k in &extra_keys {
        print!(" {:>14}", k);
    }
    println!();
    for r in rows {
        let ts_str = chrono_like_ts(r.timestamp);
        print!(
            "  {:<14} {:>6} {:>6} {:>6} {:>7.1}%",
            ts_str,
            r.total,
            r.passed,
            r.failed,
            100.0 * r.pass_rate
        );
        for k in &extra_keys {
            let v = r.extra.get(k).copied().unwrap_or(0.0);
            print!(" {:>14.3}", v);
        }
        println!();
    }
}

fn chrono_like_ts(unix_secs: u64) -> String {
    // 不引入 chrono 依赖。简单格式：YYYY-MM-DD HH:MM (UTC)
    let secs_per_day = 86_400u64;
    let secs_per_hour = 3_600u64;
    let secs_per_min = 60u64;
    let days = unix_secs / secs_per_day;
    let rem = unix_secs % secs_per_day;
    let h = rem / secs_per_hour;
    let m = (rem % secs_per_hour) / secs_per_min;
    // 1970-01-01 为基准
    let (y, mo, d) = days_to_ymd(days);
    format!("{:04}-{:02}-{:02} {:02}:{:02}", y, mo, d, h, m)
}

fn days_to_ymd(days: u64) -> (u32, u32, u32) {
    // 简化公历转换：1970-01-01 起算
    let mut y = 1970u32;
    let mut d = days;
    loop {
        let leap = (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0);
        let dy = if leap { 366 } else { 365 };
        if d >= dy {
            d -= dy;
            y += 1;
        } else {
            break;
        }
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0);
    let months = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut mo = 1u32;
    for &dm in &months {
        if d >= dm as u64 {
            d -= dm as u64;
            mo += 1;
        } else {
            break;
        }
    }
    (y, mo, d as u32 + 1)
}

fn main() -> anyhow::Result<()> {
    let history_dir = Path::new(".sisyphus/eval-history");
    if !history_dir.exists() {
        println!("No eval-history dir at {}", history_dir.display());
        return Ok(());
    }

    let structural = collect(history_dir, "golden-sop-", parse_structural);
    let exec = collect(history_dir, "golden-sop-exec-", parse_exec);

    // 过滤掉 exec 行（它们也匹配 golden-sop- 前缀）；按 timestamp 区分
    let structural_only: Vec<TrendRow> = structural
        .into_iter()
        .filter(|r| !exec.iter().any(|e| e.timestamp == r.timestamp))
        .collect();

    print_table("Structural (eval.rs)", &structural_only);
    print_table("Execution (exec_eval.rs)", &exec);

    // 总体摘要
    println!("\n=== Summary ===");
    if let Some(latest) = exec.last() {
        println!(
            "  Latest execution: pass_rate={:.1}%, node_jaccard={:.3}, edge_jaccard={:.3}",
            100.0 * latest.pass_rate,
            latest.extra.get("node_jaccard").copied().unwrap_or(0.0),
            latest.extra.get("edge_jaccard").copied().unwrap_or(0.0),
        );
    }
    if let Some(latest) = structural_only.last() {
        println!(
            "  Latest structural: pass_rate={:.1}% ({}/{})",
            100.0 * latest.pass_rate,
            latest.passed,
            latest.total
        );
    }

    Ok(())
}
