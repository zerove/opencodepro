//! 嵌入召回率基准（P2-7 收尾）：伪嵌入（DummyEmbeddingClient，哈希正弦向量）
//! vs 词法回退（Jaccard 词元重合度）在同一经验库上的召回质量对比。
//!
//! 背景：bridge 在非 openai provider 下静默降级到 DummyEmbeddingClient，
//! 其 128 维哈希正弦向量无语义区分度。P2-7 已为 embed 失败路径加了 Jaccard
//! 词法回退；本基准回答"词法路径是否优于伪向量路径"，为降级策略提供数据。
//!
//! 方法：构造带已知相关性的经验集（查询-文档对共享主题词元），
//! 分别走两条检索路径，计算 top-k 的 Recall@k 与 MRR（平均倒数排名）。
//! 纯离线（sqlite::memory:），可进 CI。

use sisyphus_mem::experience_bank::jaccard_similarity;
use sisyphus_tools_llm::{DummyEmbeddingClient, EmbeddingClient};

/// 一条查询及其已知相关的经验文本集合（按主题构造）。
struct QueryCase {
    query: &'static str,
    /// 相关经验的下标（相对于 corpus）。
    relevant: &'static [usize],
}

/// 语料：5 个主题 × 每主题若干变体 + 干扰项。
/// 主题间词元重叠刻意压低，相关性由主题词元决定。
const CORPUS: &[&str] = &[
    // 主题 A：部署
    "deploy the service to production with zero downtime",
    "deployment pipeline rolls out the service gradually",
    "rollback the deployed service after a failed release",
    // 主题 B：数据库迁移
    "database migration adds an index on the users table",
    "run the pending database migrations before deploy",
    "migration rollback restores the previous schema",
    // 主题 C：认证与安全
    "rotate the authentication api keys for the oauth provider",
    "authentication middleware rejects expired tokens",
    "security audit finds hardcoded credentials in config",
    // 主题 D：性能调优
    "profile the hot loop and reduce allocation pressure",
    "latency regression traced to a missing database index",
    "cache hit rate improved after tuning eviction policy",
    // 主题 E：测试
    "flaky integration test fails intermittently in ci",
    "property test shrinks the failing input to minimal case",
    "test coverage report shows untested error paths",
    // 干扰项：跨主题噪声（与任何查询都不完全同题）
    "update the changelog and bump the patch version",
    "weekly digest of pull requests merged by the team",
];

/// 查询用例：query 及其语料中的相关条目。
const QUERY_CASES: &[QueryCase] = &[
    QueryCase {
        query: "how to deploy the service safely",
        relevant: &[0, 1, 2],
    },
    QueryCase {
        query: "database migration failed and needs recovery",
        relevant: &[3, 4, 5],
    },
    QueryCase {
        query: "authentication tokens are being rejected",
        relevant: &[6, 7, 8],
    },
    QueryCase {
        query: "reduce latency caused by allocation in the hot path",
        relevant: &[9, 10, 11],
    },
    QueryCase {
        query: "integration test is flaky in ci runs",
        relevant: &[12, 13, 14],
    },
];

const TOP_K: usize = 5;

// ── 路径一：伪向量检索 ──────────────────────────────────────

async fn dummy_vector_ranking(query: &str) -> Vec<usize> {
    let client = DummyEmbeddingClient;
    let qv = client
        .embed(query)
        .await
        .expect("dummy embed must not fail");

    let doc_vecs: Vec<Vec<f32>> = {
        let mut v = Vec::with_capacity(CORPUS.len());
        for doc in CORPUS {
            v.push(client.embed(doc).await.expect("dummy embed must not fail"));
        }
        v
    };

    let mut scored: Vec<(f32, usize)> = doc_vecs
        .into_iter()
        .enumerate()
        .map(|(i, dv)| {
            // 余弦相似度（与 VectorMemoryStore.search_similar 同度量）
            let dot: f32 = qv.iter().zip(dv.iter()).map(|(a, b)| a * b).sum();
            let nq: f32 = qv.iter().map(|x| x * x).sum::<f32>().sqrt();
            let nd: f32 = dv.iter().map(|x| x * x).sum::<f32>().sqrt();
            (
                if nq > 0.0 && nd > 0.0 {
                    dot / (nq * nd)
                } else {
                    0.0
                },
                i,
            )
        })
        .collect();

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.into_iter().map(|(_, i)| i).collect()
}

// ── 路径二：Jaccard 词法检索 ────────────────────────────────

fn jaccard_ranking(query: &str) -> Vec<usize> {
    let mut scored: Vec<(f64, usize)> = CORPUS
        .iter()
        .enumerate()
        .map(|(i, doc)| (jaccard_similarity(query, doc), i))
        .filter(|(s, _)| *s > 0.0)
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.into_iter().map(|(_, i)| i).collect()
}

// ── 度量 ───────────────────────────────────────────────────

/// Recall@k：top-k 中命中的相关文档比例。
fn recall_at_k(ranking: &[usize], relevant: &[usize], k: usize) -> f64 {
    if relevant.is_empty() {
        return 0.0;
    }
    let hits = ranking
        .iter()
        .take(k)
        .filter(|i| relevant.contains(i))
        .count();
    hits as f64 / relevant.len() as f64
}

/// MRR：第一个相关结果排名倒数的平均值。
fn mrr(ranking: &[usize], relevant: &[usize]) -> f64 {
    for (rank, i) in ranking.iter().enumerate() {
        if relevant.contains(i) {
            return 1.0 / (rank as f64 + 1.0);
        }
    }
    0.0
}

#[tokio::main]
async fn main() {
    println!(
        "Embedding recall benchmark: DummyEmbeddingClient (hash pseudo-vectors)\n\
         vs lexical fallback (Jaccard token overlap). Corpus: {} docs, {} queries, top-{}.\n",
        CORPUS.len(),
        QUERY_CASES.len(),
        TOP_K
    );

    let mut dummy_recalls = Vec::new();
    let mut jaccard_recalls = Vec::new();
    let mut dummy_mrrs = Vec::new();
    let mut jaccard_mrrs = Vec::new();

    println!(
        "{:<48} | {:>10} {:>6} | {:>10} {:>6}",
        "query", "dummy R@k", "MRR", "jaccard R@k", "MRR"
    );
    println!("{}", "-".repeat(92));

    for case in QUERY_CASES {
        let d_rank = dummy_vector_ranking(case.query).await;
        let j_rank = jaccard_ranking(case.query);

        let dr = recall_at_k(&d_rank, case.relevant, TOP_K);
        let jr = recall_at_k(&j_rank, case.relevant, TOP_K);
        let dm = mrr(&d_rank, case.relevant);
        let jm = mrr(&j_rank, case.relevant);

        dummy_recalls.push(dr);
        jaccard_recalls.push(jr);
        dummy_mrrs.push(dm);
        jaccard_mrrs.push(jm);

        println!(
            "{:<48} | {:>10.3} {:>6.3} | {:>10.3} {:>6.3}",
            truncate(case.query, 46),
            dr,
            dm,
            jr,
            jm
        );
    }

    let avg = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    println!("{}", "-".repeat(92));
    println!(
        "{:<48} | {:>10.3} {:>6.3} | {:>10.3} {:>6.3}",
        format!("AVERAGE (top-{TOP_K})"),
        avg(&dummy_recalls),
        avg(&dummy_mrrs),
        avg(&jaccard_recalls),
        avg(&jaccard_mrrs)
    );

    let d_avg = avg(&dummy_recalls);
    let j_avg = avg(&jaccard_recalls);

    println!("\nConclusion:");
    if j_avg > d_avg {
        println!(
            "  Lexical fallback (Recall@{TOP_K}={j_avg:.3}) OUTPERFORMS pseudo-vector retrieval ({d_avg:.3}).\n  Hash-based pseudo-vectors carry no semantic signal; Jaccard fallback should be\n  preferred whenever real embeddings are unavailable."
        );
    } else if j_avg < d_avg {
        println!(
            "  Pseudo-vector retrieval outperforms lexical fallback ({d_avg:.3} vs {j_avg:.3})."
        );
    } else {
        println!("  Both paths perform equivalently at Recall@{TOP_K}.");
    }

    // 断言：词法回退必须显著优于伪向量（这是 P2-7 降级策略的验收标准）
    assert!(
        j_avg >= d_avg,
        "lexical fallback must not be worse than pseudo-vector retrieval: jaccard={j_avg:.3}, dummy={d_avg:.3}"
    );
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n - 1).collect::<String>())
    }
}
