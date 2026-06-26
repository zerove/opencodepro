# OpenCodePro 战略进化路线图 (Roadmap)

## 1. 当前状态分析 (Current State)

### 核心能力
- **执行层规划**: 已实现 `TeamGoal` $\rightarrow$ `TeamPlan (DAG)` $\rightarrow$ 并行执行的完整闭环。
- **自适应认知**: 通过 `Cognitive Loop` (预测-执行-反思-演化) 实现了单次执行的动态修正。
- **专业工具链**: 构建了基于 Agent 角色 (Metis, Momus, Scribe, Librarian) 的专业分析与合成能力。
- **持久化底座**: 通过 `MfsStore` 实现了 SOP 和执行状态的 KV 持久化。

### 关键缺失 (Capability Gap)
- **战略层缺失**: 缺乏从“项目”到“里程碑”再到“目标”的层级管理。
- **长线状态追踪**: 无法跨 Session 追踪一个长期项目的整体进度。
- **动态路线图调整**: 缺乏基于执行结果反向调整宏观计划的能力。

---

## 2. 进化目标 (Target State)
将 Sisyphus 从一个**“目标执行引擎” (Goal-Executor)** 进化为**“自主项目架构师” (Project Architect)**。

## 3. 分阶段实施计划

### 第一阶段：构建战略规划层 (Strategic Planning Layer)
**重点**: 建立宏观管理实体。
- [ ] **引入 Roadmap 数据模型**: 定义 `Project` $\rightarrow$ `Milestone` $\rightarrow$ `Phase` $\rightarrow$ `Goal` 的层级结构。
- [ ] **实现 `RoadmapManager`**: 负责路线图的创建、目标排序、依赖定义及持久化。
- [ ] **集成 MfsStore**: 将路线图状态持久化，支持跨 Session 的进度恢复。

### 第二阶段：增强状态感知与进度追踪 (Progress Awareness)
**重点**: 实现量化追踪与可见性。
- [ ] **里程碑追踪系统**: 自动计算各阶段完成度，支持进度可视化。
- [ ] **动态路线图调整**: 实现基于执行失败或结果偏差的“宏观重规划”触发机制。
- [ ] **项目级上下文合成**: 为每一个 Goal 提供基于路线图当前进度的全局上下文。

### 第三阶段：闭环演进与自主治理 (Autonomous Governance)
**重点**: 实现战略级的自我迭代。
- [ ] **战略反思循环**: 在 `Cognitive Loop` 之上构建 $\text{Roadmap-Reflection}$ 循环，反思整体路径的合理性。
T- **自主里程碑生成**: 根据项目初始需求，利用 LLM 自动建议最优的阶段划分与里程碑设定。
- [ ] **资源/成本全局优化**: 基于路线图预测整体资源消耗并优化目标执行顺序。

---

## 4. 核心模型定义 (Draft)
- **Project**: 项目根节点，包含名称、全局上下文及 `Roadmap`。
- **Milestone**: 关键交付点，包含目标集合及验收标准。
- **Phase**: 逻辑阶段，包含一组有序或部分有序的 `Goal`。
- **Goal**: 原子执行单位（由 `TeamOrchestrator` 处理）。
