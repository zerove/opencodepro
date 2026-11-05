# Sisyphus — 认知计算引擎

Sisyphus 是一个**认知计算引擎**，用于构建能够通过递归 **观察–判断–决策–行动**（observe–orient–decide–act，OODA）循环来规划、执行并改进自身行为的自主 AI 代理。

## 功能概述

Sisyphus 的核心是接收一个高层目标（以自然语言表达），然后：

1. **合成**（Synthesize）标准操作程序（Standard Operating Procedure，SOP）—— 一个由可执行子步骤组成的有向无环图（directed acyclic graph，DAG）。
2. **执行**（Execute）SOP，在可用节点上按依赖关系并行运行。
3. **模拟**（Simulate）通过蒙特卡洛树搜索评估结果，预测成功概率。
4. **反思**（Reflect）执行结果，计算预测与实际结果之间的*偏差分数*（deviation score）。
5. **演进**（Evolve）SOP，重试失败节点、增加燃料预算，或重构图结构。

该循环持续进行，直到目标收敛（偏差低于阈值）或达到最大迭代次数。

## 为什么选择 Sisyphus？

- **自我改进**：引擎从每次执行中学习，无需人工干预即可演进其 SOP。
- **设计即安全**：工具代码在 WASM 沙箱中执行，带燃料上限与内存边界。
- **可观测**：每个认知操作都内置 Prometheus 指标。
- **可组合**：每个 crate 相互独立，可按需取用。

## 项目状态

Sisyphus 处于**积极开发**中（v0.3.0）。核心认知循环已可用并通过测试，具备 WASM 工具执行、多级缓存与 Prometheus 指标集成。

## 下一步

- 阅读[架构总览](architecture.md)了解高层系统设计。
- 深入[核心概念](concepts.md)理解 SOP、认知循环与 DAG 执行。
- 查看 [Crate 参考](crates.md)获取详细 API 文档。