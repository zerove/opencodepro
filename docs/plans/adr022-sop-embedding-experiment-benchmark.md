# ADR-022 Workstream 1: SOP 嵌入匹配实测基线

状态：**Proposed**（2026-11-22，实验数据；**非决策**）

关联：[ADR-022 问题陈述](../adr/022-cognitive-loop-v3-problem-statement.md)、
[实验草图](adr022-sop-embedding-experiment.md)、
`evals/ground_truth_gen.py`、`evals/sop_embed_eval.py`、`evals/sop_embed_bench.rs`。

本文件是草图 §6 出口第 3 项（编码器/检索基准 + 实测数据）。它只证明
「编码 → 检索 → 指标」管线可复现，不构成编码器选择或 ADR 决策。

## 1. 数据

- `evals/ground_truth_42.json`：9 DAG / 90 queries。
  - 8 个合成 A/B DAG（`sop-000..007`），每个 10 条 query，共 80 条；
  - 1 个 Library backwards case（`lib-case-backwards`，10 个确定性节点名），10 条 query。
- 8-DAG A/B 子集：`python3 evals/ground_truth_gen.py --seed 42 --dags 8 --queries 10 --out /tmp/gt80.json`
  （80 queries，语料仅 8 个 DAG）。
- 全部数据由本地确定性 RNG 生成，零网络、零 LLM；查询名与语料节点名同名，
  `pattern_set` 额外携带 near/far/deep 干扰词（当前指标只使用查询名）。

## 2. 编码器与检索

| codec | 维度 | 实现 |
|-------|------|------|
| `none` | 1 | 恒 `[0.0]`，无语义基线 |
| `name_hash` | 8 | FNV-1a 64 位，按字节切片 / 255 |
| `name_hash_dense16` | 128 | FNV-1a 64 位 → 16 字节 → 128 维投影，缩放到 `[-4, 4]` |

检索：对全部 DAG 节点名去重、排序，按 cosine 取 top-5。单条 ground truth
（查询名自身），命中即 `hit@5`；`mAP@5 = mean(1/rank)`（未命中记 0）。

## 3. 结果

2026-11-22 实跑，Python eval 与 Rust bench 输出逐位一致：

| 数据集 | codec | hit@5 | mAP@5 |
|--------|-------|------:|------:|
| 90 query（全量） | `none` | 0.1111 | 0.0602 |
| 90 query（全量） | `name_hash` | 1.0000 | 1.0000 |
| 90 query（全量） | `name_hash_dense16` | 1.0000 | 1.0000 |
| 80 query（8-DAG A/B） | `none` | 0.1250 | 0.0677 |
| 80 query（8-DAG A/B） | `name_hash` | 1.0000 | 1.0000 |
| 80 query（8-DAG A/B） | `name_hash_dense16` | 1.0000 | 1.0000 |
| 10 query（Library case） | `none` | 0.0000 | 0.0000 |
| 10 query（Library case） | `name_hash` | 1.0000 | 1.0000 |
| 10 query（Library case） | `name_hash_dense16` | 1.0000 | 1.0000 |

## 4. 解释与限制

- 哈希编码在合成数据上 1.0 是**构造使然**：查询名与语料节点名完全相同，
  精确匹配的余弦相似度为 1；这只验证管线正确，**不证明语义泛化**。
- `none` 基线反映「无信息时按字典序碰运气」；命中来自排序靠前的名称，
  与节点语义无关。
- 要回答草图 §1 的目标条件（`hit@5` 相对提升 ≥10% 或任务成功率显著提升），
  还需要：查询改写/同义节点名、结构相似度、held-out DAG，以及接入
  `VectorExperienceStore` 后的真实经验检索。这些仍待做。
- 因此 ADR-022 保持 **Proposed**；本基线只满足出口 §6.3，不触发编码器决策。

## 5. 复现

```bash
# 全量 90 query（默认 name_hash）
make sop-embed-eval

# 无语义基线
make sop-embed-eval SOP_CODEC=none

# Rust bench（与 Python eval 应一致）
cargo run -q -p eval-runner --bin sop_embed_bench -- --dag-file evals/ground_truth_42.json

# Library backwards case（只过滤 query，语料保留全部节点作为干扰项）
cargo run -q -p eval-runner --bin sop_embed_bench -- \
  --dag-file evals/ground_truth_42.json --case lib-case-backwards

# 8-DAG A/B 子集
python3 evals/ground_truth_gen.py --seed 42 --dags 8 --queries 10 --out /tmp/gt80.json
python3 evals/sop_embed_eval.py --truth /tmp/gt80.json --codec none --k 5
```

## 6. 同日修复的评测缺陷

- Python `mAP` 曾把 rank 当 AP（rank=2 记 2.0，mAP 可 >1）；改为 `1/rank`，
  与 Rust bench 口径一致。修正后 `none` 的 mAP 从 0.2778 降到 0.0602。
- Python `make_vectors` 的 `name_hash` 分支被 `name_hash_dense16` 提前
  `return` 遮蔽；移除后 `name_hash` 恢复 8 维实现。
- Rust bench 的 `--dag-file` / `--case` 解析与文档不符（路径被当作位置参数，
  文件读失败被 `unwrap_or` 吞掉，输出 NaN）；改为真实参数解析，载入失败
  以非零码退出，`--case` 与 Python eval 一样只过滤 query。
- `ground_truth_gen.py` 的 `--queries` 被忽略、未开 `--backwards-case` 时
  仍追加 Library 查询（8-DAG 子集因此引用不存在的节点）；两处均已修复。
