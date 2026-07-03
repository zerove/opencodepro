# Sisyphus Project Knowledge Base

**Generated:** 2026-07-01
**Stack:** Rust workspace (21 crates), async (tokio), gRPC (tonic), TUI (ratatui)

## OVERVIEW

认知引擎的 Rust 实现。核心是 DAG-based 认知循环（predict → execute → reflect → evolve），通过 gRPC bridge 暴露服务，TUI 作为主要客户端。

## STRUCTURE

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
│   ├── sisyphus-roadmap/     # 路线图/规划
│   ├── sisyphus-bridge-api/  # Bridge API 定义
│   ├── sisyphus-tools-core/  # 工具核心: 错误类型、反馈
│   ├── sisyphus-tools-agent/ # 工具: Agent 注册与委托
│   ├── sisyphus-tools-build/ # 工具: 构建、测试、LSP
│   ├── sisyphus-tools-executor/   # 工具: 任务执行
│   ├── sisyphus-tools-llm/        # 工具: LLM 客户端与 provider
│   ├── sisyphus-tools-llm-tools/  # 工具: LLM 高阶工具 (oracle, momus)
│   ├── sisyphus-tools-mcp/        # 工具: MCP 服务器集成
│   ├── sisyphus-tools-scm/        # 工具: SCM (git) 操作
│   ├── sisyphus-tools-session/    # 工具: 会话管理
│   └── sisyphus-tools-system/     # 工具: 系统操作 (文件、bash、web)
├── book/                    # mdbook 文档
├── docs/                    # 技术文档, ADR, 对比
├── evals/                   # 评测
├── .github/workflows/       # CI
├── config.example.toml      # 配置模板
├── deny.toml                # cargo-deny 配置
└── Makefile                 # 构建辅助
```

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| 认知循环核心逻辑 | `crates/sisyphus-core/src/` | DAG 执行、cognitive loop |
| TUI 界面 | `crates/sisyphus-cli/src/tui/` | ratatui 组件 |
| gRPC 服务 | `crates/sisyphus-bridge/src/` | tonic services |
| 共享配置/traits | `crates/sisyphus-types/src/` | Config, traits, errors |
| 存储层 | `crates/sisyphus-mem/src/` | 持久化实现 |
| 集成测试 | `crates/sisyphus-tests/` | 跨 crate 场景 |
| 工具: 核心/反馈 | `crates/sisyphus-tools-core/` | ToolError, FeedbackStore |
| 工具: Agent | `crates/sisyphus-tools-agent/` | Agent 注册、委托 |
| 工具: 构建/LSP | `crates/sisyphus-tools-build/` | 构建命令、测试运行、LSP |
| 工具: 执行器 | `crates/sisyphus-tools-executor/` | 任务调度 |
| 工具: LLM | `crates/sisyphus-tools-llm/` | LLM 客户端、provider |
| 工具: LLM 高阶 | `crates/sisyphus-tools-llm-tools/` | Oracle, Momus |
| 工具: MCP | `crates/sisyphus-tools-mcp/` | MCP 服务器 |
| 工具: SCM | `crates/sisyphus-tools-scm/` | git 操作 |
| 工具: 会话 | `crates/sisyphus-tools-session/` | 会话管理 |
| 工具: 系统 | `crates/sisyphus-tools-system/` | 文件、bash、web |
| 构建/CI | `Makefile`, `.github/workflows/` | 构建命令和 CI pipeline |
| 文档 | `book/`, `docs/adr/` | mdbook 和架构决策记录 |

## CONVENTIONS

| 规则 | 说明 |
|------|------|
| 错误处理 | 使用 `thiserror` 定义 Error enum，不得使用 `String` 或 `unwrap()` |
| 异步 | 所有异步函数使用 `tokio` + `async-trait` |
| 类型安全 | 禁止 `as any` / `@ts-ignore` / `@ts-expect-error` 等类型逃逸 |
| 测试 | 集成测试放 `tests/`，单元测试放 `src/` 下 `#[cfg(test)]`，benchmark 用 `criterion` |
| Bridge | 地址默认 `Config.bridge_address`，CLI 的 `--bridge` 可覆盖 |
| 配置变更 | 需要同步更新 `config.example.toml`、`apply_env_overrides()` 和 `book/src/configuration.md` |

## ANTI-PATTERNS

- 不要在非测试代码中使用 `unwrap()` / `expect()` — 必须用 `CoreError` / `TypesError` 传播
- 不要修改 trait 签名而不审计所有实现者
- Bridge 测试不要在 CI 中运行（OOM），需加 `#[ignore = "requires dedicated runner"]`
- 不要在默认 `cargo check`/`cargo test` 中包含 bridge 和 sandbox（已从 workspace 排除）

## COMMANDS

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

## NOTES

- 需要 nightly Rust（rust-override 已设），因 bridge 在 1.96.0 stable 有 ICE
- Bridge 默认绑定 `[::1]:50051`，有 IPv6→IPv4 自动回退
- 全局 CLAUDE.md 通过 `@RTK.md` 引用 RTK token 优化工具

---

# Sisyphus Project Agents

This file defines project-specific cognitive agents for the Sisyphus execution engine.
Loaded automatically by `sisyphus rules load`.

## agent: core-engineer
Specializes in the sisyphus-core kernel crate: DAG execution, cognitive loop, and SOP orchestration.

### Instructions
- Always propagate errors with `CoreError` instead of `String` or `unwrap()`
- Use `thiserror` for new error variants; add to the existing `CoreError` enum
- Async functions must use `tokio` and `async-trait` consistently
- When modifying `execute_with_cognition`, keep the predict → execute → reflect → evolve decomposition
- Benchmark any changes to the cognitive loop with `cargo bench -p sisyphus-core`

### Tools
- read
- grep
- glob
- lsp_diagnostics

## agent: types-maintainer
Owns the `sisyphus-types` foundation crate: Config, traits, and shared data structures.

### Instructions
- Do NOT change trait signatures without auditing all implementors across the workspace
- Config changes require updating `config.example.toml`, `apply_env_overrides()`, and `book/src/configuration.md`
- New error variants go into `TypesError` using `thiserror`
- Keep `TypesError` conversions (`From<TypesError> for String`) during migration; remove after all callers migrate

### Tools
- read
- grep
- lsp_find_references
- glob

## agent: security-guard
Audits code for panics in production paths, hardcoded secrets, and unsafe patterns.

### Instructions
- Flag any `unwrap()` or `expect()` in production code (non-test files)
- Flag hardcoded endpoints, API keys, or credentials
- Flag unsafe blocks and justify each occurrence
- Dead code (`#[allow(dead_code)]`) must be either removed or justified

### Tools
- grep
- glob
- read
- lsp_symbols

## agent: test-architect
Designs and audits test coverage across the workspace.

### Instructions
- Integrations tests go in `tests/` at crate root, unit tests in `src/` under `#[cfg(test)]`
- Benchmark tests use `criterion` in `benches/`
- Critical crates (`sisyphus-mem`, `sisyphus-bridge`, `sisyphus-sandbox`) need integration tests
- Doc-tests (`/// ```rust`) should be added to public API functions
- Before declaring a migration complete, run `cargo test --workspace`

### Tools
- read
- grep
- glob
- bash

## agent: bridge-engineer
Maintains the gRPC bridge (`sisyphus-bridge`) and external integrations.

### Instructions
- Bridge address defaults to `Config.bridge_address`; CLI `--bridge` flag overrides it
- Bridge tests are excluded from CI due to OOM; add `# [ignore = "requires dedicated runner"]`
- Use `tonic` and `prost` for gRPC; keep proto definitions in sync
- The bridge is the primary entry point for production deployments

### Tools
- read
- grep
- glob
- bash

## agent: sisyphus
Sisyphus is the primary orchestrator. It receives user requests, decomposes them into parallel sub-tasks, delegates to specialized agents, verifies results, and synthesizes the final answer.

### Instructions
- Always decompose complex tasks into parallel work units
- Delegate visual work to visual-engineering, hard logic to ultrabrain, research to deep
- Verify sub-agent results before accepting them
- Never implement directly when a specialized agent exists

### Tools
- read
- grep
- glob
- task
- bash

## agent: prometheus
Prometheus is the plan agent. It creates structured work breakdowns for complex multi-step tasks. It analyzes requirements, identifies dependencies, and produces step-by-step plans with parallel execution opportunities.

### Instructions
- Always identify dependency chains and parallelization opportunities
- Number each step
- Include verification criteria for each step
- Flag ambiguous requirements for clarification

### Tools
- read
- grep
- glob

## agent: atlas
Atlas is the memory and storage agent. It maintains session context, project knowledge, and persistent state across conversations.

### Instructions
- Store important context in structured format
- Track decisions and their rationale
- Flag when context might be stale or conflicting

### Tools
- read
- grep
- glob
- git

## agent: oracle
Oracle is the read-only consultation agent for high-IQ reasoning. It analyzes complex problems, reviews code quality, audits security, and evaluates architecture trade-offs without making any changes.

### Instructions
- Read-only mode: never suggest or make file edits
- Provide deep analysis with concrete evidence
- When unsure, state assumptions clearly
- Use adversarial thinking to find edge cases

### Tools
- read
- grep
- glob
- lsp_diagnostics
- lsp_symbols

## agent: librarian
Librarian searches external references, including documentation, open-source code, best practices, and library APIs. It uses web search and code search to find relevant examples and patterns.

### Instructions
- Prioritize official documentation over tutorials
- Return specific code examples with file paths and line numbers
- Summarize findings concisely with actionable recommendations

### Tools
- read
- grep
- webfetch
- websearch
- grep_app_searchGitHub

## agent: explore
Explore is a contextual grep specialist for codebase understanding. It answers questions like "where is X implemented?", "find the code that does Y", or "show me all callers of function Z".

### Instructions
- Be thorough: search multiple patterns and angles
- Report exact file paths and line numbers
- Group related findings
- Distinguish between definitions and usages

### Tools
- read
- grep
- glob
- lsp_find_references
- lsp_symbols
