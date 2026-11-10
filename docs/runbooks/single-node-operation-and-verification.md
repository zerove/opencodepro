# 单机运行与验收 Runbook（S2）

> 目标：在单节点环境下完成 Sisyphus 的最小启动、关键路径验收与证据记录。  
> 边界：不涉及分布式控制面生产启用（`cluster.control_plane.enabled=false`），不依赖专用 runner 真实网络。

## 1. 前置条件

- Rust 工具链：按仓库 `rust-toolchain.toml`（nightly-2026-08-08）；建议 `env -u RUSTUP_TOOLCHAIN` 运行 cargo 命令以避免本机覆盖。
- 可选依赖（按需）：LLM provider 凭据（用于真实 SOP/LLM 执行）、Node/Go/Python（SDK 验证）。
- 端口：默认 gRPC `[::1]:50051`（回退 IPv4）、HTTP `[::1]:8080`；WebUI 默认 `0.0.0.0:3000`。按需调整避免冲突。

## 2. 配置要点（单机）

配置文件由 `SISYPHUS_CONFIG` 指定（默认 `config.toml`）。关键字段（**顶层**，非 `[bridge]` 子表）：

```toml
# gRPC 监听地址（bridge_address）
bridge_address = "http://127.0.0.1:50051"
# HTTP REST 监听地址
http_address = "http://127.0.0.1:8080"

# 可选：关闭/配置集群（单机默认关闭）
[cluster]
enabled = false
```

常用环境变量（示例）：`SISYPHUS_LLM_API_KEY`、`SISYPHUS_LLM_ENDPOINT`、`SISYPHUS_LLM_MODEL`；单机验证嵌入可用 `SISYPHUS_LLM_EMBEDDING_ALLOW_DUMMY=true`（仅非生产语义质量）。

## 3. 最小启动流程

### 3.1 启动 Bridge

```bash
# 使用默认或指定配置
SISYPHUS_CONFIG=./config.toml \
SISYPHUS_LLM_EMBEDDING_ALLOW_DUMMY=true \
cargo run -p sisyphus-bridge --bin sisyphus-bridge
```

预期日志要点：HTTP REST / gRPC 监听地址、Tool registry / Agent registry 初始化完成。  
> 注意：`sisyphus-bridge` 包含多个 bin，需指定 `--bin sisyphus-bridge`。

### 3.2 健康检查（HTTP）

```bash
curl -s http://127.0.0.1:8080/health
# 预期：JSON 健康响应（如 {"status":"ok","version":"..."}）
```

其他探针：`/health/live`、`/health/ready`、`/health/deep`、`/api/metrics`、`/openapi.json`。

### 3.3 启动 WebUI（可选）

```bash
WEBUI_BRIDGE_URL=http://127.0.0.1:8080 \
WEBUI_LISTEN_ADDR=0.0.0.0:3000 \
cargo run -p sisyphus-webui --bin sisyphus-webui
```

浏览器访问 `http://127.0.0.1:3000`；集群只读观测页 `/cluster`，HTMX 面板 `/htmx/cluster/status`、`/htmx/cluster/control-lease`。

### 3.4 CLI 连接 Bridge（可选）

```bash
cargo run -p sisyphus-cli -- --bridge http://127.0.0.1:50051 health
cargo run -p sisyphus-cli -- --bridge http://127.0.0.1:50051 status
```

（具体子命令以 `sisyphus --help` 为准。）

## 4. 端到端验收清单（单机）

| 编号 | 项目 | 步骤 | 预期 | 证据/备注 |
|---|---|---|---|---|
| SN-01 | CLI 本地文件工具 | `read/write/glob/grep/exec` 对本地文件操作 | 输出正确、退出码正确 | ✅ 2026-11-10 已验证（`/tmp/opencode` 下 read/write/glob/grep/exec 均成功） |
| SN-02 | Bridge 健康与基础接口 | 启动 bridge 后 `GET /health`、`/health/live`、`/openapi.json` | 200 与预期结构 | ✅ 2026-11-10 本地实测（正确顶层配置 `bridge_address`/`http_address`）：`GET /health` → `{"status":"SERVING","version":"0.4.0"}` |
| SN-03 | SOP 列表与详情 | `GET /api/sops`、`GET /api/sops/{id}`、`GET /api/sops/{id}/dag`（需工作区头时按约定附加） | 契约与 SDK/WebUI 一致（`string[]`/detail/dag） | ✅ 2026-11-10 本地实测：`GET /api/sops` → `["verify_sop"]`；详情/DAG 按 SOP 与工作区上下文进一步验收 |
| SN-04 | 目标执行（SOP） | `POST /api/execute/goal` `{sop_id, goal}`；或 CLI `sop run <id>`（直接 SOP 路径） | 返回 `{output, report}` 或可解释错误；CLI 路径可直接返回执行结果 | ✅ 2026-11-10 本地实测：CLI `sop run verify_sop` 成功返回 `Test Passed` 与 CognitionReport；`POST /api/execute/goal` 在无可用 LLM 时返回可解释错误（路径差异） |
| SN-05 | 会话生命周期 | `POST /api/sessions` → `GET /api/sessions` → `GET /api/sessions/{id}` → `POST /api/sessions/{id}/messages` | 契约正确、状态可查询 | 按 bridge 实现与 SDK 契约验证 |
| SN-06 | 技能 | `GET /api/skills`、`POST /api/skills/install` | 列表字段与安装语义（201 空体）一致 | 已有契约对齐与测试覆盖 |
| SN-07 | 只读观测（集群/控制面） | `GET /api/cluster/status`、`GET /api/cluster/control/leases/{key}`；WebUI `/cluster` 面板 | 只读、默认安全；字段与文档一致 | ✅ 2026-11-10 本地实测：`GET /api/cluster/status` 返回有效 JSON（cluster/control_plane/config/consensus 等字段）；WebUI/SDK 契约与测试已覆盖 |
| SN-08 | WebUI 契约路径 | HTMX 表单提交、SOP/DAG/技能页面 | 与 bridge 契约一致、无 415/404 误用 | ✅ WebUI 测试 26 passed |
| SN-09 | SDK 最小调用 | TS/Py/Go 对接本地 bridge（health/sops/execute_sop 等） | 方法签名与返回符合契约 | ✅ 单元测试通过；建议补最小可运行示例（S3） |
| SN-10 | 认知追踪/地图（如启用） | `/api/cognitive/trace`、`/api/cognitive/map/*` 等 | 端点可达、返回符合 OpenAPI | 按实际启用状态验收 |

## 5. 已验证证据（2026-11-10）

- `cargo test -p sisyphus-core --lib`：116 passed
- `cargo test -p sisyphus-webui`：26 passed
- `cargo check -p sisyphus-cli`：通过；`sisyphus --help` 可用
- CLI 本地工具：`read/write/glob/grep/exec` 实测通过（SN-01）
- Bridge 本地实测（正确顶层配置）：`GET /health` → `{"status":"SERVING","version":"0.4.0"}`；`GET /api/sops` → `["verify_sop"]`；`GET /api/cluster/status` 返回有效只读快照（SN-02/SN-03/SN-07）
- SDK：Go `go test ./...`、Python 38 passed、TS 34 passed（建议二批次）
- 门禁：`cargo check-all`、`cargo fmt --all --check`、`make audit`、相关 clippy 通过（以批次记录为准）

## 6. 已知限制与环境说明

- **Bridge lib 测试（本环境）**：`cargo test -p sisyphus-bridge --lib --no-default-features` 中 `sandboxd_client` 3 项因 `Permission denied`（spawn sandboxd）失败，属环境限制；不代表 REST/单机主流程回归。
- **端口冲突**：HTTP/gRPC 默认端口可能被占用；需通过顶层 `http_address`/`bridge_address` 或环境变量调整。
- **LLM/外部依赖**：真实 goal 执行、部分 agent/MCP/联网能力依赖凭据与外部服务；单机“功能存在”与“端到端效果”需分层验收。
- **分布式控制面**：生产准入与真实网络证据仍为外部阻塞；单机验收不包含该部分。

## 7. 后续（S3/S4）

- S3：补充 SDK 最小可运行示例、工具/MCP 分层说明、错误态一致性改进。
- S4：输出单机功能矩阵验收表并与分布式后置项分离归档。


### 1.4 CLI 诊断与快速上手（新增）

```bash
# 本地 + bridge 诊断（可选 HTTP 检查）
sisyphus doctor --http http://127.0.0.1:8080

# 快速上手指引
sisyphus quickstart

# 端到端演示（需 bridge HTTP+gRPC 可达）
BASE=http://127.0.0.1:8080 BRIDGE_GRPC=http://127.0.0.1:50051 SOP_ID=verify_sop   ./scripts/single-node-demo-flow.sh

# 保存并执行场景/模板 SOP
BASE=http://127.0.0.1:8080 BRIDGE_GRPC=http://127.0.0.1:50051   ./scripts/single-node-apply-sop-template.sh docs/examples/sop-templates/scenario-code-review-gate.json --run
```

**场景手册**：`docs/runbooks/single-node-dev-scenarios.md`（质量门、诊断修复验证、端到端执行路径与观测/回归）。
