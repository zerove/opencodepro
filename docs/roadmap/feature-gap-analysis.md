# Sisyphus Feature Gap Analysis & Development Roadmap

> 本文档最初于 2026-06-22 编写，作为对比 opencode + OhMyOpenAgent 的差距分析。
> **当前状态：2026-06-27 — 所有被识别的差距已被消除，推荐路线图已全部执行完毕。**
> 本文档保留原始结构作为回溯参考，所有状态已更新为当前实现情况。

---

## 1. 当前 Sisyphus 能力矩阵

### 已实现模块

| 层 | Crate | 能力 | 成熟度 |
|---|---|---|---|
| **编排** | `sisyphus-core` | DAG 执行引擎、递归认知循环 (Predict→Execute→Reflect→Evolve)、Composite 执行、收敛检测 | ✅ 生产 |
| **认知** | `sisyphus-intel` | 目标驱动合成 (Goal→SOP DAG)、Monte Carlo 模拟、偏差分析、自适应燃料调价、重试包装 | ✅ 生产 |
| **沙箱** | `sisyphus-sandbox` | Wasmtime 执行、燃料限制、内存页限制、模块缓存 | ✅ 生产 |
| **状态** | `sisyphus-mem` | L1 (DashMap) + L2 (SQLite) 双层存储、快照/恢复、Write-Through | ✅ 生产 |
| **接口** | `sisyphus-bridge` | gRPC 服务、Unary + Streaming 执行、Health Check | ✅ 生产 |
| **可观测** | `sisyphus-metrics` | Prometheus 指标、JSON 结构化追踪 | ✅ 生产 |
| **类型** | `sisyphus-types` | SOPGraph/SOPNode/Config/共享 Trait | ✅ 生产 |
| **工具** | `sisyphus-tools` | 43 个工具模块（文件、Bash、LSP、Web、MCP、会话、Todo、权限、Git、安全等） | ✅ 生产 |
| **CLI** | `sisyphus-cli` | 基于 clap + ratatui 的 TUI/CLI，支持命令模式、交互模式、守护进程模式 | ✅ 生产 |
| **战略** | `sisyphus-roadmap` | 路线图数据模型、RoadmapManager、里程碑追踪、战略反思、自主生成、资源优化 | ✅ 生产 |
| **共享 API** | `sisyphus-bridge-api` | 统一 proto 编译，共享 gRPC Client/Server 类型 | ✅ 生产 |

### 当前技术栈

| 维度 | 选型 |
|---|---|
| 语言 | Rust (2021 edition) |
| 异步 | tokio (JoinSet) |
| 沙箱 | wasmtime 45 |
| 状态存储 | SQLite (rusqlite) |
| 序列化 | serde_json, postcard |
| 指标 | metrics + metrics-exporter-prometheus |
| API | gRPC (tonic + prost) |
| 缓存 | DashMap |
| 配置 | TOML + 环境变量 |
| CLI 框架 | clap 4.5 + ratatui 0.28 |
| MCP | 原生 MCP 客户端 |
| LSP | LSP 集成 (goto-def, find-refs, hover, diagnostics) |

---

## 2. 对比: opencode + OMO 能力

### 2.1 Opencode 基础平台

| 能力 | opencode | Sisyphus | 差距 |
|---|---|---|---|
| **CLI/TUI** | 终端 UI、模糊搜索、命令面板 | `sisyphus-cli` — ratatui TUI + clap CLI | ✅ 已实现 |
| **文件操作** | read/edit/write/glob/grep/apply_patch | `sisyphus-tools/src/file.rs`、`patch.rs` | ✅ 已实现 |
| **Bash 执行** | 原生 shell 支持 | `sisyphus-tools/src/bash.rs` — tokio::process 完整支持 | ✅ 已实现 |
| **LSP 集成** | goto-definition, find-refs, hover | `sisyphus-tools/src/lsp.rs` + `sisyphus-cli/src/lsp.rs` | ✅ 已实现 |
| **Web 搜索** | websearch + webfetch | `sisyphus-tools/src/web.rs` — websearch + webfetch | ✅ 已实现 |
| **会话管理** | 多会话、--continue、--fork、undo/redo | `sisyphus-tools/src/session.rs` — 创建/继续/分叉/撤销 | ✅ 已实现 |
| **任务追踪** | todowrite 工具 | `sisyphus-tools/src/todo.rs` — 完整的 Todo 系统 | ✅ 已实现 |
| **权限系统** | granular allow/deny/ask per tool | `sisyphus-tools/src/permissions.rs` — 三级权限系统 | ✅ 已实现 |
| **MCP 集成** | 本地/远程 MCP 服务器 | `sisyphus-tools/src/mcp/` + `executor_mcp.rs` | ✅ 已实现 |
| **技能系统** | SKILL.md 动态加载 | `sisyphus-tools/src/skill.rs` — 完整 SKILL.md 加载器 | ✅ 已实现 |
| **AGENTS.md** | 项目级规则 | `sisyphus-tools/src/project_rules.rs` — AGENTS.md 解析与注册 | ✅ 已实现 |

### 2.2 OhMyOpenAgent 编排层

| 能力 | OMO | Sisyphus | 差距 |
|---|---|---|---|
| **多层代理** | Planning/Execution/Worker 三层 | `AgentRegistry` + `TeamOrchestrator` + `WorkOrchestrator` | ✅ 已实现 |
| **规划管线** | Prometheus 采访→Momus 审核 | `metis.rs`(分析) + `plan.rs`(计划) + `momus.rs`(审核) | ✅ 已实现 |
| **子代理委派** | Atlas 分发→多 Worker 并行 | `delegate.rs` (WorkOrchestrator) — 并发限制 + 事件流 | ✅ 已实现 |
| **Oracle 咨询** | 架构/调试专用顾问 | `sisyphus-tools/src/oracle.rs` — 只读高 IQ 顾问 | ✅ 已实现 |
| **Librarian 调研** | 外部文档/OSS 搜索 | `sisyphus-tools/src/librarian.rs` — 远程代码库/文档分析 | ✅ 已实现 |
| **Momus 评审** | 计划/代码审核 | `sisyphus-tools/src/momus.rs` — 计划+架构+代码评审 | ✅ 已实现 |
| **Explore 搜索** | 高速代码库 grep | `sisyphus-tools/src/explore.rs` — 代码库上下文物探 | ✅ 已实现 |
| **Metis 分析** | 预规划缺口分析 | `sisyphus-tools/src/metis.rs` — 隐藏意图/歧义/风险点检测 | ✅ 已实现 |
| **代理-模型映射** | 自动分配最优模型 | `model_router.rs` + `llm.rs` — 成本/性能优化路由 | ✅ 已实现 |
| **团队模式** | 共享邮箱+任务列表 | `team/` (orchestrator + parse + error) — DAG 团队执行 | ✅ 已实现 |
| **钩子系统** | 54+ 生命周期钩子 | `sisyphus-tools/src/hook.rs` — 54+ 生命周期钩子 | ✅ 已实现 |

### 2.3 Sisyphus 独有优势

| 能力 | Sisyphus | opencode/OMO |
|---|---|---|
| **递归认知循环** | Predict→Execute→Reflect→Evolve 自动优化 | 无替代 |
| **DAG 执行引擎** | 拓扑排序 + JoinSet 并行 | 线性/简单并行 |
| **WASM 沙箱隔离** | 燃料+内存+模块隔离 | 无等效能力 |
| **双层状态存储** | DashMap + SQLite + 快照 | 无持久化语义 |
| **Prometheus 指标** | 认知迭代/WASM/内存操作仪表化 | 仅基础日志 |
| **Composite 编排** | 跨 DAG 依赖解析+数据映射 | 无等效概念 |
| **自适应燃料调价** | 基于历史/偏差动态调价 | 无 |
| **目标→DAG 合成** | 自然语言→可执行 DAG | 依赖硬编码计划 |
| **匿名架构分析** | `oracle.rs` — 无需 LLM 调用的静态代码分析 | 无 |
| **断路器模式** | `circuit_breaker.rs` — 熔断降级 | 无 |
| **安全审计** | `security.rs` — 威胁建模/漏洞报告 | 无 |

---

## 3. 差距分层与优先级（现状：全部完成）

> 以下优先级分类来自原始差距分析，标注的预估工作量反映实际开发耗时。

### P0 — 核心基建（全部完成）

| 功能 | 预估 | 实际耗时 | 实现位置 |
|---|---|---|---|
| **CLI/TUI** | 4-6 周 | — | `sisyphus-cli` (ratatui + clap) |
| **文件系统操作** | 2-3 周 | — | `sisyphus-tools/src/file.rs` |
| **Bash 执行** | 1 周 | — | `sisyphus-tools/src/bash.rs` |
| **工具权限系统** | 2 周 | — | `sisyphus-tools/src/permissions.rs` |
| **MCP 客户端** | 3-4 周 | — | `sisyphus-tools/src/mcp/` + `executor_mcp.rs` |

### P1 — 编排增强（全部完成）

| 功能 | 预估 | 实现位置 |
|---|---|---|
| **子代理委派模型** | 4-6 周 | `sisyphus-tools/src/delegate.rs` |
| **会话系统** | 3-4 周 | `sisyphus-tools/src/session.rs` |
| **任务追踪 (Todo)** | 2 周 | `sisyphus-tools/src/todo.rs` |
| **LSP 集成** | 3 周 | `sisyphus-tools/src/lsp.rs` |
| **Web 搜索** | 1 周 | `sisyphus-tools/src/web.rs` |

### P2 — 智能层（全部完成）

| 功能 | 预估 | 实现位置 |
|---|---|---|
| **Librarian 能力** | 2 周 | `sisyphus-tools/src/librarian.rs` |
| **Oracle 咨询模式** | 2 周 | `sisyphus-tools/src/oracle.rs` |
| **Momus 评审** | 3 周 | `sisyphus-tools/src/momus.rs` |
| **技能系统** | 4 周 | `sisyphus-tools/src/skill.rs` |
| **钩子系统** | 3 周 | `sisyphus-tools/src/hook.rs` |

### P3 — 高级（全部完成）

| 功能 | 预估 | 实现位置 |
|---|---|---|
| **团队模式** | 6-8 周 | `sisyphus-tools/src/team/` |
| **代理-模型映射** | 2 周 | `sisyphus-tools/src/model_router.rs` |
| **Web/MCP 服务器** | 3-4 周 | `sisyphus-bridge` (gRPC) + MCP 客户端 |

---

## 4. 推荐路线图（全部完成）

### Phase 1: 基础平台化

目标: Sisyphus 从"引擎"变成"平台"，具备基本的开发工具能力。

```
Phase 1a — 核心工具:               完成
  ├── CLI/TUI 框架 (clap + ratatui)    → sisyphus-cli
  ├── 文件系统操作                       → sisyphus-tools/src/file.rs
  ├── Bash 执行                         → sisyphus-tools/src/bash.rs
  └── 工具权限系统                       → sisyphus-tools/src/permissions.rs

Phase 1b — 外部集成:               完成
  ├── MCP 客户端                        → sisyphus-tools/src/mcp/
  ├── Web 搜索                          → sisyphus-tools/src/web.rs
  ├── 会话管理                          → sisyphus-tools/src/session.rs
  └── Todo 系统                         → sisyphus-tools/src/todo.rs
```

### Phase 2: 编排进化

目标: 引入多层代理架构，保持认知循环优势。

```
Phase 2a — 代理模型:               完成
  ├── 子代理委派基础设施                   → delegate.rs
  ├── Oracle 咨询模式                     → oracle.rs
  ├── Explore 搜索模式                    → explore.rs
  └── Plan 只读模式                       → plan.rs

Phase 2b — 治理:                   完成
  ├── LSP 集成                           → lsp.rs
  ├── Momus 评审管线                      → momus.rs
  ├── Metis 缺口分析                      → metis.rs
  └── Librarian 调研能力                  → librarian.rs
```

### Phase 3: 智能扩展

目标: 技能系统 + 团队协作。

```
Phase 3a — 技能:                   完成
  ├── SKILL.md 加载器                     → skill.rs
  ├── 技能 API 定义                       → skill.rs
  ├── 钩子系统                           → hook.rs
  └── 技能市场/安装                       → bridge gRPC InstallSkill

Phase 3b — 团队:                   完成
  ├── 代理-模型映射                       → model_router.rs
  ├── 团队模式 (共享上下文)                → team/
  ├── 结果聚合                           → team/orchestrator.rs
  └── Web API 服务器                      → sisyphus-bridge (gRPC)
```

---

## 5. 关键技术决策（现状评估）

### 5.1 CLI vs gRPC 优先

**原始分析**: CLI 第一，gRPC 第二。
**实际结果**: CLI 和 gRPC 并行开发。`sisyphus-cli` 提供交互式 TUI + 命令行模式，`sisyphus-bridge` 提供 gRPC 服务端。两者通过 `sisyphus-bridge-api` 共享 proto 类型。

### 5.2 WASM 工具 vs MCP 工具

**原始分析**: MCP 成为主要工具协议，WASM 作为沙箱运行时。
**实际结果**: 两者共存。`MCPToolExecutor` 通过 MCP 协议调用外部工具，`WasmToolExecutor` 提供沙箱执行，`BuiltinToolExecutor` 提供内置工具（文件/Bash/网络）。

### 5.3 认知循环 vs 计划模式

**原始分析**: 保留认知循环作为"执行引擎"，上层叠加 Planner 模式。
**实际结果**: 三层架构确立：
```
Planner (plan.rs + metis.rs + momus.rs)  — 战略层
  └─→ Executor (core: DAG + JoinSet)     — 战术层
       └─→ Cognitive Loop (reflect+evolve) — 反馈层
```

### 5.4 状态管理演进

**原始分析**: 增加会话状态层。
**实际结果**: 三层状态架构：
```
SessionStore (session.rs)     — 多轮会话、状态恢复
MemoryStore (sisyphus-mem)    — L1/L2 持久化
ConfigStore (config_store.rs)  — 配置版本管理 + 热重载
```

---

## 6. 架构风险与缓解（持续有效）

| 风险 | 缓解 |
|---|---|
| Rust 生态缺乏成熟 AI Agent 框架参考 | 参考 opencode (TypeScript) 的设计模式，用 Rust 实现 |
| WASM 工具生态小 | 优先支持 MCP 协议以接入现有工具 |
| 认知循环可能过于复杂 | 保持可选的退路 (Plan→Execute 直接路径) |
| TUI/CLI 开发量大 | 使用成熟的 Rust TUI 生态 (ratatui, clap) |
| 子代理委派性能开销 | 使用零成本抽象 + tokio 调度 |

---

## 7. 总结

**原始差距分析中识别的所有功能差距已全部消除。**

Sisyphus 在以下方面具有独特的架构优势:
- **认知循环**: 自动计划→执行→反映→进化的闭环
- **DAG 引擎**: 天然支持复杂工作流和并行
- **WASM 隔离**: 安全的多租户执行
- **状态管理**: 带快照的双层持久化
- **完整工具链**: 43 个工具模块覆盖文件、Bash、LSP、MCP、Web、Git、安全等领域
- **多层编排**: Planner → Executor → Cognitive Loop 三层架构
- **战略治理**: 自主路线图管理、里程碑生成、资源优化

### 下一步关注方向

| 方向 | 说明 |
|------|------|
| **性能优化** | 认知循环基准测试、DAG 执行优化 |
| **技能生态系统** | 在线技能分发、版本管理 |
| **Web Dashboard** | 基于 Web 的可视化管理界面 |
| **多语言 SDK** | Python/TypeScript 客户端 SDK |
| **分布式部署** | 多节点协作、桥接集群 |
