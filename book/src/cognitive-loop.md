# Cognitive Loop

## 概述

认知循环（Cognitive Loop）是 Sisyphus 执行引擎的核心迭代过程。它遵循 **预测 → 执行 → 反思 → 演进**（predict → execute → reflect → evolve）的四步循环，持续优化 SOP 图的执行质量直至收敛。

## Cognitive Loop v2

从 2026 年 7 月起，认知循环引入了三个新组件以提升效率、鲁棒性和探索能力。

### AdaptiveDepthController

替代固定 `max_iterations` 上限，改用基于置信度（confidence）的自适应深度控制。

- 每次迭代记录置信度，在滑动窗口（默认 3 次）上计算收敛速度（convergence velocity）。
- 使用 `ComplexityAnalyzer` 根据图拓扑、目标复杂度和经验库计算初始迭代预算。
- 满足以下任一条件时停止：达到目标置信度（默认 0.95）、收敛停滞（窗口内改进量 < 0.01）、达到硬上限（默认 30 次）。
- 强制 `min_iterations`（默认 2）防止过早停止。

### ParallelBranchPredictor

同时模拟多个变体 SOP 图，并选择最佳分支执行。

- 对每个变体执行两类模拟：**执行模拟**（预测成功率、成本、稳定性）和**逻辑模拟**（检测逻辑缺陷）。
- 综合评分公式：`0.5 × success_probability + 0.3 × cost_efficiency + 0.2 × predicted_stability - logical_penalty`
- 通过 `ExperienceBank` 校准置信度，使历史成功数据影响分支选择。
- 默认：`max_branches = 3`，`concurrency = 3`。

### AnomalyDetector

在运行时监控每个节点的执行指标并标记异常。

- 跟踪三类指标：**燃料消耗**（>2.0x 基线）、**执行时长**（>3.0x 基线，>5.0x 为高严重性）、**错误率**（>30%）。
- 使用指数加权移动平均（EWMA，alpha = 0.3）进行基线追踪。
- 产生 `AnomalyReport`，其中的 `should_extra_reflect` 标志触发额外的反思循环。
- 集成于 `StrategicManager`，使循环能在执行过程中自我修正。

### 配置

```toml
[cognitive_loop]
max_iterations = 10
convergence_threshold = 0.15

[cognitive_loop.adaptive_depth]
initial_budget = 2.0
decay = 0.3
min_improvement = 0.15
convergence_threshold = 0.05
```

上述配置通过 `CognitiveLoopConfig` 结构体定义，位于 `crates/sisyphus-types/src/config/sub.rs`。
