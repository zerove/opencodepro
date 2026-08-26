# Golden SOP 库（P3-10）

## 目的

为 SOP 合成/执行质量提供可度量的基准。路线图要求 ≥50 个带标准答案的
golden SOP，nightly 跑通过率/成本/迭代数趋势。

## 当前状态（第一步：种子库 + 离线结构评测）

- `cases.json`：50 个种子用例，每例含
  - `goal`：目标文本（合成管线的输入）
  - `expected_sop`：标准答案的图结构（节点 kind + 边）
  - `baseline_metrics`：结构基线（节点数边界、期望含 Retry/Test/Branch）
  - `category` / `difficulty`：5 类 × 3 档分布
- `eval.rs`（binary `golden-sop-eval`）：离线结构评测器——不执行 SOP、不调 LLM，
  校验用例库一致性（ID 唯一、DAG 无环、边索引有效、名称唯一）并核算基线指标。
  已接入 nightly 工作流。

## 用例分布

| 类别 | 数量 | 说明 |
|------|------|------|
| file_ops/code_fix/refactor/deps/lint/fmt/git_ops/docs/build/perf | 12 | 文件与代码操作 |
| testing/ci_verify/benchmarking/sandbox/property_testing/api_contract/chaos/isolation/smoke/evolution_guard | 10 | 测试与验证 |
| memory_write/memory_read/synthesis/evaluation/reflection/observability/adaptive_depth/dedup | 8 | 认知与记忆 |
| service_ops/secrets/logging/interop/skill_market/data_sync/security_scan/panic_hygiene/dashboard/release | 10 | 运维与集成 |
| root_cause/benchmarking/telemetry_analysis/tracing/documentation/reporting/planning/cost_analysis/automation/self_healing | 10 | 分析综合自动化 |

## 嵌入召回率基准（P2-7 收尾）

`recall-benchmark` binary：同一语料（5 主题 × 3 变体 + 干扰项）上对比
两条检索路径的 Recall@5 与 MRR：

| 路径 | Recall@5 | MRR |
|------|----------|-----|
| DummyEmbeddingClient（哈希伪向量） | 0.333 | 0.476 |
| Jaccard 词法回退 | **0.800** | **1.000** |

结论：哈希正弦伪向量无语义信号；嵌入不可用时词法回退是更优降级路径，
P2-7 的降级策略得到数据验证。断言保证 jaccard ≥ dummy 不回归。

## 后续步骤（第二步起，待真实管线接入后实施）

1. **执行评测**：对每例 goal 走完整合成 → 执行 → 反思循环，记录
   通过率（最终 outcome_success）、成本（fuel_consumed 真值 + token）、迭代数。
2. **回归门禁**：`GoldenDataset::verify_no_regression` 已有 10% 退化阈值；
   执行评测落地后将 nightly 结果持久化，形成趋势序列。
3. **评分对接**：`HeuristicSopEvaluator` 的结构分可作为合成结果的快速预检，
   与本库的 `baseline_metrics` 对齐。

## 设计取舍

- 种子用例的标准答案是 **图结构约束** 而非唯一固定图：同一 goal 可有多种等价实现，
  基线只约束结构性必要条件（节点数范围、必须存在的容错/验证机制）。
- 结构评测与 agent 评测（`evals/runner.rs`）分离：前者零依赖可进 CI 门禁，
  后者需 LLM judge 只在夜间跑。
