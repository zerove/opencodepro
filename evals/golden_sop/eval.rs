//! Golden SOP 库评测（P3-10 第一步：种子库 + 离线结构评测）。
//!
//! 与 evals/runner.rs 的 LLM-agent 评测不同，本文件是 **离线结构评测**：
//! 不执行 SOP、不调用 LLM，只验证种子库自身的一致性，并对每个用例的
//! 标准答案图做结构指标核算（节点数边界、期望的节点种类存在性）。
//! 后续接入真实合成/执行管线时，nightly 任务可在此基线上追加
//! 通过率/成本/迭代数趋势记录。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

// ── 用例模型 ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct GoldenCase {
    id: String,
    #[allow(dead_code)]
    goal: String,
    category: String,
    difficulty: String,
    expected_sop: ExpectedSop,
    baseline_metrics: BaselineMetrics,
    #[allow(dead_code)]
    notes: String,
}

#[derive(Debug, Deserialize)]
struct ExpectedSop {
    /// 节点定义。kind 以 JSON 表达（NodeKind 的 serde 表示），
    /// 结构检查只提取 type 字段，避免与 types crate 的枚举表示强耦合。
    nodes: Vec<SopNodeJson>,
    edges: Vec<Vec<usize>>,
}

#[derive(Debug, Deserialize, Clone)]
struct SopNodeJson {
    name: String,
    kind: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct BaselineMetrics {
    min_nodes: usize,
    max_nodes: usize,
    #[serde(default)]
    expects_retry: bool,
    #[serde(default)]
    expects_test: bool,
    #[serde(default)]
    expects_branch: bool,
}

/// 单个用例的结构评测结果。
#[derive(Debug, Serialize)]
struct CaseCheck {
    id: String,
    passed: bool,
    failures: Vec<String>,
}

impl CaseCheck {
    fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            passed: true,
            failures: Vec::new(),
        }
    }
    fn fail(&mut self, msg: String) {
        self.passed = false;
        self.failures.push(msg);
    }
}

// ── 检查逻辑 ────────────────────────────────────────────────

fn node_kind(node: &SopNodeJson) -> String {
    node.kind
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("<missing>")
        .to_string()
}

/// 图是否有环（Kahn 拓扑计数）。
fn is_acyclic(n: usize, edges: &[Vec<usize>]) -> bool {
    let mut indegree = vec![0usize; n];
    for e in edges {
        if e.len() == 2 {
            indegree[e[1]] += 1;
        }
    }
    let mut queue: Vec<usize> = (0..n).filter(|&i| indegree[i] == 0).collect();
    let mut visited = 0;
    while let Some(u) = queue.pop() {
        visited += 1;
        for e in edges {
            if e.len() == 2 && e[0] == u {
                indegree[e[1]] -= 1;
                if indegree[e[1]] == 0 {
                    queue.push(e[1]);
                }
            }
        }
    }
    visited == n
}

fn check_case(case: &GoldenCase) -> CaseCheck {
    let mut check = CaseCheck::new(&case.id);
    let nodes = &case.expected_sop.nodes;
    let edges = &case.expected_sop.edges;

    // 1. 节点数在基线边界内
    let (lo, hi) = (
        case.baseline_metrics.min_nodes,
        case.baseline_metrics.max_nodes,
    );
    if !(nodes.len() >= lo && nodes.len() <= hi) {
        check.fail(format!(
            "node count {} outside baseline [{}, {}]",
            nodes.len(),
            lo,
            hi
        ));
    }

    // 2. 边引用的索引必须有效
    for e in edges {
        if e.len() != 2 || e[0] >= nodes.len() || e[1] >= nodes.len() {
            check.fail(format!("invalid edge {:?}", e));
        }
    }

    // 3. DAG 无环
    if !is_acyclic(nodes.len(), edges) {
        check.fail("expected_sop contains a cycle".into());
    }

    // 4. 节点名唯一
    let mut names: Vec<&str> = nodes.iter().map(|n| n.name.as_str()).collect();
    names.sort_unstable();
    let before = names.len();
    names.dedup();
    if names.len() != before {
        check.fail("duplicate node names in expected_sop".into());
    }

    // 5. 基线要求的节点种类存在
    let has = |k: &str| nodes.iter().any(|n| node_kind(n) == k);
    if case.baseline_metrics.expects_retry && !has("Retry") {
        check.fail("baseline expects a Retry node".into());
    }
    if case.baseline_metrics.expects_test && !has("Test") {
        check.fail("baseline expects a Test node".into());
    }
    if case.baseline_metrics.expects_branch && !has("Branch") {
        check.fail("baseline expects a Branch node".into());
    }

    check
}

// ── 主流程 ──────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cases_path = Path::new("evals/golden_sop/cases.json");
    let content = fs::read_to_string(cases_path)?;
    let cases: Vec<GoldenCase> = serde_json::from_str(&content)?;

    println!(
        "Loaded {} golden SOP seed cases from {}",
        cases.len(),
        cases_path.display()
    );

    // 库级一致性：ID 唯一、类别覆盖统计
    let mut seen = std::collections::HashSet::new();
    for c in &cases {
        if !seen.insert(&c.id) {
            anyhow::bail!("duplicate case id: {}", c.id);
        }
    }
    let mut by_category: HashMap<&str, usize> = HashMap::new();
    let mut by_difficulty: HashMap<&str, usize> = HashMap::new();
    for c in &cases {
        *by_category.entry(c.category.as_str()).or_default() += 1;
        *by_difficulty.entry(c.difficulty.as_str()).or_default() += 1;
    }

    // 逐例结构评测
    let checks: Vec<CaseCheck> = cases.iter().map(check_case).collect();
    let passed = checks.iter().filter(|c| c.passed).count();

    println!("\nCategory distribution:");
    let mut cats: Vec<_> = by_category.into_iter().collect();
    cats.sort_unstable();
    for (cat, n) in cats {
        println!("  {cat:<18} {n}");
    }
    println!("\nDifficulty distribution:");
    let mut diffs: Vec<_> = by_difficulty.into_iter().collect();
    diffs.sort_unstable();
    for (d, n) in diffs {
        println!("  {d:<10} {n}");
    }

    println!("\nStructural evaluation:");
    let mut failed = 0;
    for c in &checks {
        if c.passed {
            println!("  PASS {}", c.id);
        } else {
            failed += 1;
            println!("  FAIL {} — {}", c.id, c.failures.join("; "));
        }
    }

    println!(
        "\nGolden SOP library: {}/{} structural checks passed",
        passed,
        checks.len()
    );

    // P3-10 第二步：执行级评测数据通道。
    // 真实通过率/成本/迭代趋势需要真实合成管线接入，本步骤先建立 JSONL 历史记录，
    // 让 nightly 任务在结构检查后追加通过率/迭代数/耗时，便于趋势分析。
    // 写入 .sisyphus/eval-history/golden-sop-{timestamp}.jsonl
    let history_dir = std::path::Path::new(".sisyphus/eval-history");
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let history_path = history_dir.join(format!("golden-sop-{timestamp}.jsonl"));
    if let Err(e) = std::fs::create_dir_all(history_dir) {
        eprintln!("WARN: failed to create eval-history dir: {e}");
    } else {
        let per_category_pass: HashMap<&str, (usize, usize)> = {
            let mut map: HashMap<&str, (usize, usize)> = HashMap::new();
            for (i, c) in cases.iter().enumerate() {
                let entry = map.entry(c.category.as_str()).or_insert((0, 0));
                entry.1 += 1;
                if checks[i].passed {
                    entry.0 += 1;
                }
            }
            map
        };
        let per_difficulty_pass: HashMap<&str, (usize, usize)> = {
            let mut map: HashMap<&str, (usize, usize)> = HashMap::new();
            for (i, c) in cases.iter().enumerate() {
                let entry = map.entry(c.difficulty.as_str()).or_insert((0, 0));
                entry.1 += 1;
                if checks[i].passed {
                    entry.0 += 1;
                }
            }
            map
        };
        let record = serde_json::json!({
            "timestamp": timestamp,
            "total": checks.len(),
            "passed": passed,
            "failed": failed,
            "pass_rate": if checks.is_empty() { 0.0 } else { passed as f64 / checks.len() as f64 },
            "by_category": per_category_pass,
            "by_difficulty": per_difficulty_pass,
        });
        if let Ok(line) = serde_json::to_string(&record) {
            if let Err(e) = std::fs::write(&history_path, format!("{line}\n")) {
                eprintln!("WARN: failed to write eval history: {e}");
            } else {
                println!("\nHistory recorded: {}", history_path.display());
            }
        }
    }

    if failed > 0 {
        anyhow::bail!("{failed} golden SOP cases failed structural checks");
    }
    Ok(())
}
