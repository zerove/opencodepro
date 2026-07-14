# Project Context: OpenCodePro

## 🚀 项目概览
OpenCodePro 是一个高度自主的**项目架构师 (Project Architect)** 引擎。它实现了从宏观战略规划 (`Roadmap`) 到微观目标执行 (`Goal-Executor`) 的完整闭环，支持基于 `Cognitive Loop` 的自适应认知与动态修正。

## 🎯 当前状态
- **路线图进度**：✅ 全阶段已完成。
- **核心能力**：
  - 执行层规划 (DAG-based Parallel Execution)
  - 自适应认知循环 (Prediction $\rightarrow$ Execution $\rightarrow$ Reflection $\rightarrow$ Evolution)
  - 专业 Agent 角色矩阵 (Metis, Momus, Scribe, etc.)
  - **多租户隔离架构 (Workspace-Aware Isolation)**：所有存储操作显式基于 `project_id` 进行隔离，确保多项目并发执行时的状态绝对安全。
  - 状态持久化 (MfsStore + SQL Store)
- **当前阶段**：维护与演进模式。关注重点在于稳定性提升及扩展 API (REST API) 与可视化看板。

## 🛠️ 开发标准
本项目严格遵循 `DEVELOPMENT_STANDARD.md` 定义的 AI-Native 工作流：
- **强制路径**：`上下文同步` $\rightarrow$ `设计先导` $\rightarrow$ `TDD 编码` $\rightarrow$ `自动化审计` $\rightarrow$ `资产更新`。
- **语言规范**：所有代码变更必须通过 `docs/specs/` 下的语言规范审计。
- **架构约束**：任何重大变更必须创建新的 `ADR` 文档。

## 📌 关键路径
- **核心逻辑**：`src/`
- **战略定义**：`docs/ROADMAP.md`
- **决策记录**：`docs/adr/`
- **质量门禁**：`ai-context-sync audit`
