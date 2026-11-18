# Project Context: Sisyphus (OpenCodePro)

## 🚀 项目概览

Sisyphus 是一个**认知计算引擎 (Cognitive Computation Engine)**，实现了从宏观战略规划 (`Roadmap`) 到微观目标执行 (`Goal-Executor`) 的完整闭环。核心是 DAG-based 认知循环（predict → execute → reflect → evolve），支持自适应认知与动态修正。

## 🎯 当前状态

- **Workspace**: 27 个成员（`crates/` 下 25 个 crate + `webui` + `evals`，`sisyphus-tools` 拆分为 10 个子 crate）
- **测试基线**: 全绿（计数随演进取值漂移，以 `make test-all` / CI 实跑输出为准；需真实 LLM/网络的用例单独标记）
- **核心能力**:
  - 认知循环 v1+v2（自适应深度控制、并行分支预测、运行时异常检测）
  - WASM 沙箱（wasmtime + 燃料计量 + 内存限制）双模式：嵌入式 + sandboxd 守护进程
  - gRPC bridge + REST API（OpenAPI 3.1；端点计数以生成物为准）
  - 双层存储（L1 LRU + L2 SQLite/PostgreSQL + pgvector 语义检索）
  - 混沌工程（故障注入 + 自动修复管线）
  - 自编程闭环（缺口检测 → 工具生成 → 部署 → 注册）
  - 多租户隔离（workspace-aware，基于 project_id）
- **生态**:
  - CLI (40+ 子命令) + TUI (ratatui)
  - Web Dashboard (axum + HTMX + SSE)
  - SDK: TypeScript / Python / Go（Rust SDK 已归档至 `ide/sdk/`）
  - IDE 插件: VS Code + Zed
  - Docker + Nightly CI + 自动 Release
- **当前阶段**: 维护与演进模式。基础设施完备，聚焦稳定性与生态深化。

## 🛠️ 开发标准

本项目严格遵循 `DEVELOPMENT_STANDARD.md` 定义的 AI-Native 工作流：

- **强制路径**: `上下文同步` → `设计先导` → `TDD 编码` → `自动化审计` → `资产更新`
- **质量门禁**: pre-commit 跑 `cargo fmt --check` → `cargo clippy -D warnings` → `cargo check-all`；CI 额外跑完整测试与审计脚本
- **架构约束**: 重大变更必须创建新的 `ADR` 文档（见 `docs/adr/`）
- **错误处理**: `thiserror` + 明确 Error enum，生产代码禁止 `unwrap()`/`expect()`

## 📌 关键路径

| 模块 | 路径 |
|------|------|
| 认知循环核心 | `crates/sisyphus-core/src/` (cognitive_loop*.rs, strategic_manager/) |
| gRPC 桥接 | `crates/sisyphus-bridge/src/` (grpc_handlers.rs, bridge_service.rs, http/) |
| TUI | `crates/sisyphus-cli/src/tui/` |
| 存储层 | `crates/sisyphus-mem/src/` (store.rs, vector_memory.rs) |
| REST 路由 | `crates/sisyphus-bridge/src/http/` (mod.rs 注册, handlers*.rs 实现) |
| 战略定义 | `docs/ROADMAP.md` |
| 决策记录 | `docs/adr/`（当前 21 份，编号至 021-quality-gate-degradation-audit） |
| 演进计划 | `docs/EVOLUTION_PLAN.md` |

## 🧪 常用命令

```bash
make check-all            # 快速检查 (排除重型 crate)
make test-ci              # 完整 CI 模拟
cargo test --workspace    # 默认成员全量测试（计数以实跑输出为准）
cargo test -p sisyphus-bridge --test bridge_integration_test  # bridge 集成测试
make bench-core           # 认知循环基准
```
