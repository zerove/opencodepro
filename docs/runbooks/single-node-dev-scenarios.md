# 单机自动化开发场景手册（D-A）

> 目标：以 Sisyphus 当前实现（SOP/DAG + 认知循环 + bridge/CLI/WebUI/SDK）落地可复现的自动化开发场景，兼顾**高质量（可审计、可回归）**与**可感知性能**。  
> 原则：单机优先；`cluster.control_plane.enabled=false`；ADR-020（控制面 REST 写入=否）不变；依赖缺失时返回可解释错误，不伪造证据。

## 0. 前置与分层

| 层级 | 能力 | 前置条件 |
|---|---|---|
| L0 本地 CLI | `read/write/glob/grep/exec`、本地会话、`doctor/quickstart`、本地技能查看 | 无需 bridge |
| L1 Bridge 接入 | `/health`、SOP/技能/会话 REST、`sop list/show/run`（gRPC） | bridge HTTP+gRPC 可达、正确顶层配置 |
| L2 目标驱动执行 | `POST /api/execute/goal`（及 SDK `executeSop`） | LLM/执行器/工具链配置可用；无配置时应返回可解释错误 |
| L3 扩展能力 | MCP/外部工具、沙箱等 | 对应服务、鉴权、权限、网络 |

**诊断入口**：
```bash
sisyphus doctor --http http://127.0.0.1:8080
sisyphus quickstart
BASE=http://127.0.0.1:8080 ./scripts/single-node-readiness-check.sh
```

## 1. 场景 A：质量门/代码评审流水线（顺序或并行）

**目标**：将检查、审查与汇总编排为可审计 DAG，产出可观测结果。

**推荐模板**：
- 顺序：`docs/examples/sop-templates/quality-gate-sequence.json`
- 并行审查汇聚：`docs/examples/sop-templates/scenario-code-review-gate.json`、`parallel-triple-review.json`

**流程**：
1. 发现/保存模板：`POST /api/sops`（`X-Sisyphus-Workspace-Id`）或复用内置 SOP。
2. 查看详情：`sisyphus sop show <id> --http <http_base>` / `GET /api/sops/{id}`。
3. 执行：`sisyphus --bridge <grpc_addr> sop run <id>`（直接 SOP 路径）。
4. 观测：执行输出与 `CognitionReport`；按需查询 `/api/execute/goal/{id}/status|trace|stream`（目标驱动路径）与 `/api/cluster/status`（只读）。

**验收**：节点步骤与边关系可追踪；失败节点/命令原因可定位；替换占位命令为项目真实门禁后可复用。

## 2. 场景 B：问题诊断 → 修复 → 验证

**目标**：以可重复流程约束诊断与验证，避免静默失败。

**推荐模板**：`docs/examples/sop-templates/scenario-diagnose-fix-verify.json`（将占位命令替换为复现/测试/检查命令）。

**流程**：
1. 复现与收集证据（日志、错误、最小输入）。
2. 定位根因并实施最小修复。
3. 通过 `sop run` 或项目命令执行验证；记录输出与残余风险。
4. 若走 `execute/goal`：确认 LLM/工具配置可用，否则按可解释错误补齐前置条件。

**验收**：每步有明确输入/输出；修复前后对比可复现；回归纳入 `make smoke-single-node-quick` 或场景脚本。

## 3. 场景 C：代码理解与任务执行（端到端）

**目标**：从发现到执行到观测形成闭环，支持集成与自动化。

**入口**：
```bash
# 一键演示（需 bridge HTTP+gRPC）
BASE=http://127.0.0.1:8080 BRIDGE_GRPC=http://127.0.0.1:50051 SOP_ID=verify_sop \
  ./scripts/single-node-demo-flow.sh
```

**执行路径选择**：
- `sisyphus --bridge <grpc> sop run <sop-id>`：直接 SOP 执行，适合已定义 DAG/模板与确定性步骤。
- `POST /api/execute/goal` / SDK `executeSop`：目标驱动，可能触发 LLM 审计与推理，需相应配置与凭据。

**验收**：就绪、发现、执行、观测四步可脚本化复现；路径差异与依赖在输出/文档中明确。

## 4. 场景 D：集成与只读观测自动化

**目标**：外部系统/CI 以最小成本接入健康、发现与状态观测。

**入口**：
- `scripts/single-node-readiness-check.sh`（`BASE/WS/TIMEOUT`）
- `docs/examples/single-node-integration-samples.md`
- OpenAPI：`GET /openapi.json`

**验收**：只读检查可用于门禁前置；失败按连通性/契约/依赖分类处理。

## 5. 失败处理与恢复

| 现象 | 归类 | 处理建议 |
|---|---|---|
| bridge 连接失败 / 超时 | 连通性 | `sisyphus doctor --http ...`；确认 `bridge_address`/`http_address`（顶层）、端口与进程 |
| 404/400（SOP/工作区/契约） | 契约/参数 | 核对 SOP ID、`X-Sisyphus-Workspace-Id`、请求体与 `docs/runbooks/single-node-contract-guard.md` |
| `execute/goal` 报模型/LLM 不可用 | 依赖缺失 | 改用 `sop run` 验证 DAG 执行；或补齐 LLM 配置后重试目标驱动路径 |
| `AllIterationsPivoted` / StrategicPivot（无真实执行结果） | 认知/执行判定 | 确认节点命令可成功退出且 `Test` 输出含显式成功状态；先用 `sop run` + 确定性模板定位；再检查 LLM/反思与阈值配置 |
| MCP/工具调用失败 | 依赖/鉴权/参数 | `sisyphus mcp info -- <connect...>` 先发现；检查凭据、网络与参数 JSON |
| 权限/沙箱限制 | 环境 | 按 runbook 分层降级；在允许环境执行对应步骤 |

## 6. 质量与性能回归

```bash
# 快速回归（推荐日常）
./scripts/single-node-contract-smoke.sh --quick
# 可选：附带运行中 bridge 的就绪冒烟
SMOKE_BRIDGE_HTTP=http://127.0.0.1:8080 ./scripts/single-node-contract-smoke.sh --quick

# 全量回归（发布前/阶段验收）
make smoke-single-node

# 运行时采样（固定口径多次）
N=10 ./scripts/single-node-bench-sample.sh cli
```

## 7. 关联资产

- 模板：`docs/examples/sop-templates/`
- 运行/验收：`docs/runbooks/single-node-operation-and-verification.md`
- 契约守护：`docs/runbooks/single-node-contract-guard.md`
- 典型流：`docs/runbooks/single-node-typical-workflow.md`
- SOP 端到端：`docs/runbooks/single-node-sop-e2e-guide.md`
- 性能基线：`docs/plans/single-node-performance-baseline.md`、`docs/benchmarks/`
