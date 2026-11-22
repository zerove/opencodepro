# ADR-022 Workstream 1: SOP 节点嵌入实验设计草图（Proposed）

状态：**Proposed**（2026-11-22，实验设计草图；**非决策**）

目标：为 `cognitive_loop.rs:predict_phase` 的「模拟路径选择」或未来「从 experience bank 检索相似 SOP/子图」预演代价；评估对象与 ADR-022.P3 一致：**检索命中率与任务成功率对比（A/B）**。

关联：`docs/adr/022-cognitive-loop-v3-problem-statement.md`、`crates/sisyphus-types/src/sop_graph/mod.rs`、`crates/sisyphus-core/src/strategic_manager/cognitive_loop.rs`。

## 1. 实验目标

- **复现/区分当前张力**：复杂 SOP（>100 节点）图，「输入 = 节点名 + 输出历史」在线性 space 里歧义；与 10.3 Memory checkpoint 的拓扑共存时，到底哪个身份维度优先？
- **目标条件**（待实测后写入 ADR 决策）：任务级成功率统计学提升（p<0.05）或 `hit@5` 同类启发式 +10% 以上。

## 2. 实验设置

### 2.1 数据源
- **导出源头**：`crates/sisyphus-roadmap` 的 `sop_graphs/export_to_json` 复用，统一 canonical JSON（不 fork）。
- **综合**：生成环 + 树/有界宽的 DAG（15-200 节点）；不引入网络/LLM，保证可重复、零网络。

### 2.2 候选编码器（待核对后加）
| 名称 | 类型 | 接口（待核对） |
|------|------|----------------|
| `NodeNameHashDense` | 哈希向量 | `name -> u8[128]` via blake3，lower/无空格/无数字种子不破坏相同词？ |
| `TFIDF-BSR` | 稀疏向量 | 先词频（增量词表），零长尾 |
| `Latent (节点/骰子/隐式边)` | 学习向量 | `d=64`，初始每骰子置 1.0，需按时间组；结构向量用一致的 edge/效率归一化 |
| `OneHot` | 独热向量 | 全局节点字典 |

**输出接口（待核对）**：
```rust
pub trait SopEmbedder {
    fn embed_name(&self, name: &str) -> Vec<f32>;
    fn embed_node(&self, node: &SopNode) -> Vec<f32>;
}
```

### 2.3 检索（待核对）
- 复用 `crates/sisyphus-mem::VectorExperienceStore::query_text / query_hash`，不引入新通道；top1-3 条摘要；缩容：`limit` 可评分。

### 2.4 实验矩阵
| 维度 | 取值 |
|------|------|
| 编码器 | 0（无语义）vs N（实现子集） |
| 相似度 | cosine（已有）vs 新结构相似度 |
| 指标 | `hit@5`（ground truth 位置 ∈ top5）+ `mAP@K` |

## 3. 地面真相与评测

### 3.1 Ground Truth 生成（实验基础，待执行）
- 对每条查询样本生成一组「应相似」的经验行：建模两类边界情况——低概率/噪声（`relevance ~ 0.1`）与高概率/模式化（`relevance ~ 0.9`），这些行由 **不同 origin/时间** 写入。
- 检索口径：相似 SOP（≥ 2 个同构前缀或共享结构组件）。不使用 LLM。

### 3.2 评测基础设施（落地规划）
- 命中计数：`hit@5 = 命中 ground-truth 内 top5 / 查询数`。
- 时序耗时：embed 与检索成本的百分位数。
- **不使用真实 LLM**（Mock 替代）。

## 4. 决策问题（ADR-022 决策分派，待证据）

1. 编码器先验化依赖多少是**显式 SOP 语义**（人工标注/当前 experience bank 实现）vs **学习**？当前工程倾向：学习一阶（latent）+ `NodeNameHashDense` 兜底。
2. 结构信号优先级：`edge 效率归一化` vs `节点名按 hit pattern 计数`。倾向：结构归一化（边沿+无宽工具变体）；名称 hash 仅排序、权重 0.3。
3. `predict_phase` 是否要在 `LinearSpace` 模拟路径前再加「少量相似 SOP 提示」？倾向：存在表征冲突时放，完全没有聊 `LinearSpace` 单位问题。
4. checkpoint（10.3）与本次 embedding 是否要统一「节点身份与 embedding 的同步滞后处理」？倾向：滞后到 10.3 合并后按 ADR-010 反熵口径兜底。

## 5. 不在本草图中

- 不选择最终编码器。
- 不定义索引/向量库/向量检索服务。
- 不改变 `predict_phase` 模拟语义（feature flag 后置）。
- 不要求 SOP 提取（提供者：roadmap 规划）。
- 不讨论长期：网络服务、向量降维合并策略、对齐维度（由实验数据后来定）。

## 6. 实施出口（路线图）

1. ✅ `docs/plans/adr022-sop-embedding-experiment.md` 实验草图（本文件）。
2. ⏳ `docs/plans/adr022-sop-embedding-experiment-design.md`：一页 design sketch（接口与默认形态）——当前接口/默认形态已并入本文件 §2.2/§2.3，独立一页待接入 `VectorExperienceStore` 时再写。
3. ✅ `docs/plans/adr022-sop-embedding-experiment-benchmark.md`：编码器/检索基准 + 实测数据（2026-11-22；eval 与 Rust bench 一致）。
4. ⏳ ADR-022 更新（可选 ADR-022B）：实验结论与接入语义演进——合成数据只验证管线，语义泛化证据不足，保持 Proposed。
5. ⏳ TODO 进阶跟踪：`TODO.md` 9.2.1 链接的草图（时间线：实验（1-2 周）+ 执行出口（需 ADR 决策））。
