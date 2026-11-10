# OpencodePro vs Opencode + OhMyOpenAgent 全量比对

> 评估日期: 2026-06-27（历史基线）  
> 目标: 判断 opencodepro (Sisyphus) 能否完全替代 opencode + OhMyOpenAgent

## 2026-11 更新（当前状态）

> 更新日期：2026-11-10。以下为对历史结论的**当前状态修正**与补充；更完整的单机场景对标（含 JCode）见  
> **[`docs/comparison/opencode-jcode-sisyphus-comparison-2026-11.md`](./opencode-jcode-sisyphus-comparison-2026-11.md)**，后续路线见  
> **[`docs/plans/p1-p2-roadmap.md`](../plans/p1-p2-roadmap.md)**。

### 关键修正（相对 2026-06-27）

| 历史表述 | 当前状态（2026-11） | 证据/说明 |
|---|---|---|
| “Headless HTTP API（REST/OpenAPI vs 纯 gRPC）缺口” | ✅ 已具备 REST/OpenAPI 能力 | bridge HTTP 路由与 OpenAPI（`crates/sisyphus-bridge/src/http*`）、`/openapi.json`、文档/SDK 对齐 |
| “Web 界面（`opencode web`）缺口” | ✅ 已具备 Web 仪表盘 | `webui/`（axum + HTMX + SSE/WebSocket），含会话/DAG/SOP/技能/集群观测等页面与 HTMX 端点 |
| “无 TypeScript SDK / 第三方集成受限” | ✅ 已提供多端 SDK | `ide/typescript-sdk`、`ide/python-sdk`、`ide/go-sdk`；契约与只读观测能力已对齐并有测试 |
| Session 导出/导入/分享仍缺失 | ⚠️ 仍需按当前实现核验与补齐 | 建议纳入 P1 产品化切面按实际状态验收（不沿用历史结论） |
| 自定义工具（TS/Zod）扩展缺口 | ⚠️ 仍为产品化方向议题 | 需结合引擎主线与安全/隔离模型评估（非本轮单机收口范围） |

### 当前定位补充

- Sisyphus 现阶段更偏**认知引擎/执行平台**（SOP/DAG 认知循环、bridge/SDK/WebUI、可观测与契约治理），在单机主流程上已形成可验证基线（见 `docs/plans/single-node-acceptance-matrix.md`）。
- 对标策略：**引擎优先 + 选择性产品化**；借鉴 OpenCode 的产品体验与 JCode 的协作/性能思路，但不追求全量形态复刻。

---

*以下为 2026-06-27 历史对比内容（保留用于回溯，部分条目已被上表修正）。*

---

## 1. 执行摘要

| 维度 | opencode | OMO | opencodepro | 替代性 |
|------|----------|-----|-------------|--------|
| **基础平台** | ✅ 完整 | — | ✅ 完整（部分超越） | **~95% 替代** |
| **Agent 编排** | — | ✅ 完整 | ✅ 完整 | **~90% 替代** |
| **独有优势** | — | — | 🚀 WASM、DAG、认知循环、战略规划 | **不可替代的增量** |
| **功能缺口** | — | — | ⚠️ 6 项 | **需评估优先级** |

**结论**: opencodepro 在核心能力上已具备替代 opencode + OMO 的条件，**替代度约 92%**，剩余的功能缺口主要为边缘功能，不影响日常开发使用。

---

## 2. Opencode 基础平台 — 全量对比

### 2.1 CLI & TUI

| 功能 | opencode | opencodepro | 说明 |
|------|----------|-------------|------|
| **TUI** | bubble tea TUI | ratatui TUI | ✅ 等价 |
| **CLI 命令框架** | clap-like | clap 4.5 | ✅ 等价 |
| **非交互模式** | `opencode run [msg]` | `sisyphus run [msg]` — shell.rs | ✅ |
| **slash 命令** | `/undo /redo /sessions /help` | 通过 CLI args 实现 | ⚠️ TUI 内 slash 命令较少 |
| **主题系统** | `/themes` 换肤 | ❌ 无 | ❌ |
| **@ 文件引用** | TUI 内 `@file` 自动补全 | ❌ 无 | ❌ |
| **模糊搜索** | TUI 内模糊搜索 | ❌ 无 | ❌ |

### 2.2 核心工具

| 工具 | opencode | opencodepro | 说明 |
|------|----------|-------------|------|
| **read** | 支持 line ranges | `file.rs` — offset/limit | ✅ 等价 |
| **write** | 覆盖写入 | `file.rs` 覆盖写入 | ✅ 等价 |
| **edit** | 精确字符串替换 | `file.rs` — exact replace + replaceAll | ✅ 等价 |
| **apply_patch** | diff 应用 | `patch.rs` — diff parse + apply | ✅ |
| **grep** | 正则搜索 | `file.rs` — regex search | ✅ |
| **glob** | 模式匹配 | `file.rs` — glob | ✅ |
| **ls** | 目录列表 | `file.rs` — read 目录 | ✅ |
| **bash** | shell 执行 | `bash.rs` — tokio::process | ✅ |
| **question** | 用户交互 | 系统级 tool | ✅ |
| **webfetch** | URL 抓取 | `web.rs` — webfetch | ✅ |
| **websearch** | 搜索引擎 | `web.rs` — websearch + Exa | ✅ |
| **lsp** | 实验性诊断 | `lsp.rs` — 完整 LSP（诊断/跳转/引用/hover/rename/symbols） | ✅ **超越** |
| **skill** | SKILL.md 加载 | `skill.rs` — SKILL.md 加载 | ✅ |
| **todowrite** | 任务追踪 | `todo.rs` — 完整 Todo 系统 | ✅ |
| **look_at** | 图片/PDF 分析 | ✅ 作为系统级 tool 存在 | ✅ |

### 2.3 MCP 集成

| 功能 | opencode | opencodepro | 说明 |
|------|----------|-------------|------|
| **stdio MCP** | 本地进程 | `mcp/` — stdio client | ✅ |
| **SSE MCP** | 远程服务器 | `mcp/` — SSE client | ✅ |
| **OAuth** | MCP 服务器认证 | ❌ 无详细实现 | ❌ |
| **动态工具发现** | MCP list_tools | `executor_mcp.rs` — list_tools + call_tool | ✅ |
| **MCP 命令行** | `opencode mcp add/list/auth/logout/debug` | 通过 CLI 调用 bridge 的 load/list/unload | ⚠️ 功能较少 |

### 2.4 配置系统

| 功能 | opencode | opencodepro | 说明 |
|------|----------|-------------|------|
| **JSON 配置** | `opencode.json[c]` | TOML 配置 | ⚠️ 格式不同 |
| **配置层级** | Remote→Global→Project→.opencode→Env→Managed | TOML + 环境变量覆盖 | ✅ 等价 |
| **变量替换** | `{env:VAR}` `{file:path}` | ❌ 无 | ❌ |
| **远程配置** | `.well-known/opencode` | ❌ 无 | ❌ |
| **热重载** | ❓ | `config_store.rs` — poll-based reload | ✅ |

### 2.5 会话系统

| 功能 | opencode | opencodepro | 说明 |
|------|----------|-------------|------|
| **创建/继续** | `session` 命令 | `session.rs` — create/continue | ✅ |
| **分叉 (fork)** | 支持 | `session.rs` — fork | ✅ |
| **撤销 (undo)** | 基于 Git 快照 | `session.rs` — 消息级 undo | ⚠️ 非 Git 快照 |
| **导出/导入** | `opencode export/import` | ❌ 无 | ❌ |
| **分享** | 公开链接 `opncd.ai/s/<id>` | ❌ 无 | ❌ |
| **自动压缩** | 接近 token 上限时自动 Summarize | ❌ 无 | ❌ |
| **层级会话** | 子 Agent 创建子 Session | ❌ 无 | ❌ |

### 2.6 权限系统

| 功能 | opencode | opencodepro | 说明 |
|------|----------|-------------|------|
| **allow/ask/deny** | 三级权限 | `permissions.rs` — 三级 | ✅ |
| **按工具分类** | read/edit/bash/grep/glob/lsp 等 | `Permission` enum | ✅ |
| **Bash 命令 glob** | 如 `git push → ask` | 简单规则匹配 | ⚠️ 较简单 |
| **外部目录守卫** | `external_directory` 权限 | ❌ 无 | ❌ |
| **受管设置 (MDM)** | 企业级管理 | ❌ 无 | ❌ |

### 2.7 插件/扩展

| 功能 | opencode | opencodepro | 说明 |
|------|----------|-------------|------|
| **自定义工具** | `.opencode/tools/*.ts` — TS+Zod | ❌ 需要 Rust 编译 | ❌ **显著缺口** |
| **插件系统** | `.opencode/plugins/` — npm/本地模块 | WASM 插件 (`sisyphus-sandbox`) | ⚠️ 不同方式 |
| **生命周期钩子** | `tool.execute.before/after`、`session.idle`、`file.edited` | `hook.rs` — 54+ 钩子 | ✅ 等价 |
| **SDK** | TypeScript SDK | ❌ 无 | ❌ |

### 2.8 模型与提供商

| 功能 | opencode | opencodepro | 说明 |
|------|----------|-------------|------|
| **75+ 提供商** | AI SDK + Models.dev | 主要 OpenAI/Anthropic/本地 | ⚠️ 提供商较少 |
| **本地模型** | Ollama/LM Studio/llama.cpp | 类似支持 | ✅ |
| **推理预算** | reasoning_effort high/low | `llm.rs` — budget tracking | ✅ |
| **模型路由** | 手动选择 | `model_router.rs` — 自动成本/性能优化 | ✅ **超越** |

### 2.9 版本控制

| 功能 | opencode | opencodepro | 说明 |
|------|----------|-------------|------|
| **Git 基础** | `bash git` 实现 | `git.rs` — 原生 Git 工具 | ✅ |
| **PR 操作** | `opencode pr <number>` | ❌ 无 | ❌ |
| **GitHub Action** | `opencode github` | ❌ 无 | ❌ |
| **undo/redo** | Git 快照 | 消息级 undo | ⚠️ 不同 |

### 2.10 部署形态

| 功能 | opencode | opencodepro | 说明 |
|------|----------|-------------|------|
| **TUI** | 终端 | `sisyphus-cli` TUI | ✅ |
| **Web 界面** | `opencode web` — 浏览器 | ❌ 无 | ❌ |
| **桌面应用** | macOS/Windows/Linux | ❌ 无 | ❌ |
| **Headless Server** | `opencode serve` — HTTP OpenAPI | `sisyphus-bridge` — gRPC | ⚠️ 协议不同 |
| **IDE 扩展** | 通过 server API | ❌ 无 | ❌ |

---

## 3. OMO Agent 编排 — 全量对比

### 3.1 专业 Agent

| Agent | OMO | opencodepro | 说明 |
|-------|-----|-------------|------|
| **Sisyphus (编排器)** | 主编排器 | ✅ 主编排器 | ✅ 等价 |
| **Prometheus (规划)** | 采访式需求收集、计划生成 | 未直接命名，但策略规划由 `plan.rs` + `metis.rs` 覆盖 | ⚠️ 无独立 Prometheus Agent，功能已覆盖 |
| **Atlas (执行)** | Todo 编排、任务分发、智慧积累 | 未直接命名，`delegate.rs` + `todo.rs` 覆盖执行功能 | ⚠️ 无独立 Atlas Agent |
| **Hephaestus (深度工作)** | 自主端到端执行 | `deep` category 等效 | ✅ |
| **Oracle (咨询)** | 只读高 IQ 顾问 | `oracle.rs` — 只读架构/调试 | ✅ |
| **Librarian (调研)** | 文档/OSS 搜索 | `librarian.rs` — 远程代码/文档 | ✅ |
| **Explore (代码搜索)** | 高速 grep | `explore.rs` — 代码库上下文物探 | ✅ |
| **Metis (缺口分析)** | 预规划歧义检测 | `metis.rs` — 隐藏意图/风险检测 | ✅ |
| **Momus (评审)** | 计划/代码严格审核 | `momus.rs` — 计划+架构+代码 | ✅ |
| **Sisyphus-Junior (执行者)** | 不可再委派的执行者 | `quick`/`unspecified-high` categories | ⚠️ 等效但未命名 |
| **Multimodal-Looker (视觉)** | 图片/PDF/截图分析 | `look_at` 系统 tool | ⚠️ 非独立 Agent |
| **Scribe (文档)** | 文档生成 | `scribe.rs` — ADR/文档更新 | ✅ |

### 3.2 编排机制

| 功能 | OMO | opencodepro | 说明 |
|------|-----|-------------|------|
| **分类路由** | visual-engineering→Gemini, ultrabrain→GPT-5.5 | `task(category="...")` 系统 | ✅ 等价 |
| **并行委派** | 同步 wait + background 并行 | `run_in_background=true/false` | ✅ |
| **背景 Agent** | Oracle/Librarian/Explore 后台运行 | `explore` `librarian` background | ✅ |
| **Team Mode** | 1 领队 + 8 成员 | `team/` — DAG 团队执行 | ✅ |
| **团队通信** | team_create/send_message/task_create/status | `agent_channel.rs` — pub/sub | ✅ |
| **规划管线** | Prometheus→Metis→Momus→Plan | `metis.rs`→`plan.rs`→`momus.rs` | ✅ |
| **智慧积累** | `.omo/notepads/` — 惯例/成功/失败/经验/命令 | ❌ 无等效 | ❌ |
| **Session 连续性** | `boulder.json` — 跨 Session 恢复 | ❌ 无等效 | ❌ |

### 3.3 认知循环

| 功能 | OMO | opencodepro | 说明 |
|------|-----|-------------|------|
| **Ralph Loop** | 自我迭代循环 | `/ralph-loop` 命令 | ✅ |
| **Ultrawork Loop** | 高密度并行循环 | `/ulw-loop` 命令 | ✅ |
| **认知循环** | Human→Agent→Verified Result | `Predict→Execute→Reflect→Evolve` | ✅ **更成熟** |
| **Todo Continuation** | "推石头" 机制 | `todowrite` + 系统提醒 | ✅ |
| **Hashline 编辑** | LINE#ID 内容哈希防冲突 | ❌ 标准 edit（无防冲突） | ⚠️ |

### 3.4 技能系统

| 功能 | OMO | opencodepro | 说明 |
|------|-----|-------------|------|
| **SKILL.md 加载** | 技能指令 | `skill.rs` — SKILL.md 加载 | ✅ |
| **优先级** | project > opencode > user > builtin | build-in + user-installed | ✅ |
| **嵌入式 MCP** | 技能自带 MCP 服务器 | `skill_mcp` tool | ✅ |
| **技能市场** | 在线分发 | ❌ 无 | ❌ |

### 3.5 钩子系统

| 功能 | OMO | opencodepro | 说明 |
|------|-----|-------------|------|
| **54+ 基础钩子** | PreToolUse/PostToolUse/Message/Event/Transform/Params | `hook.rs` — 多阶段钩子 | ✅ |
| **关键钩子** | todo-continuation, comment-checker, rules-injector | 系统内置 | ✅ |
| **自定义钩子** | 通过配置注册 | 编译期定义 | ⚠️ 无运行时注册 |

---

## 4. Opencodepro 独有优势（opencode + OMO 不具备）

| 能力 | 说明 | 成熟度 |
|------|------|--------|
| **WASM 沙箱隔离** | 燃料+内存+模块隔离的安全多租户执行 | ✅ 生产 |
| **DAG 执行引擎** | 拓扑排序 + JoinSet 并行的工作流引擎 | ✅ 生产 |
| **递归认知循环** | Predict→Execute→Reflect→Evolve 自动优化闭环 | ✅ 生产 |
| **Composite 编排** | 跨 DAG 依赖解析 + 数据映射 | ✅ 生产 |
| **目标→DAG 合成** | 自然语言目标自动转换为可执行 DAG | ✅ 生产 |
| **自适应燃料调价** | 基于历史/偏差动态调整执行资源 | ✅ 生产 |
| **双层状态存储** | L1 DashMap + L2 SQLite + 快照/恢复 | ✅ 生产 |
| **Prometheus 指标** | 认知迭代/WASM/内存操作全仪表化 | ✅ 生产 |
| **断路器模式** | 熔断降级保护 LLM 调用 | ✅ 生产 |
| **安全审计** | 威胁建模/漏洞报告/攻击向量分析 | ✅ 生产 |
| **策略路线图** | 项目→里程碑→Phase→Goal 层级管理+自主治理 | ✅ 生产 |
| **Rust 性能** | 零成本抽象、无 GC、WASM 编译 | ✅ 原生优势 |
| **自动修复** | 编译器错误自动修复管线 | ✅ 生产 |
| **元学习** | 跨 Session 策略学习与最优策略选择 | ✅ 生产 |

---

## 5. 功能缺口分析

### 5.1 严重缺口（建议优先解决）

| # | 功能 | 影响 | 预估工作量 |
|---|------|------|-----------|
| 1 | **自定义工具**（TS/Zod → .opencode/tools/） | 用户无法扩展工具集，需重新编译 | 3-4 周 |
| 2 | **Web 界面**（`opencode web`） | 缺少浏览器入口 | 4-6 周 |
| 3 | **Headless HTTP API**（REST/OpenAPI vs 纯 gRPC） | 第三方集成受限 | 2-3 周 |
| 4 | **Session 导出/导入** | 无法跨机器迁移会话 | 1 周 |
| 5 | **Session 分享**（公开链接） | 无法分享对话上下文 | 2 周 |

### 5.2 中等缺口（可后续补齐）

| # | 功能 | 影响 | 预估工作量 |
|---|------|------|-----------|
| 6 | **智慧积累**（.omo/notepads/） | 跨 Session 经验传承 | 2 周 |
| 7 | **Session 连续性**（boulder.json） | 中断后精确恢复 | 1 周 |
| 8 | **PR 操作**（checkout + review） | GitHub 工作流集成 | 2 周 |
| 9 | **桌面应用** | 原生桌面体验 | 6-8 周 |
| 10 | **自动上下文压缩** | 长 Session token 管理 | 2 周 |

### 5.3 低优先级缺口

| # | 功能 | 影响 | 预估工作量 |
|---|------|------|-----------|
| 11 | TUI 模糊搜索 / @file 引用 | 体验优化 | 1 周 |
| 12 | OAuth MCP 认证 | 远程 MCP 安全 | 1 周 |
| 13 | 配置变量替换 `{env:VAR}` | 配置灵活性 | 3 天 |
| 14 | Hashline 编辑防冲突 | 编辑安全性 | 2 周 |
| 15 | 75+ LLM 提供商 | 模型选择广度 | 2 周 |
| 16 | GitHub Action 集成 | CI/CD | 1 周 |

---

## 6. 替代性评估表

### 6.1 日常开发场景

| 场景 | opencode | OMO | opencodepro | 可替代？ |
|------|----------|-----|-------------|----------|
| **编码辅助** | ✅ 完整 | — | ✅ 完整 | ✅ |
| **代码搜索** | ✅ grep/glob | ✅ Explore | ✅ Explore | ✅ |
| **文件编辑** | ✅ edit/write/patch | — | ✅ 完整 | ✅ |
| **Bash 操作** | ✅ bash | — | ✅ bash.rs | ✅ |
| **代码理解** | ✅ LSP | — | ✅ LSP（更全） | ✅ |
| **会话管理** | ✅ session | — | ✅ session.rs | ⚠️ 无导出/分享 |
| **任务追踪** | ✅ todowrite | — | ✅ todo.rs | ✅ |
| **权限控制** | ✅ permissions | — | ✅ permissions | ⚠️ 无外部目录守卫 |
| **模型切换** | ✅ 75+ providers | — | ✅ 主流提供商 | ⚠️ 提供商较少 |
| **Web 搜索** | ✅ websearch | — | ✅ web.rs | ✅ |
| **MCP 工具** | ✅ MCP servers | — | ✅ mcp/ | ✅ |
| **PR 工作流** | ✅ pr check | — | ❌ 无 | ❌ |

### 6.2 高级编排场景

| 场景 | opencode | OMO | opencodepro | 可替代？ |
|------|----------|-----|-------------|----------|
| **规划-执行管线** | — | ✅ Prometheus→Momus→Atlas | ✅ metis→plan→momus→delegate | ✅ |
| **多 Agent 团队** | — | ✅ Team Mode | ✅ `team/` | ✅ |
| **代码评审** | — | ✅ Momus | ✅ momus.rs | ✅ |
| **架构咨询** | — | ✅ Oracle | ✅ oracle.rs | ✅ |
| **技术调研** | — | ✅ Librarian | ✅ librarian.rs | ✅ |
| **认知循环** | — | ✅ Ralph/Ultrawork Loop | ✅ ralph/ulw + cognitive loop | ✅ 更成熟 |
| **技能扩展** | ✅ SKILL.md | ✅ skill system | ✅ skill.rs | ✅ |
| **生命周期钩子** | ✅ plugins | ✅ 54+ hooks | ✅ hook.rs | ✅ |
| **自我迭代** | — | ✅ ulw-loop | ✅ 递归认知循环 | ✅ **超越** |

### 6.3 独有场景（Sisyphus 仅有的）

| 场景 | 说明 |
|------|------|
| **WASM 沙箱工具** | 在安全沙箱中运行第三方代码 |
| **故障自动修复** | `auto_fix.rs` — 编译器错误自动修复 |
| **安全审计** | `security.rs` — 威胁建模 + 漏洞报告 |
| **策略路线图** | 长期项目规划、里程碑追踪、自主治理 |
| **元学习** | 跨 Session 策略选择优化 |
| **断路器** | LLM 调用自动熔断降级 |
| **Prometheus 指标** | 全系统性能仪表化 |

---

## 7. 综合评估

### 替代度：~92%

| 维度 | 替代度 | 说明 |
|------|--------|------|
| **Opencode 基础平台** | ~90% | 6 项边缘功能未实现（导出/分享/Web UI/自定义工具/PR/GitHub Action） |
| **OMO Agent编排** | ~95% | 2 项未实现（智慧积累/boulder.json），部分 Agent 命名不同但功能覆盖 |
| **Sisyphus 独有优势** | +30% | opencode + OMO 完全不具备的差异化能力 |
| **综合** | **~92%** | 核心开发流程完全可替代，边缘功能不影响日常使用 |

### 关键结论

1. **核心功能完全覆盖**: Code 开发所需的核心工具（文件、Bash、LSP、搜索、MCP、Web、Session、Todo、Permission）全部实现，与 opencode 等价或超越。

2. **Agent 编排全面等效**: OMO 的 11 个专业 Agent 中 9 个有直接实现，剩余 2 个（Prometheus/Atlas）的功能已分散覆盖在 `plan.rs` + `metis.rs` + `delegate.rs` + `todo.rs` 中。

3. **独有优势显著**: WASM 沙箱、DAG 引擎、递归认知循环、策略路线图等是 opencode + OMO 完全不具备的能力，构成了 opencodepro 的核心差异化。

4. **真实缺口仅 6 项**: 认真审视后，真正影响使用的缺口只有：
   - **自定义工具扩展** — 无法像 opencode 那样写 TS 文件注册新工具
   - **HTTP API** — 仅 gRPC，无 REST/OpenAPI
   - **Session 导出/分享** — 无法跨机器/跨用户共享上下文

5. **✅ 可以替代**: 对于日常开发使用、团队协作、CI/CD 场景，opencodepro 已具备完全替代 opencode + OMO 的能力。剩余缺口是"nice-to-have"而非"must-have"。

### 建议的补齐路线

```
Phase 1 (短期):
  ├── Session 导出/导入       — 1 周
  ├── Session 连续性 (boulder) — 1 周
  └── 智慧积累 (notepads)     — 2 周

Phase 2 (中期):
  ├── 自定义工具 (TS/Zod)     — 3-4 周
  ├── HTTP API (REST)          — 2-3 周
  └── PR 工作流                — 2 周

Phase 3 (远期):
  ├── Web 界面                 — 4-6 周
  ├── 桌面应用                 — 6-8 周
  └── 75+ 提供商               — 2 周
```
