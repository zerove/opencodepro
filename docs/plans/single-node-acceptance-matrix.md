# 单机功能验收矩阵（S4）

> 日期：2026-11-10  
> 依据：`docs/plans/single-node-full-functionality-audit.md`（S1）、`docs/runbooks/single-node-operation-and-verification.md`（S2）、`docs/runbooks/single-node-tools-and-dependencies.md`（S3）  
> 边界：单机能力验收；分布式控制面生产准入（ADR-019 真实网络证据/专用 runner）与控制面启用评估**不计入**单机完成度。

## 验收矩阵

| 领域 | 能力项 | 状态 | 证据 | 备注 |
|---|---|---|---|---|
| 核心引擎 | 认知循环 / DAG / SOP 核心实现与单测 | ✅ 通过 | `cargo test -p sisyphus-core --lib` 116 passed | 端到端 goal 执行依赖 LLM/运行环境配置 |
| Bridge | HTTP/gRPC 服务、路由与契约面 | ✅ 通过（主流程） | 路由与 handler 实现、契约对齐；lib 测试 179 passed；2026-11-10 本地实测 `GET /health`、`/api/sops`、`/api/cluster/status` 返回预期 | `sandboxd_client` 3 项在本环境 Permission denied（环境限制），不阻塞非沙箱主流程 |
| CLI | 本地文件/命令工具 | ✅ 通过 | `read/write/glob/grep/exec` 实测（SN-01） |  |
| CLI | 子命令体系与编译可用性 | ✅ 通过 | `cargo check -p sisyphus-cli`；`sisyphus --help` 可用 | 依赖 bridge/外部服务的子命令需按分层验收 |
| WebUI | 页面、HTMX、契约对齐、只读观测 | ✅ 通过 | `cargo test -p sisyphus-webui` 26 passed；集群页面/HTMX/REST 端点已实现 | 实际对接需运行 bridge 并配置 `WEBUI_BRIDGE_URL` |
| SDK (TS/Py/Go) | 契约对齐与单元测试 | ✅ 通过 | TS 34 passed、Python 38 passed、Go `go test ./...` ok | 最小示例已提供（`docs/examples/single-node-sdk-examples.md`） |
| 状态与观测（只读） | 会话/技能/SOP/认知/集群只读接口与面板 | ✅ 通过（契约与实现） | 路由/handler/测试与文档覆盖 | 端到端数据依赖运行时状态与配置 |
| 工具与 MCP | 工具 crate 与命令面存在 | ✅ 通过（实现层面） | workspace 结构与 CLI 子命令覆盖 | 可用性按“纯本地/需依赖”分层（S3 文档） |
| 文档与运维 | 单机运行/验收/分层/示例文档 | ✅ 通过 | S1 审计、S2 runbook、S3 示例与依赖说明、本矩阵 |  |
| 门禁与质量 | fmt/clippy/check/audit（批次范围） | ✅ 通过 | `cargo check-all`、`cargo fmt --all --check`、`make audit`、相关 clippy |  |

## 状态汇总

- **单机主流程（可在本地/常规开发环境闭环）**：✅ 已具备并有证据支撑。
- **受环境限制（非功能缺失）**：⚠️ sandboxd 相关测试需具备执行权限与环境；真实 LLM 执行需凭据与配置。
- **明确后置（不纳入单机）**：⏸ ADR-019 专用 runner 真实网络证据、控制面生产准入与启用评估（`cluster.control_plane.enabled=false`）。

## 结论（S4）

在“单机全功能”定义与边界下，**单机主流程能力已完成基线盘点、补齐关键文档与验收证据，并通过当前可执行的验证项**。剩余未闭环项主要为**外部依赖/环境受限**或**分布式后置项**，已明确标注，不作为单机完成度的阻塞缺陷。
