# ADR-022: 认知循环 v3 问题陈述与候选方向（Problem Statement）

状态：**Proposed**（2026-11-22，问题陈述阶段；**不含决策**）

日期：2026-11-22
关联：`crates/sisyphus-core/src/cognitive_loop.rs`、`crates/sisyphus-core/src/strategic_manager/cognitive_loop.rs`、`crates/sisyphus-core/src/cognitive_loop_v2.rs`、TODO 9.2

## 背景

当前认知循环（v2，见 `CognitiveEngine::run` / `StrategicManager::run_cognitive_loop`）实现了
**predict → execute → reflect → evolve** 的迭代闭环，并已具备：

- `AdaptiveDepthController` + `AnomalyDetector`（`cognitive_loop_v2.rs`）的深度/异常自适应；
- 基于 `ComplexityAnalyzer` 的模拟跳过与深度估计；
- `max_iterations` / `convergence_threshold` 的用户上限包络（有效值由 adaptive budget 与用户配置共同裁决）；
- 反思/演进阶段的 LLM 调用与 `record_llm_stage_usage` 成本记录；
- MicroReflector 抽象与认知事件流（TUI/SSE）。

v2 在单机、默认配置和现有评测集上可用，但随着 SOP 规模、上下文长度、长会话经验积累增长，
出现了若干**结构性张力**。本 ADR 只记录问题与约束，不选择方案。

## 问题陈述

### P1：token / 上下文预算是粗粒度的，无法按阶段最优分配

- 反思阶段把执行轨迹拼成字符串传入 `reflect_on_trace`，并以轨迹字节数近似记录输入侧成本（`record_llm_stage_usage(Some(trace.len()))`）。
- 预测、反思、演进三个 LLM 阶段共享同一循环预算语义，但没有“本轮该把多少 token 给 predict/reflect/evolve”的显式分配策略。
- 结果：复杂 SOP 可能在早期迭代耗尽有效上下文/预算，导致后期反思深度下降或演进信号质量退化；简单 SOP 又可能过度分配。

**待回答的问题**：预算应按阶段静态分配、按迭代动态分配、还是按偏差/异常信号自适应分配？预算单位是 token、字节、调用次数还是成本金额？

### P2：反思输入是线性 trace 摘要，难以承载大图结构

- 当前反思以节点名与输出的线性拼接作为 trace 主体。对小图足够，但对 >100 节点 SOP、并行分支、失败重试路径，线性 trace 会丢失结构信息或膨胀上下文。
- `CognitiveReport` 记录迭代历史，但反思主要消费当前轨迹而非结构化跨迭代记忆。

**待回答的问题**：反思应消费原始 trace、结构化图摘要、还是检索式证据包？如何在不超过预算的前提下保留因果链？

### P3：经验检索与 SOP 结构语义未统一

- 经验库（sisyphus-mem）与 SOP 图执行目前是相邻系统：经验可用于复杂度/提示，但 SOP 节点本身没有统一的向量语义表示。
- 相似 SOP/子图复用、跨项目经验迁移、节点级策略推荐缺少共同的 embedding 基底。

**待回答的问题**：是否需要 SOP 节点/子图 embedding？embedding 的对象是节点、边、子图还是完整 SOP？如何评估检索质量而非只看相似度？

### P4：演进信号与执行证据的因果归因较弱

- 反思产出 `suggested_improvements` / `improvement_signals` / 可能的 `evolved_sop_structure`；演进阶段应用后进入下一迭代。
- 当演进失败或带来回归时，难以把“哪条证据导致哪个图变更”做细粒度归因，也难以做跨会话的演进效果审计。

**待回答的问题**：演进是否应产出可审计的 patch/intent 对象？回滚单位是整图、子图还是单条 mutation？

### P5：长会话/跨会话的认知状态缺少压缩策略

- 迭代历史、trace、事件流在单次循环内可管理；跨会话经验增长后，检索和提示注入会面临噪声与成本问题。
- 现有 memory/experience 机制已有存储能力，但“哪些认知状态应压缩、哪些应保留原样”缺少明确策略。

**待回答的问题**：压缩对象是反思记录、经验元组、SOP 演进历史还是事件流？压缩后的可追溯性如何保证？

## 约束

- **不破坏 v2 行为默认值**：任何 v3 实验应在 feature flag / 独立路径后启用，默认单机行为不变。
- **可观测性优先**：新策略必须能解释“为什么分配了这些预算 / 为什么检索了这些经验 / 为什么演进了这个节点”。
- **可回滚**：SOP 演进必须有回滚/拒绝路径；不能把不可解释的图变更直接写入生产 SOP。
- **成本可计量**：token/调用次数/时延/质量必须能被 benchmark 或 eval 观测，不能只靠主观判断。
- **不引入隐式网络依赖**：单机离线/Mock 环境下仍可运行核心路径（与 evals-ci 稳定集原则一致）。

## 候选方向（仅候选，非决策）

| 方向 | 可能解决 | 主要风险 | 验证方式 |
|------|----------|----------|----------|
| SOP 节点/子图 embedding | P3, 部分 P2 | 嵌入质量不稳；维护成本；检索噪声 | 与现有 experience bank A/B，对检索命中率/任务成功率做对比 |
| 自适应 token 预算分配 | P1 | 预算控制复杂；可能饿死关键阶段 | 固定预算下的成功率、收敛迭代数、成本回归基准 |
| 结构化反思证据包（graph-aware trace） | P2, P4 | 摘要丢信息；实现复杂 | 大图 SOP 回归集 + 反思质量评测 |
| 演进 patch/intent 审计对象 | P4 | schema 演进成本；兼容性 | 回放/回滚测试、演进审计日志 |
| 长期记忆压缩与分层检索 | P5 | 压缩失真；召回下降 | 长会话 benchmark、检索召回/精度、成本对比 |

## 不在本 ADR 中决定

- 不选择 v3 的最终架构。
- 不承诺 embedding 模型、向量库或检索算法。
- 不修改现有认知循环默认行为。
- 不定义 v3 的发布日期或版本号。

## 下一步（进入决策前必须完成）

1. **基线测量**：为典型 SOP（小/中/大图）采集迭代数、阶段耗时、token 成本、反思触发率、演进接受率。
2. **问题复现集**：构造能稳定暴露 P1-P5 的 eval/benchmark cases（Mock 可跑，真实 LLM 可选）。
3. **方案草图**：每个候选方向写 1 页 design sketch，含接口、数据流、回滚策略、指标。
4. **ADR-022 决策更新**：在证据齐备后，将本 ADR 从 Proposed 更新为 Accepted/Rejected，并拆分实施 ADR。

## 验收标准（问题陈述阶段）

- [x] 问题陈述明确区分“现象、约束、候选方向、未决事项”。
- [x] 每个问题可映射到代码位置或可观测信号。
- [x] 未做架构决策，不引入默认行为变更。
