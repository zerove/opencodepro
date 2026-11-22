//! ADR-022 Workstream 1: SOP 嵌入匹配的 bench（§9.3.3，§5.2）。
//!
//! - 运行地面真相：`evals/ground_truth_42.json`（体量 ≈ 8 DAG / 80 queries）。
//! - 编码器：`name_hash` / `name_hash_dense16` / `none`（§2.2 接口：name -> Vec<f32>）。
//! - 评测：`evals/sop_embed_eval.py`（§3.2 hit@5/mAP@5）。
//!
//! 用法（§6）：
//! ```bash
//! cargo run -p eval-runner --bin sop_embed_bench -- --dag-file evals/ground_truth_42.json
//! ```
//!
//! 现状：编码器 = 本地简约实现（name hash）。接入现有系统路径（§5.2，参考 `cognitive_loop.rs:predict_phase`、`VectorExperienceStore`）待实现。

use serde_json::Value;

/// `SopEmbedder` 接口（§2.2 输出接口）：name -> embedding vector。
#[derive(Clone, Copy, PartialEq)]
pub enum Codec {
    None,
    NameHash,
    NameHashDense16,
}

impl Codec {
    pub fn as_str(&self) -> &'static str {
        match self {
            Codec::None => "none",
            Codec::NameHash => "name_hash",
            Codec::NameHashDense16 => "name_hash_dense16",
        }
    }
}

pub struct SopEmbedder {
    pub codec: Codec,
}

impl SopEmbedder {
    pub fn new(codec: Codec) -> Self {
        Self { codec }
    }

    /// 按 codec 编码一个 name（§2.2：`embed_name`）。
    pub fn embed_name(&self, name: &str) -> Vec<f32> {
        let nm = name.to_lowercase();
        match self.codec {
            Codec::None => vec![0.0],
            Codec::NameHash => (0..8)
                .map(|i| {
                    let shift = (8 * i).min(63);
                    let b = ((fnv1a64(nm.as_bytes()) >> shift) & 0xFF) as f32;
                    if 8 * i >= 64 {
                        0.0
                    } else {
                        b / 255.0
                    }
                })
                .collect(),
            Codec::NameHashDense16 => {
                let h = fnv1a64(nm.as_bytes());
                let mut dense = vec![0.0f32; 128];
                for i in 0..16usize {
                    let shift = (8 * i).min(63);
                    let b = ((h >> shift) & 0xFF) as f32;
                    dense[(i * 8) % 128] += (b - 127.5) / 127.5;
                }
                // 缩放到相似范围，避免 dense16 正常化异常
                dense.iter_mut().for_each(|v| *v *= 4.0);
                dense
            },
        }
    }
}

/// FNV-1a 64-bit（确定性，与 `evals/sop_embed_eval.py::make_vectors` 一致）。
fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let na = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>() / (na * nb)
}

/// 最小测评（§3.2）：hit@5 + mAP@5（无脚本换乘，减少依赖面）。
fn eval_report(codec: Codec, dags: &[&[String]], queries: &[String]) -> (f64, f64) {
    let ed = SopEmbedder::new(codec);
    let mut names: Vec<String> = dags.iter().flat_map(|d| d.iter().cloned()).collect();
    names.sort();
    names.dedup();
    let mut hits = 0u32;
    let mut ap_sum = 0.0f64;
    for q in queries {
        let qv = ed.embed_name(q);
        let mut sims: Vec<(String, f32)> =
            names.iter().map(|n| (n.clone(), cosine(&qv, &ed.embed_name(n)))).collect();
        sims.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let topk = sims.iter().take(5).map(|(n, _)| n.clone()).collect::<Vec<_>>();
        if topk.contains(q) {
            hits += 1;
            let rank = topk.iter().position(|n| n == q).unwrap_or(5) + 1;
            if rank == 1 {
                ap_sum += 1.0;
            } else if rank <= 5 {
                ap_sum += 1.0 / rank as f64;
            }
        }
    }
    (
        hits as f64 / queries.len() as f64,
        ap_sum / queries.len() as f64,
    )
}

fn load_fixture(path: &str) -> Result<(Vec<Vec<String>>, Vec<String>), String> {
    let txt = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(&txt).map_err(|e| e.to_string())?;
    let mut dags = Vec::new();
    for d in v["dags"].as_array().ok_or("no dags")? {
        let names = d["node_names"]
            .as_array()
            .ok_or("no nodes")?
            .iter()
            .map(|n| n.as_str().unwrap_or("").to_string())
            .collect();
        dags.push(names);
    }
    let queries = v["queries"]
        .as_array()
        .ok_or("no queries")?
        .iter()
        .map(|q| q["name"].as_str().unwrap_or("").to_string())
        .collect();
    Ok((dags, queries))
}

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "evals/ground_truth_42.json".to_string());
    let case_filter: Option<String> = std::env::args().nth(2);
    let (dags, queries) = load_fixture(&path).unwrap_or((vec![], vec![]));
    let (dags, queries) = if let Some(case_id) = case_filter {
        // --case <id>：只评测指定 DAG（如 lib-case-backwards）
        let filtered_dags: Vec<Vec<String>> =
            dags.iter().filter(|d| d[0].contains(&case_id)).cloned().collect();
        let filtered_queries: Vec<String> = queries
            .iter()
            .filter(|q| {
                q.starts_with(&format!("q-{}", case_id))
                    || filtered_dags.iter().any(|d| d.contains(q))
            })
            .cloned()
            .collect();
        (filtered_dags, filtered_queries)
    } else {
        (dags, queries)
    };

    // 基线 codec（§2.2）: none/name_hash/name_hash_dense16
    let dags_refs: Vec<&[String]> = dags.iter().map(|d| d.as_slice()).collect();
    for codec in [Codec::None, Codec::NameHash, Codec::NameHashDense16] {
        let (hits, ap) = eval_report(codec, &dags_refs, &queries);
        println!(
            "bench: codec={} hit@5={:.4} mAP@5={:.4}",
            codec.as_str(),
            hits,
            ap
        );
    }

    // Library backwards case（--case lib-case-backwards：通过 arg[2] 过滤）
    if let Some(id) = std::env::args().nth(2) {
        let filtered: Vec<Vec<String>> =
            dags.iter().filter(|d| d[0].contains(&id)).cloned().collect();
        if let Some(dag) = filtered.first() {
            let names = dag.clone();
            for codec in [Codec::None, Codec::NameHash, Codec::NameHashDense16] {
                let (hits, ap) = eval_report(codec, &[names.as_slice()], &queries);
                println!(
                    "bench-lib: codec={} hit@5={:.4} mAP@5={:.4}",
                    codec.as_str(),
                    hits,
                    ap
                );
            }
        }
    }
}
