# Sisyphus Feature Gap Analysis & Development Roadmap

> 对比 opencode + OhMyOpenAgent 全量功能，规划 Sisyphus 后续开发方向。
> 日期: 2026-06-22

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

---

## 2. 对比: opencode + OMO 能力

### 2.1 Opencode 基础平台

| 能力 | opencode | Sisyphus | 差距 |
|---|---|---|---|
| **CLI/TUI** | 终端 UI、模糊搜索、命令面板 | ❌ 无 | **严重** |
| **文件操作** | read/edit/write/glob/grep/apply_patch | ❌ 无 | **严重** |
| **Bash 执行** | 原生 shell 支持 | ❌ 无 | **严重** |
| **LSP 集成** | goto-definition, find-refs, hover | ❌ 无 | **中等** |
| **Web 搜索** | websearch + webfetch | ❌ 无 | **中等** |
| **会话管理** | 多会话、--continue、--fork、undo/redo | ❌ 无 | **严重** |
| **任务追踪** | todowrite 工具 | ❌ 无 | **中等** |
| **权限系统** | granular allow/deny/ask per tool | ❌ 无 | **中等** |
| **MCP 集成** | 本地/远程 MCP 服务器 | ❌ 无 | **严重** |
| **技能系统** | SKILL.md 动态加载 | ❌ 无 | **严重** |
| **AGENTS.md** | 项目级规则 | ❌ 但可用 GEMINI.md | **低** |

### 2.2 OhMyOpenAgent 编排层

| 能力 | OMO | Sisyphus | 差距 |
|---|---|---|---|
| **多层代理** | Planning/Execution/Worker 三层 | 单一 Kernel | **严重** |
| **规划管线** | Prometheus 采访→Momus 审核 | 认知循环 (不同抽象层次) | **对齐中** |
| **子代理委派** | Atlas 分发→多 Worker 并行 | JoinSet 内并行 | **架构差异** |
| **Oracle 咨询** | 架构/调试专用顾问 | ❌ 无 | **中等** |
| **Librarian 调研** | 外部文档/OSS 搜索 | ❌ 无 | **中等** |
| **Momus 评审** | 计划/代码审核 | ❌ 无 | **中等** |
| **Explore 搜索** | 高速代码库 grep | ❌ 无 | **低** |
| **Metis 分析** | 预规划缺口分析 | 已有偏差分析 (不同层次) | **低** |
| **代理-模型映射** | 自动分配最优模型 | ❌ 无 | **低** |
| **团队模式** | 共享邮箱+任务列表 | ❌ 无 | **严重** |
| **钩子系统** | 54+ 生命周期钩子 | ❌ 无 | **中等** |

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

---

## 3. 差距分层与优先级

### P0 — 核心基建 (必须做)

| 功能 | 理由 | 预估工作量 |
|---|---|---|
| **CLI/TUI** | 无法脱离 gRPC 独立使用，无开发体验 | 4-6 周 |
| **文件系统操作** | 不能读写文件 = 无法参与开发工作流 | 2-3 周 |
| **Bash 执行** | 不能运行外部命令 = 能力严重受限 | 1 周 |
| **工具权限系统** | 安全运行外部工具的必要条件 | 2 周 |
| **MCP 客户端** | 接入外部工具/数据源的通用协议 | 3-4 周 |

### P1 — 编排增强 (应该做)

| 功能 | 理由 | 预估工作量 |
|---|---|---|
| **子代理委派模型** | 替代扁平的 JoinSet，支持多样化 Worker | 4-6 周 |
| **会话系统** | 多轮交互、历史、继续/分支 | 3-4 周 |
| **任务追踪 (Todo)** | 面向用户的工作跟踪 | 2 周 |
| **LSP 集成** | 代码理解（跳转、引用、诊断） | 3 周 |
| **Web 搜索** | 信息获取能力 | 1 周 |

### P2 — 智能层 (值得做)

| 功能 | 理由 | 预估工作量 |
|---|---|---|
| **Librarian 能力** | 外部文档/源码调研 | 2 周 |
| **Oracle 咨询模式** | 只读架构顾问 | 2 周 |
| **Momus 评审** | 计划/代码质量门禁 | 3 周 |
| **技能系统** | 可插拔的扩展指令 | 4 周 |
| **钩子系统** | 生命周期可观察/可干预 | 3 周 |

### P3 — 高级 (远期)

| 功能 | 理由 | 预估工作量 |
|---|---|---|
| **团队模式** | 多 Agent 协作 | 6-8 周 |
| **代理-模型映射** | 自动模型选择 | 2 周 |
| **Web/MCP 服务器** | 对外暴露能力 | 3-4 周 |

---

## 4. 推荐路线图

### Phase 1: 基础平台化 (8-10 周)

目标: Sisyphus 从"引擎"变成"平台"，具备基本的开发工具能力。

```
Phase 1a (4-5 周) — 核心工具:
  ├── CLI/TUI 框架 (clap + ratatui)
  ├── 文件系统操作 (read/write/edit/glob/grep)
  ├── Bash 执行 (tokio::process)
  └── 工具权限系统 (allow/deny/ask)

Phase 1b (4-5 周) — 外部集成:
  ├── MCP 客户端 (连接外部工具)
  ├── Web 搜索 (HTTP 客户端)
  ├── 会话管理 (多轮/历史/继续)
  └── Todo 系统
```

### Phase 2: 编排进化 (6-8 周)

目标: 引入多层代理架构，保持认知循环优势。

```
Phase 2a (3-4 周) — 代理模型:
  ├── 子代理委派基础设施
  ├── Oracle 咨询模式
  ├── Explore 搜索模式
  └── Plan 只读模式

Phase 2b (3-4 周) — 治理:
  ├── LSP 集成
  ├── Momus 评审管线
  ├── Metis 缺口分析
  └── Librarian 调研能力
```

### Phase 3: 智能扩展 (6-8 周)

目标: 技能系统 + 团队协作。

```
Phase 3a (3-4 周) — 技能:
  ├── SKILL.md 加载器
  ├── 技能 API 定义
  ├── 钩子系统
  └── 技能市场/安装

Phase 3b (3-4 周) — 团队:
  ├── 代理-模型映射
  ├── 团队模式 (共享上下文)
  ├── 结果聚合
  └── Web API 服务器
```

---

## 5. 关键技术决策

### 5.1 CLI vs gRPC 优先

**现状**: 纯 gRPC，无 CLI。
**建议**: CLI 第一，gRPC 第二。CLI 提供开发体验，gRPC 提供集成。

```
sisyphus-cli (新 crate)
├── 交互模式 (ratatui TUI)
├── 命令模式 (脚本/CI)
└── 守护进程模式 (后台 + gRPC)
```

### 5.2 WASM 工具 vs MCP 工具

**现状**: 只有 WASM 工具。
**建议**: MCP 成为主要工具协议，WASM 作为沙箱运行时。

```
ToolExecutor trait
├── MCPToolExecutor (新) — 通过 MCP 协议调用外部工具
├── WasmToolExecutor (现有) — 安全沙箱
└── BuiltinToolExecutor (新) — 文件/bash/网络
```

### 5.3 认知循环 vs 计划模式

**现状**: 递归认知循环 (自动优化)。
**建议**: 保留认知循环作为"执行引擎"，上层叠加 Planner 模式。

```
Planner (新)          — 战略层: 采访 → 计划 → 审核
  └─→ Executor (现)   — 战术层: DAG 执行
       └─→ Cognitive Loop (现) — 反馈层: 反映 → 进化
```

### 5.4 状态管理演进

**现状**: L1 DashMap + L2 SQLite。
**建议**: 保留 MFS 作为通用存储，增加会话状态层。

```
SessionStore (新)     — 多轮会话、状态恢复
MemoryStore (现有)    — L1/L2 持久化
ConfigStore (新)      — 配置版本管理
```

---

## 6. 架构风险与缓解

| 风险 | 缓解 |
|---|---|
| Rust 生态缺乏成熟 AI Agent 框架参考 | 参考 opencode (TypeScript) 的设计模式，用 Rust 实现 |
| WASM 工具生态小 | 优先支持 MCP 协议以接入现有工具 |
| 认知循环可能过于复杂 | 保持可选的退路 (Plan→Execute 直接路径) |
| TUI/CLI 开发量大 | 使用成熟的 Rust TUI 生态 (ratatui, clap) |
| 子代理委派性能开销 | 使用零成本抽象 + tokio 调度 |

---

## 7. 总结

Sisyphus 在以下几方面具有独特的架构优势:
- **认知循环**: 自动计划→执行→反映→进化的闭环
- **DAG 引擎**: 天然支持复杂工作流和并行
- **WASM 隔离**: 安全的多租户执行
- **状态管理**: 带快照的双层持久化

但与 opencode + OMO 相比，缺乏:
- **基础工具能力**: 文件操作、Bash、CLI
- **代理多样性**: 没有专用角色 (Oracle/Librarian/Explore)
- **扩展机制**: 没有 MCP/技能/钩子系统
- **团队协同**: 单进程单体而非多 Agent 网络

**推荐的 first step**: 实现 CLI 框架 + 文件操作 + Bash 执行 (Phase 1a)，这是从"引擎"到"平台"的关键跨越。
