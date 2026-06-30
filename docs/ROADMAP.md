# OpenCodePro 战略进化路线图 (Roadmap)

> 状态：✅ **全部阶段已完成**。以下路线图中的所有规划功能均已实现。
> 最后更新：2026-06-27

## 1. 当前状态分析 (Current State)

### 核心能力
- **执行层规划**: 已实现 `TeamGoal` → `TeamPlan (DAG)` → 并行执行的完整闭环。
- **自适应认知**: 通过 `Cognitive Loop` (预测-执行-反思-演化) 实现了单次执行的动态修正。
- **专业工具链**: 构建了基于 Agent 角色 (Metis, Momus, Scribe, Librarian, Oracle, Explore) 的专业分析与合成能力。
- **持久化底座**: 通过 `MfsStore` 实现了 SOP 和执行状态的 KV 持久化。
- **战略规划**: 完整的 Roadmap 数据模型 (Project → Milestone → Phase → Goal) 及自动化治理。
- **CLI/TUI**: 基于 clap + ratatui 的交互式命令行界面。

### 已解决的关键缺失
- ✅ **战略层**: 已实现从"项目"到"里程碑"再到"目标"的层级管理（`sisyphus-roadmap`）。
- ✅ **长线状态追踪**: 跨 Session 追踪长期项目整体进度（`RoadmapManager` + `MfsStore`）。
- ✅ **动态路线图调整**: 基于执行结果反向调整宏观计划的能力（`apply_blocking`、`strategic_reflection`）。

---

## 2. 进化目标 (Target State)
将 Sisyphus 从一个**"目标执行引擎" (Goal-Executor)** 进化为**"自主项目架构师" (Project Architect)**。

✅ 目标已达成。

## 3. 分阶段实施计划

### 第一阶段：构建战略规划层 (Strategic Planning Layer)
**重点**: 建立宏观管理实体。
- [x] **引入 Roadmap 数据模型**: 定义 `Project` → `Milestone` → `Phase` → `Goal` 的层级结构。
- [x] **实现 `RoadmapManager`**: 负责路线图的创建、目标排序、依赖定义及持久化。
- [x] **集成 MfsStore**: 将路线图状态持久化，支持跨 Session 的进度恢复。

### 第二阶段：增强状态感知与进度追踪 (Progress Awareness)
**重点**: 实现量化追踪与可见性。
- [x] **里程碑追踪系统**: 自动计算各阶段完成度，支持进度可视化。
- [x] **动态路线图调整**: 实现基于执行失败或结果偏差的"宏观重规划"触发机制。
- [x] **项目级上下文合成**: 为每一个 Goal 提供基于路线图当前进度的全局上下文。

### 第三阶段：闭环演进与自主治理 (Autonomous Governance)
**重点**: 实现战略级的自我迭代。
- [x] **战略反思循环**: 在 `Cognitive Loop` 之上构建 `Roadmap-Reflection` 循环，反思整体路径的合理性。
- [x] **自主里程碑生成**: 根据项目初始需求，利用 LLM 自动建议最优的阶段划分与里程碑设定。
- [x] **资源/成本全局优化**: 基于路线图预测整体资源消耗并优化目标执行顺序。

---

## 4. 核心模型定义 (Final)
- **Project**: 项目根节点，包含名称、全局上下文及 `Roadmap`。
- **Milestone**: 关键交付点，包含目标集合及验收标准。
- **Phase**: 逻辑阶段，包含一组有序或部分有序的 `Goal`。
- **Goal**: 原子执行单位（由 `TeamOrchestrator` 处理）。

## 5. 后续演进方向

路线图核心目标已全部完成。后续可关注的演进方向：

| 方向 | 说明 |
|------|------|
| **技能市场** | 在线技能分发与安装机制 |
| **REST API** | 在 gRPC 之外提供 HTTP/JSON API |
| **多工作区** | 支持同时管理多个项目路线图 |
| **可视化看板** | 基于 Web 的路线图看板 |
