# Sisyphus 项目知识库

**生成时间:** 2026-08-10
**技术栈:** Rust 工作区 (27 成员 = crates/ 下 25 个 crate + webui + evals), async (tokio), gRPC (tonic), TUI (ratatui)

## 概述

认知引擎的 Rust 实现。核心是 DAG-based 认知循环（predict → execute → reflect → evolve），通过 gRPC bridge 暴露服务，TUI 作为主要客户端。

## 结构

```
opencodepro/
├── crates/
│   ├── sisyphus-core/        # ★ 核心认知引擎: DAG 执行, 认知循环, SOP 编排
│   ├── sisyphus-cli/         # TUI 客户端 (ratatui + crossterm)
│   ├── sisyphus-bridge/      # gRPC 桥接服务 (tonic + prost)
│   ├── sisyphus-types/       # 共享类型, Config, traits
│   ├── sisyphus-mem/         # 持久化/存储层
│   ├── sisyphus-intel/       # 情报分析模块
│   ├── sisyphus-metrics/     # 指标收集
│   ├── sisyphus-sandbox/     # 沙箱执行 (wasmtime)
│   ├── sisyphus-tests/       # 集成测试
│   ├── sisyphus-tools-core/  # 工具核心: 错误类型、反馈
│   ├── sisyphus-tools-agent/ # 工具: Agent 注册与委托
│   ├── sisyphus-tools-build/ # 工具: 构建、测试、LSP
│   ├── sisyphus-tools-executor/   # 工具: 任务执行
│   ├── sisyphus-tools-llm/        # 工具: LLM 客户端与 provider
│   ├── sisyphus-tools-llm-tools/  # 工具: LLM 高阶工具 (oracle, momus)
│   ├── sisyphus-tools-mcp/        # 工具: MCP 服务器集成
│   ├── sisyphus-tools-scm/        # 工具: SCM (git) 操作
│   ├── sisyphus-tools-session/    # 工具: 会话管理
│   ├── sisyphus-tools-system/     # 工具: 系统操作 (文件、bash、web)
│   ├── sisyphus-roadmap/     # 路线图/规划 (Project→Milestone→Phase→Goal)
│   ├── sisyphus-knowledge/   # 知识图谱与认知资产管理
│   ├── sisyphus-visual/      # 视觉感知与嵌入
│   ├── sisyphus-wasm-sdk/    # WASM 工具 SDK (guest crate)
│   ├── sisyphus-sandboxd/    # WASM 沙箱守护进程 (进程隔离)
│   └── sisyphus-bridge-api/  # Bridge API 定义
├── webui/                   # Web 仪表盘 (axum + HTMX + SSE)
├── evals/                   # 评测 (metis/momus/librarian/scribe/security)
├── book/                    # mdbook 文档
├── docs/                    # 技术文档, ADR, 对比
├── .github/workflows/       # CI
├── config.example.toml      # 配置模板
├── deny.toml                # cargo-deny 配置
└── Makefile                 # 构建辅助
```

## 关键模块查询

| 任务 | 位置 | 备注 |
|------|----------|-------|
| 认知循环核心逻辑 | `crates/sisyphus-core/src/` | DAG 执行、cognitive loop v1/v2 |
| TUI 界面 | `crates/sisyphus-cli/src/tui/` | ratatui 组件 |
| gRPC 服务 | `crates/sisyphus-bridge/src/` | tonic services |
| REST API | `crates/sisyphus-bridge/src/http.rs` | axum + OpenAPI 3.1（独立 sisyphus-api 已归档至 ide/api）|
| Web 仪表盘 | `webui/` | axum + HTMX + SSE |
| Rust SDK | `ide/sdk/`（已归档） | REST 客户端；活跃 SDK 见 ide/python-sdk、ide/go-sdk、ide/typescript-sdk |
| TypeScript SDK | `ide/typescript-sdk/` | vitest, npm 包，CI 跑 build + test |
| VSCode 扩展 | `ide/vscode/` | TypeScript 扩展，本地开发 |
| Zed 扩展 | `ide/zed/` | Rust 扩展（lib.rs + extension.toml），本地开发 |
| 已归档 IDE | `ide/api/`, `ide/sdk/`, `ide/tauri/` | 历史方向冻结，不在 CI 范围内 |
| 知识管理 | `crates/sisyphus-knowledge/` | KnowledgeGraph, AssetManager |
| 战略规划 | `crates/sisyphus-roadmap/` | RoadmapManager, 里程碑生成 |
| 沙箱守护进程 | `crates/sisyphus-sandboxd/` | gRPC 通信、进程隔离 |
| 共享配置/traits | `crates/sisyphus-types/src/` | Config, traits, errors |
| 存储层 | `crates/sisyphus-mem/src/` | 持久化实现 |
| 集成测试 | `crates/sisyphus-tests/` | 跨 crate 场景 |
| 工具: 核心/反馈 | `crates/sisyphus-tools-core/` | ToolError, FeedbackStore |
| 工具: Agent | `crates/sisyphus-tools-agent/` | Agent 注册、委托 |
| 工具: 构建/LSP | `crates/sisyphus-tools-build/` | 构建命令、测试运行、LSP |
| 工具: 执行器 | `crates/sisyphus-tools-executor/` | 任务调度 |
| 工具: LLM | `crates/sisyphus-tools-llm/` | LLM 客户端、provider |
| 工具: LLM 高阶 | `crates/sisyphus-tools-llm-tools/` | Oracle, Momus, Metis, Librarian |
| 工具: MCP | `crates/sisyphus-tools-mcp/` | MCP 服务器 |
| 工具: SCM | `crates/sisyphus-tools-scm/` | git 操作 |
| 工具: 会话 | `crates/sisyphus-tools-session/` | 会话管理 |
| 工具: 系统 | `crates/sisyphus-tools-system/` | 文件、bash、web |
| 构建/CI | `Makefile`, `.github/workflows/` | 构建命令和 CI pipeline |
| 文档 | `book/`, `docs/adr/` | mdbook 和架构决策记录 |

## 约定

| 规则 | 说明 |
|------|------|
| 错误处理 | 使用 `thiserror` 定义 Error enum，不得使用 `String` 或 `unwrap()` |
| 异步 | 所有异步函数使用 `tokio` + `async-trait` |
| 类型安全 | 禁止 `as any` / `@ts-ignore` / `@ts-expect-error` 等类型逃逸 |
| 测试 | 集成测试放 `tests/`，单元测试放 `src/` 下 `#[cfg(test)]`，benchmark 用 `criterion` |
| Bridge | 地址默认 `Config.bridge_address`，CLI 的 `--bridge` 可覆盖 |
| 配置变更 | 需要同步更新 `config.example.toml`、`apply_env_overrides()` 和 `book/src/configuration.md` |

## 反模式

- 不要在非测试代码中使用 `unwrap()` / `expect()` — 必须用 `CoreError` / `TypesError` 传播
- 不要修改 trait 签名而不审计所有实现者
- Bridge 测试不要在 CI 中运行（OOM），需加 `#[ignore = "requires dedicated runner"]`
- 不要在默认 `cargo check`/`cargo test` 中包含 bridge 和 sandbox（已从 workspace 排除）

## 常用命令

```bash
cargo check-all          # 检查全部（排除 bridge/sandbox）
cargo build-all          # 构建全部
cargo test-all           # 测试全部
cargo check -p sisyphus-bridge   # 单独检查 bridge
cargo test -p sisyphus-core --lib  # 快速测试 core
make test-core           # 完整 core 测试（含 wasmtime）
cargo build --release -p sisyphus-cli  # 构建 release TUI
cargo bench -p sisyphus-core   # 认知循环 benchmark
```

## 备注

- 工具链：仓库**没有** `rust-toolchain.toml`，不固定版本。实测 stable 与
  nightly 均可编译（含 `sisyphus-bridge`、`sisyphus-sandboxd` 的
  `--all-targets`），代码里也没有 nightly 专属的 `#![feature(...)]` 或
  cargo feature。
- 本机 `rustup override` 指向 `nightly`，但环境变量 `RUSTUP_TOOLCHAIN=1.99.0`
  会覆盖它，因此**实际生效的是 stable `rustc 1.99.0` / `cargo 1.99.0`**。
  排查构建问题前先确认 `rustc --version`，不要假设在用 nightly。
- CI（`.github/workflows/ci.yml`）、`Dockerfile`、`CONTRIBUTING.md` 仍统一
  使用 nightly。若要在 stable 上开发，本地需显式设置 `RUSTUP_TOOLCHAIN`，
  否则会与 CI 的编译器版本不一致。
- **`cargo test-all` 不等于完整 workspace 验收**：该 alias 显式
  `--exclude sisyphus-bridge --exclude sisyphus-sandbox --exclude
  sisyphus-sandboxd --exclude sisyphus-tests`（见 `.cargo/config.toml`），
  这 4 个重型 crate 默认不参与。需单独跑：
  `cargo test -p sisyphus-tests -p sisyphus-sandbox -p sisyphus-sandboxd -p sisyphus-bridge`
- Bridge 默认绑定 `[::1]:50051`，有 IPv6→IPv4 自动回退
- 内部 crate 的路径依赖均带 `version = "0.1.0"`，这是 `cargo package` 的
  硬性要求（缺 version 会报 “all dependencies must have a version
  requirement specified when packaging”）。全部内部 crate 当前都是 0.1.0。
- **当前仍无法完成打包**：manifest 校验已通过，但 `cargo package` 会去
  crates.io 查 `sisyphus-bridge-api` / `sisyphus-metrics` 等内部 crate 而
  失败（“no matching package named ... found”）。仓库没有任何发布流程
  （无 `publish = false`、无 `cargo publish`、CI 也不打包），因此这些
  crate 实际是**仅本地**使用。若将来要发布，必须先整体发布内部 crate，
  或改用 workspace 依赖 + 私有 registry。
- Release 构建已验证：`cargo build --release -p sisyphus-bridge -p sisyphus-sandboxd`
  均成功，产物可执行（`sisyphus-sandboxd --help` 正常）。
- 全局 CLAUDE.md 通过 `@RTK.md` 引用 RTK token 优化工具

---

# Sisyphus 项目代理 (Agents)

本文件定义了 Sisyphus 执行引擎的项目特定认知代理。
由 `sisyphus rules load` 自动加载。

## agent: core-engineer (核心工程师)
专注于 `sisyphus-core` 内核 crate：DAG 执行、认知循环和 SOP 编排。

### 指令
- 始终使用 `CoreError` 传播错误，不得使用 `String` 或 `unwrap()`
- 为新的错误变体使用 `thiserror`；添加到现有的 `CoreError` 枚举中
- 异步函数必须一致地使用 `tokio` 和 `async-trait`
- 修改 `execute_with_cognition` 时，保持 “预测 → 执行 → 反思 → 演进” 的分解结构
- 使用 `cargo bench -p sisyphus-core` 对认知循环的任何更改进行基准测试

### 工具
- read
- grep
- glob
- lsp_diagnostics

## agent: types-maintainer (类型维护员)
负责 `sisyphus-types` 基础 crate：配置 (Config)、trait 和共享数据结构。

### 指令
- 未经审计所有工作区实现者，严禁更改 trait 签名
- 配置变更需要同步更新 `config.example.toml`、`apply_env_overrides()` 和 `book/src/configuration.md`
- 新的错误变体使用 `thiserror` 添加到 `TypesError` 中
- 在迁移期间保留 `TypesError` 转换 (`From<TypesError> for String`)；在所有调用者迁移后移除

### 工具
- read
- grep
- lsp_find_references
- glob

## agent: security-guard (安全卫士)
审计生产路径中的 panic、硬编码密钥和不安全模式。

### 指令
- 标记生产代码（非测试文件）中任何 `unwrap()` 或 `expect()`
- 标记硬编码的端点、API 密钥或凭据
- 标记 `unsafe` 块并为每次出现提供理由
- 死代码 (`#[allow(dead_code)]`) 必须被移除或提供正当理由

### 工具
- grep
- glob
- read
- lsp_symbols

## agent: test-architect (测试架构师)
设计并审计整个工作区的测试覆盖范围。

### 指令
- 集成测试放在 crate 根目录的 `tests/` 中，单元测试放在 `src/` 下的 `#[cfg(test)]` 中
- 基准测试在 `benches/` 中使用 `criterion`
- 关键 crate (`sisyphus-mem`, `sisyphus-bridge`, `sisyphus-sandbox`) 需要集成测试
- 为公共 API 函数添加文档测试 (`/// ```rust`)
- 在宣布迁移完成之前，运行 `cargo test --workspace`

### 工具
- read
- grep
- glob
- bash

## agent: bridge-engineer (桥接工程师)
维护 gRPC 桥接服务 (`sisyphus-bridge`) 和外部集成。

### 指令
- 桥接地址默认为 `Config.bridge_address`；CLI 的 `--bridge` 标志可覆盖该地址
- 由于 OOM 内存问题，桥接测试被排除在 CI 之外；请添加 `# [ignore = "requires dedicated runner"]`
- 使用 `tonic` 和 `prost` 实现 gRPC；保持 proto 定义同步
- 桥接服务是生产部署的主要入口点

### 工具
- read
- grep
- glob
- bash

## agent: sisyphus (西西弗斯)
主编排者。接收用户请求，将其分解为并行子任务，委派给专业代理，验证结果并合成最终答案。

### 指令
- 始终将复杂任务分解为并行工作单元
- 将视觉工作委派给 visual-engineering，硬逻辑交给 ultrabrain，调研交给 deep
- 在接受子代理结果之前先进行验证
- 当存在专业代理时，绝不要直接实现

### 工具
- read
- grep
- glob
- task
- bash

## agent: prometheus (普罗米修斯)
计划代理。为复杂的多步任务创建结构化的工作分解。分析需求，识别依赖关系，并生成包含并行执行机会的分步计划。

### 指令
- 始终识别依赖链和并行化机会
- 为每个步骤编号
- 为每个步骤包含验证标准
- 标记不明确的需求以寻求澄清

### 工具
- read
- grep
- glob

## agent: atlas (阿特拉斯)
内存和存储代理。维护会话上下文、项目知识以及跨对话的持久状态。

### 指令
- 以结构化格式存储重要上下文
- 追踪决策及其理由
- 当上下文可能过时或冲突时发出标记

### 工具
- read
- grep
- glob
- git

## agent: oracle (神谕)
用于高智商推理的只读咨询代理。分析复杂问题，审查代码质量，审计安全性，并在不做出任何更改的情况下评估架构权衡。

### 指令
- 只读模式：绝不建议或进行文件编辑
- 提供基于具体证据的深度分析
- 不确定时，清晰地陈述假设
- 使用对抗性思维寻找边缘情况

### 工具
- read
- grep
- glob
- lsp_diagnostics
- lsp_symbols

## agent: librarian (图书管理员)
搜索外部参考资料，包括文档、开源代码、最佳实践和库 API。使用网络搜索和代码搜索来寻找相关的示例和模式。

### 指令
- 优先考虑官方文档而非教程
- 返回带有文件路径和行号的具体代码示例
- 用简洁且具有可操作性的建议总结发现

### 工具
- read
- grep
- webfetch
- websearch
- grep_app_searchGitHub

## agent: explore (探索者)
代码库理解的上下文 grep 专家。回答诸如“X 在哪里实现？”、“找到执行 Y 的代码”或“显示函数 Z 的所有调用者”之类的问题。

### 指令
- 保持彻底：搜索多种模式和角度
- 报告准确的文件路径和行号
- 将相关的发现分组
- 区分定义和用法

### 工具
- read
- grep
- glob
- lsp_find_references
- lsp_symbols
