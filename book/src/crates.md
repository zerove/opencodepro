# Crate 参考

Sisyphus 以 Rust 工作区组织，包含 37 个成员：`crates/` 下的 35 个 crate（含 10 个 `sisyphus-tools-*` 子 crate）加 `webui` 与 `evals`。

## 基础层

| Crate | 说明 | 依赖 |
|-------|-------------|-------------|
| [sisyphus-types](crates/types.md) | 核心数据结构：SOP 图、执行上下文、配置 | 无（独立） |
| [sisyphus-metrics](crates/metrics.md) | Prometheus/OpenTelemetry 可观测性埋点 | types |
| sisyphus-bridge-api | gRPC API 定义（prost/tonic） | types |

## 认知层

| Crate | 说明 | 依赖 |
|-------|-------------|-------------|
| [sisyphus-core](crates/core.md) | 内核：DAG 执行器、认知循环 v1/v2、编排 | types, intel, mem, sandbox, tools, metrics |
| [sisyphus-intel](crates/intel.md) | 认知：模拟、反思、SOP 演进、形式化验证 | types, metrics |
| sisyphus-roadmap | 战略规划：项目 → 里程碑 → 阶段 → 目标 | types, intel, mem, metrics |

## 存储层

| Crate | 说明 | 依赖 |
|-------|-------------|-------------|
| [sisyphus-mem](crates/mem.md) | 多级缓存（L1 DashMap + L2 SQLite/PostgreSQL + pgvector） | types, metrics |
| [sisyphus-knowledge](crates/knowledge.md) | 知识图谱与认知资产管理 | types, mem |
| [sisyphus-visual](crates/visual.md) | 视觉感知与嵌入 | types, mem |

## 工具层（10 个子 crate）

| Crate | 说明 | 依赖 |
|-------|-------------|-------------|
| sisyphus-tools-core | 工具核心基础设施（错误、反馈、钩子） | types, metrics |
| sisyphus-tools-llm | LLM 客户端抽象（多提供商、预算、熔断） | types, metrics |
| sisyphus-tools-llm-tools | 高阶 LLM 工具（librarian、oracle、momus、metis、scribe） | types, llm, intel |
| sisyphus-tools-agent | 代理注册、委托、团队编排 | types, llm, intel |
| sisyphus-tools-build | 构建/测试/LSP 工具 | types, llm |
| sisyphus-tools-system | 文件、bash、web、探索、权限、脚手架工具 | types |
| sisyphus-tools-mcp | MCP 服务器集成（stdio/HTTP） | types |
| sisyphus-tools-session | 会话与待办管理 | types |
| sisyphus-tools-executor | 任务执行与委托 | types, llm |
| sisyphus-tools-scm | SCM（git/GitHub）操作 | types |

## 沙箱层

| Crate | 说明 | 依赖 |
|-------|-------------|-------------|
| [sisyphus-sandbox](crates/sandbox.md) | 工具执行的 WASM 沙箱运行时（wasmtime） | types, metrics |
| sisyphus-sandboxd | 独立的 WASM 沙箱守护进程（stdio/gRPC 双模式） | sandbox |
| [sisyphus-wasm-sdk](crates/wasm-sdk.md) | WASM 工具 SDK（guest crate） | types |

## 接口层

| Crate | 说明 | 依赖 |
|-------|-------------|-------------|
| [sisyphus-bridge](crates/bridge.md) | HTTP/gRPC API 服务器、入口点 | core, tools（全部传递依赖） |
| sisyphus-api | REST API 处理器（axum + OpenAPI 3.1） | bridge, mem, types |
| [sisyphus-cli](crates/cli.md) | 命令行界面 + 带代理子命令的 TUI | types, tools（10 个子 crate） |
| sisyphus-sdk | REST API 的 Rust SDK 客户端 | api |
| sisyphus-tauri | Tauri 桌面应用（已归档） | api, types |

## 测试与评测

| Crate | 说明 | 依赖 |
|-------|-------------|-------------|
| sisyphus-tests | 跨 crate 集成测试 | 全部核心 crate |
| evals | 代理能力评测（metis/momus/librarian/scribe/security） | types, llm |

## Crate 依赖图

```
sisyphus-bridge → sisyphus-core
                    ├── sisyphus-types
                    ├── sisyphus-intel → sisyphus-types, sisyphus-metrics
                    ├── sisyphus-mem   → sisyphus-types, sisyphus-metrics
                    ├── sisyphus-sandbox → sisyphus-types, sisyphus-metrics
                    ├── sisyphus-sandboxd → sisyphus-sandbox
                    ├── sisyphus-roadmap → sisyphus-types, sisyphus-intel, sisyphus-mem, sisyphus-metrics
                    ├── sisyphus-knowledge → sisyphus-types, sisyphus-mem
                    ├── sisyphus-visual → sisyphus-types, sisyphus-mem
                    ├── sisyphus-tools-{10 sub-crates} → sisyphus-types
                    ├── sisyphus-cli   → sisyphus-types, sisyphus-tools-*
                    ├── sisyphus-bridge-api → sisyphus-types
                    ├── sisyphus-api   → sisyphus-bridge, sisyphus-mem, sisyphus-types
                    ├── sisyphus-sdk   → sisyphus-api
                    ├── sisyphus-wasm-sdk → sisyphus-types
                    ├── sisyphus-webui → sisyphus-bridge, sisyphus-types
                    └── sisyphus-metrics → sisyphus-types
```