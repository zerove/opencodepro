# 单机全功能基线盘点与缺口清单（S1）

> 日期：2026-11-10  
> 目标：以“认知引擎平台”为主线，在单节点环境下完成可闭环、可验证的全功能能力基线盘点，并给出后续 S2–S4 实施清单。  
> 边界：**不包含**分布式控制面生产准入/启用（ADR-019 真实网络证据、专用 runner）、跨节点强一致写入；`cluster.control_plane.enabled` 保持 `false`；遵循 ADR-020（REST 写入=否）。

## 1. 范围定义（单机全功能）

| 领域 | 单机完成标准 |
|---|---|
| 核心引擎 | 认知循环（predict→execute→reflect→evolve）、DAG 执行、SOP 编排在单机可稳定运行 |
| 运行与接入 | CLI/TUI、bridge（gRPC+REST）、WebUI、三套 SDK 在单机可端到端使用，契约一致 |
| 工具与能力 | 本地工具链（tools-*：系统/文件/bash/SCM/构建/会话/LLM/MCP 等）在单机场景可用且行为可解释 |
| 状态与观测 | 会话/记忆/知识等本地状态管理可用；只读观测在单机场景可用 |
| 质量与交付 | 测试/门禁/文档/示例覆盖关键单机路径；配置与文档同步 |

## 2. 基线验证证据（2026-11-10，本环境）

| 检查项 | 命令/依据 | 结果 |
|---|---|---|
| WebUI 测试 | `cargo test -p sisyphus-webui` | ✅ 26 passed（含 cluster REST/HTMX/页面路由） |
| Core lib 测试 | `cargo test -p sisyphus-core --lib` | ✅ 116 passed |
| CLI 编译与入口 | `cargo check -p sisyphus-cli`；`sisyphus --help` | ✅ 可编译，子命令丰富（文件/会话/SOP/MCP/工具等） |
| Bridge lib（no-default-features） | `cargo test -p sisyphus-bridge --lib --no-default-features` | ⚠️ 179 passed / 3 failed / 3 ignored；失败 3 项均为 `sandboxd_client::tests::*`（`Failed to spawn sandboxd: Permission denied (os error 13)`），属环境限制，非本批次回归 |
| SDK 契约与测试（本批次已完成） | `go test ./...`、`python -m pytest tests/`、`npm run build && npm test` | ✅ Go ok / Python 38 passed / TS 34 passed（建议二批次收口） |
| 门禁 | `cargo check-all`、`cargo fmt --all --check`、`make audit`、`cargo clippy`（相关 crate） | ✅ 批次内持续全绿（以最近提交为准） |
| 单机可执行产物 | `sisyphus`(cli)、`sisyphus-bridge`、`sisyphus-webui`、`sisyphus-sandboxd` 等 | ✅ workspace 定义存在（`cargo metadata`） |

## 3. 能力矩阵（单机视角）

### 3.1 核心引擎（sisyphus-core / sop / dag）
- **现状**：认知循环、执行管理、SOP/DAG 与目标执行路径存在（`execute_goal`/`execute_goals*`、cognitive trace、SOP 存储等）；core lib 测试通过。
- **单机就绪度**：**高**（核心能力已实现并有测试覆盖）。
- **缺口**：需在 S2 做“端到端 SOP 执行”单机验收脚本/步骤文档化（从 bridge/CLI 触发到结果可观测）。

### 3.2 Bridge（gRPC + REST，单节点）
- **现状**：HTTP 路由覆盖健康、OpenAPI、会话、SOP、执行（`/api/execute`、`/api/execute/goal`）、技能、认知图/追踪、集群只读状态（`/api/cluster/status`、`/api/cluster/control/leases/{key}`）等；OpenAPI 生成与门禁存在。
- **单机就绪度**：**高**（接口面完整；本批次已修复 SDK/WebUI 契约对齐）。
- **缺口**：
  - `sandboxd_client` 相关单测在本环境受限（需在具备 sandboxd 执行条件的环境验证或保持 ignore/环境说明）；不影响非 sandboxd 的单机 REST 主流程。
  - 需补一份“单机最小启动与验证”步骤（bridge + 可选 webui）并纳入文档。

### 3.3 CLI / TUI（sisyphus-cli）
- **现状**：子命令覆盖文件操作、会话/计划、SOP、MCP、工具、构建/git/诊断等；`--help` 可用，可编译。
- **单机就绪度**：**中高**（功能面广，但“单机全功能”需按关键路径逐项验收）。
- **缺口（S2）**：
  - 关键本地工作流验收：`read/write/edit/glob/grep/exec`、`session`、`sop`（经 bridge）、`mcp`/`skill` 基本可用性。
  - TUI/交互模式在无外部依赖时的可用性说明与冒烟步骤。

### 3.4 WebUI（axum + HTMX）
- **现状**：页面（仪表盘/会话/DAG/SOP/技能/集群/混沌/认知地图等）、HTMX 端点、REST 代理与只读观测已实现并有测试（26 passed）；契约已与 bridge 对齐。
- **单机就绪度**：**高**。
- **缺口**：补充“单机启动 WebUI 对接本地 bridge”的操作文档与已知前置（`WEBUI_BRIDGE_URL` 等）的一致性检查。

### 3.5 SDK（TS / Python / Go）
- **现状**：契约修复与只读观测方法已完成，三端测试通过；文档已更新。
- **单机就绪度**：**高**。
- **缺口**：提供最小可运行示例（对接本地 bridge）并在文档中固化（S3）。

### 3.6 工具链与 MCP（tools-*）
- **现状**：工具 crate 覆盖系统/构建/LLM/MCP/SCM/会话/执行器等；CLI 暴露相关命令。
- **单机就绪度**：**中**（实现广，但依赖配置/外部服务的能力需分层验收）。
- **缺口（S2/S3）**：区分“纯本地可用”（文件/bash/git 等）与“需外部依赖”（LLM provider、远程 MCP）的清单，并给出单机验收步骤与前置条件。

### 3.7 状态与存储（mem / knowledge / sessions）
- **现状**：`sisyphus-mem`（SQLite/经验/向量/技能/SOP 存储等）、`sisyphus-knowledge`、会话相关能力存在；bridge 提供会话与认知追踪接口。
- **单机就绪度**：**中高**。
- **缺口**：单机会话生命周期（创建→消息→查询→持久化）端到端验收与文档化。

### 3.8 观测与运维（单机）
- **现状**：健康检查、指标、认知追踪/地图、集群状态只读接口与 WebUI 面板可用。
- **单机就绪度**：**高**（只读观测面已具备）。
- **缺口**：汇总单机观测入口（端点/页面/命令）到一处运维说明。

## 4. 缺口优先级与 S2–S4 计划

### S2（核心闭环补齐，优先）
1. **单机最小运行手册**：bridge 启动（配置/环境变量要点）→ 健康检查 → CLI/WebUI/SDK 最小调用链路；补充到 `book/` 或 `docs/` 并与现有文档对齐。
2. **端到端验收清单（可执行步骤）**：SOP 列表/详情/执行（goal）、会话 CRUD、技能列表、只读观测；记录实际验证结果。
3. **CLI 关键路径冒烟**：本地文件工具 + 会话/SOP（对接本地 bridge）最小命令序列验证。
4. **环境受限项标注**：`sandboxd` 相关测试/能力在受限环境的处理说明（不阻塞非 sandboxd 单机主流程）。

### S3（可用性与完善）
1. **SDK 最小示例**：TS/Python/Go 各 1 个对接本地 bridge 的最小示例（健康检查/SOP 列表/执行），纳入文档。
2. **工具/MCP 分层说明**：纯本地 vs 需外部依赖能力矩阵与前置条件。
3. **错误与空态体验**：WebUI/CLI 在 bridge 不可达或未配置依赖时的提示一致性（按需小步改进）。

### S4（验收与冻结）
1. **单机功能矩阵验收表**：按领域逐项标记通过/阻塞/已知限制（附证据）。
2. **与分布式项分离**：明确列出“需专用 runner/真实网络”的后置项（ADR-019 准入等），不计入单机未完成。

## 5. 已知限制（当前不纳入单机完成度）

- 专用 runner 真实网络证据矩阵与 ADR-019 生产准入（外部硬件前提）。
- 控制面生产启用评估（默认 `cluster.control_plane.enabled=false`）。
- 依赖外部 LLM/凭据/远程服务的能力：单机“功能存在”可验收，但端到端效果取决于环境配置（需在文档中明确）。

## 6. 下一步（立即执行）

- 执行 **S2-1/S2-2**：产出单机最小运行与端到端验收文档，并进行一次可复现的本地验证（在可行范围内）；同步更新相关 book/文档索引。
- 将本审计作为“单机全功能”推进的基线，后续迭代按缺口项逐条闭环并附证据。
