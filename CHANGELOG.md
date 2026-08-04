# 变更日志

Sisyphus 的所有重要变更将记录在本文件中。

格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，
本项目遵循 [语义化版本](https://semver.org/lang/zh-CN/)。

## [0.1.0] — 未发布

### 新增

#### 核心引擎
- **认知循环 v1**：可配置最大迭代次数与收敛阈值的递归观察-定向-决策-行动循环。
- **认知循环 v2**：自适应深度控制（置信度 + 收敛速度）、并行分支预测与运行时异常检测。
- **SOP (标准操作程序)**：基于 DAG 的程序表示，通过拓扑排序实现并行执行。
- **蒙特卡洛仿真**：SOP 图谱上的路径概率估计与混合成本分析。
- **结构化反思**：预测与实际执行结果间的偏差评分。
- **自适应燃料定价**：基于执行历史动态调整预算的指数退避燃料定价器。
- **SOP 演进**：自动图谱变异 — 重试包装、燃料调整与子图重构。
- **跨图谱依赖解析**：跨多个 SOP 的拓扑执行顺序 CompositeGraph。
- **自编程闭环**：工具缺口检测 → 生成 → 编译 → 验证 → 部署 → 注册。
- **混沌工程**：可配置故障注入（延迟/错误），重复故障触发自动修复管线。
- **战略规划**：Project → Milestone → Phase → Goal 层级的 RoadmapManager，战略反思与自主里程碑生成。

#### 存储与检索
- **多级缓存**：L1 (DashMap 内存) + L2 (SQLite 持久化)，支持快照/恢复与写透传。
- **结构化存储**：SQLite/PostgreSQL 后端，支持 pgvector 语义搜索。
- **向量记忆**：存储嵌入的余弦相似度搜索（文本 + 视觉模态）。
- **经验库**：基于向量的历史执行经验语义检索。
- **用户偏好**：持久化的用户级偏好存储。

#### API 与接口
- **gRPC 桥接**：17 个 RPC，含 ExecuteSOP、ExecuteTool、DecomposeGoal、技能/插件/工作区管理、流式执行与元学习洞察。
- **REST API**：34 个端点（健康、目标、会话、SOP、技能、工作区、路线图、认知地图），OpenAPI 3.1 规范与 Swagger UI。
- **CLI 客户端**：40+ 子命令，覆盖文件工具、Git、构建、Agent、引擎、会话与开发工作流。
- **TUI**：基于 ratatui 的交互式终端，支持会话、仪表盘、DAG 追踪、工作区管理与技能市场。
- **Web 仪表盘**：axum + HTMX + SSE 实时仪表盘，含 DAG 可视化、混沌面板与实时事件。
- **SDK**：TypeScript (14 测试)、Python (21 测试)、Go (13 测试)、Rust (`sisyphus-sdk`)。

#### 沙箱与部署
- **WASM 沙箱**：Wasmtime 安全工具代码执行，燃料计量、内存限制与模块缓存。
- **Sandboxd 守护进程**：进程隔离 WASM 执行，支持 stdio JSON-line (v1) 与 gRPC (v2) 协议。
- **Docker**：多阶段构建与 docker-compose（bridge + 可选 PostgreSQL）。
- **Nightly CI**：自动化 GHCR 镜像发布与多平台二进制。
- **发布管线**：自动变更日志生成与多平台制品上传。

#### 开发者体验
- **IDE 插件**：VS Code 扩展（状态栏、会话树、Agent 面板、DAG 视图）与 Zed 扩展（LSP 桥接、斜杠命令）。
- **属性测试**：SOPGraph 往返、ExecutionContext 操作、CompositeGraph 正确性的 Proptest 覆盖。
- **供应链审计**：cargo-deny 配置，含许可证、禁用与安全通告检查。
- **mdBook 文档**：架构概览、crate 参考、配置指南与核心概念。
- **Pre-commit 钩子**：每次提交自动 fmt + clippy + check。
- **CI 管线**：14 步工作流，覆盖 fmt、clippy、全量测试套件 (1261 测试)、基准测试、E2E 冒烟与 SDK 测试。

### 变更
- 单体 `sisyphus-tools` 拆分为 10 个独立子 crate。
- `MfsStore` 现在自动创建 SQLite 数据库文件（修复默认配置下的生产启动问题）。
- REST 路由统一为 `{id}` 参数语法以兼容 axum 0.8。

### 修复
- CLI bin 测试未被 CI 编译（仅 `--lib`）；现已运行 52 个测试。
- Bridge 集成测试因 OOM 被排除在 CI workspace 外；现已隔离为单独 crate 运行。
- SQLite 文件持久化测试曾被 `#[ignore]`；现已启用并通过。
- 废弃的 `upload-release-asset` GitHub Action 替换为 `upload-artifact` + `gh-release`。

## [0.0.1] — 2025-06-01

### 新增
- 初始项目脚手架与认知循环原型。
- 带燃料计量与内存限制的 WASM 沙箱。
