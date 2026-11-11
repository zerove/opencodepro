# 单机自动化开发能力验收手册

> 目标：将“高性能、高质量自动化开发工具”在单机场景下的关键能力转化为**可复现步骤、可记录证据、可分层判定**的验收流程。  
> 边界：引擎/产品化主线优先；`cluster.control_plane.enabled=false`，P2-D/ADR-019 生产准入为外部后置。

## 1. 验收分层与判定

| 层级 | 含义 | 典型依赖 | 判定 |
|---|---|---|---|
| L0 契约/静态 | 脚本、模板、接口与文档一致性 | 本地工具链 | 必须通过（CI/本地门禁） |
| L1 单机运行时 | bridge/CLI/HTTP 基础链路可用 | 本地端口、配置 | 有 bridge 环境时必须通过 |
| L2 确定性执行 | SOP/DAG/模板/工具的可复现执行与观测 | 执行器、无外部 LLM 亦可部分覆盖 | 目标场景应通过或给出可解释降级 |
| L3 目标驱动自动化 | `execute/goal`、LLM 审计/规划、复杂修复闭环 | LLM/凭据/MCP/工具权限 | 环境具备时验证；缺失则记录为**环境阻塞**并保留可解释失败证据 |
| L4 性能/稳定性 | 启动、延迟、吞吐、长任务、回归 | 稳定负载与采样口径 | 按基线口径给出趋势，不伪造结论 |

**总体结论格式**：`PASS` / `PASS_WITH_LIMITATIONS` / `FAIL` / `BLOCKED`（逐项标注）。

## 2. 前置条件与环境记录

执行前记录（写入证据）：

- 时间、提交：`git rev-parse HEAD`、分支/工作区状态
- 工具链：`rustc -V`、`cargo -V`（注意 `RUSTUP_TOOLCHAIN` 覆盖）
- 关键二进制：`./target/debug|release/sisyphus`、`./target/debug|release/sisyphus-bridge`
- 配置摘要：`bridge_address`/`http_address`（顶层）、`cluster.control_plane.enabled`、LLM/MCP/embedding 相关开关（脱敏）
- 端口与工作区：HTTP/gRPC 端口、`X-Sisyphus-Workspace-Id`

## 3. 证据模板

为每次验收创建目录（建议）：`docs/benchmarks/acceptance/<YYYY-MM-DD>-<tag>/`

```bash
./scripts/single-node-acceptance-evidence.sh <tag>
# 例：./scripts/single-node-acceptance-evidence.sh nightly
```

```text
meta.md          # 环境、提交、配置摘要、执行人/自动化
l0-static.log    # fmt/check/audit/模板校验/smoke --quick
l1-runtime.log   # doctor/readiness/health/sops/cluster/status
l2-sop.log       # sop list/show/run、模板保存与执行、demo-flow
l3-goal.log      # execute/goal 请求与响应（含 hint/error）、trace（如可用）
l4-perf.md       # 采样命令、N、min/avg/max、波动说明
summary.md       # 分项判定与差距/阻塞
```

> 脚本生成的模板含可选命令提示：L3 可用 `TEST_GOAL=1 ./scripts/single-node-demo-flow.sh` 或 `curl /api/execute/goal` 记录 `error/hint`；L4 可用 `./scripts/single-node-bench-sample.sh {cli|bridge} --json` 归档结构化样本。

最小 `summary.md` 字段：`commit`, `date`, `scope`, `L0..L4`, `blocking_issues`, `next_actions`.

## 4. L0 契约与静态门禁

```bash
# 快速回归（推荐）
./scripts/single-node-contract-smoke.sh --quick
# 或
make smoke-single-node-quick

# SOP 模板资产
./scripts/validate-sop-templates.sh
# 或
make validate-sop-templates

# 全量门禁（按环境耗时较高）
make audit
env -u RUSTUP_TOOLCHAIN cargo fmt --all --check
env -u RUSTUP_TOOLCHAIN cargo check-all
```

**通过标准**：脚本/门禁退出码 0；模板结构校验全部通过；审计无新增违规。

## 5. L1 单机运行时与可观测

启动 bridge（示例，使用正确顶层字段）：

```bash
SISYPHUS_CONFIG=<config.toml> SISYPHUS_LLM_EMBEDDING_ALLOW_DUMMY=true \
  ./target/debug/sisyphus-bridge --bin sisyphus-bridge
```

检查：

```bash
./scripts/single-node-readiness-check.sh
# 或
sisyphus doctor --bridge http://127.0.0.1:50051 --http http://127.0.0.1:8080

curl -sS http://127.0.0.1:8080/health
curl -sS -H 'X-Sisyphus-Workspace-Id: default' http://127.0.0.1:8080/api/sops
curl -sS http://127.0.0.1:8080/api/cluster/status
```

**通过标准**：健康可达；`/api/sops` 返回数组；`/api/cluster/status` 仅只读观测且符合默认关闭策略；doctor/readiness 无致命失败。

## 6. L2 确定性执行与场景闭环

### 6.1 发现与执行

```bash
sisyphus --bridge http://127.0.0.1:50051 sop list
sisyphus --bridge http://127.0.0.1:50051 sop show <sop-id>
sisyphus --bridge http://127.0.0.1:50051 sop run verify_sop
```

### 6.2 模板保存与执行

```bash
BASE=http://127.0.0.1:8080 BRIDGE_GRPC=http://127.0.0.1:50051 WS=default \
  ./scripts/single-node-apply-sop-template.sh docs/examples/sop-templates/verify-sequence.json --run

# 场景模板示例（按需）
./scripts/single-node-apply-sop-template.sh docs/examples/sop-templates/scenario-code-review-gate.json --run
```

### 6.3 端到端演示

```bash
BASE=http://127.0.0.1:8080 BRIDGE_GRPC=http://127.0.0.1:50051 SOP_ID=verify_sop \
  ./scripts/single-node-demo-flow.sh
```

**通过标准**：SOP 可发现/查看/执行并返回真实执行结果（`Test` 节点结构化输出含 `status/success/exit_code/message`）；认知报告可含偏差/未收敛记录但不应出现“全轮无真实结果”误判；模板保存(201)与执行链路可复现。

**已知关注点**：认知反思偏差与收敛表现受环境/阈值/LLM 影响，需记录而非忽略。

## 7. L3 目标驱动自动化（LLM 依赖）

### 7.1 环境准备清单（执行前逐项确认）

| 项 | 检查方式 | 期望 |
|---|---|---|
| LLM 端点 | `SISYPHUS_LLM_ENDPOINT` 或 `[llm].endpoint` | 可从本机访问 |
| API Key | `SISYPHUS_LLM_API_KEY` 或 `[llm].api_key`（支持 `env:VAR`） | 非空且有效 |
| 模型 | `SISYPHUS_LLM_MODEL` 或 `[llm].model` | 与端点匹配 |
| 连通性 | `sisyphus provider test <name>` | 返回模型响应 |
| 嵌入 | `SISYPHUS_LLM_EMBEDDING_PROVIDER` / `_ALLOW_DUMMY` | 明确是否使用 dummy |
| 超时/预算 | `SISYPHUS_LLM_REQUEST_TIMEOUT_SECS`、`budget_mode` | 与任务规模匹配 |
| 工具/MCP | `sisyphus tool list`、`sisyphus mcp --help` | 任务所需工具可用 |
| 凭据脱敏 | 证据仅记录 provider/model/endpoint 主机名 | 不落盘 key |

```bash
# 连通性探测（先于任何 L3 执行）
sisyphus provider list
sisyphus provider test <provider-name>
```

```bash
curl -sS -X POST http://127.0.0.1:8080/api/execute/goal \
  -H 'Content-Type: application/json' \
  -H 'X-Sisyphus-Workspace-Id: default' \
  -d '{"goal":"示例目标","sop_id":"verify_sop","project_id":"default"}'
```

> 可选：`TEST_GOAL=1 GOAL_TEXT="..." ./scripts/single-node-demo-flow.sh` 可在演示流程中探测并展示 `error/hint` 响应（不替代成功闭环证据）。

**通过标准（环境具备时）**：返回成功结果并可关联 trace/事件（按配置）；失败时返回可解释 `error` 与可选 `hint`，能区分依赖缺失、StrategicPivot/无真实结果、参数或契约问题。

**阻塞记录（环境缺失时）**：明确缺失的 LLM/凭据/MCP/工具；保留请求与响应证据；以 `BLOCKED` 标注 L3，不替代 L2 结论。

## 8. L4 性能与稳定性采样

```bash
# CLI（固定 N）
N=10 CLI_BIN=./target/debug/sisyphus ./scripts/single-node-bench-sample.sh cli

# Bridge（固定端口/配置，建议稳定环境连续采样）
N=10 BRIDGE_BIN=./target/debug/sisyphus-bridge ./scripts/single-node-bench-sample.sh bridge

# 可选：机器可读输出，便于归档/对比
./scripts/single-node-bench-sample.sh cli --json
./scripts/single-node-bench-sample.sh bridge --json
```

**通过标准**：记录 `n/min/p50/p95/avg/max`、环境与二进制类型（debug/release）、冷热条件；不将中断/分批样本当作连续回归结论；对显著波动给出定位项而非掩盖。`--json` 输出含 `timestamp/build`，可直接归档用于回归对比。

## 9. 错误与恢复验收（横切）

| 场景 | 期望 |
|---|---|
| bridge 不可达 | CLI/脚本给出可操作提示（地址、doctor/readiness） |
| SOP/参数/工作区错误 | 明确 4xx/错误信息与修正方向 |
| `execute/goal` / composite 执行失败 | 返回可解释 `error`；命中时附带可选 `hint`（HTTP），SDK/CLI/TUI 应展示或暴露 hint 与恢复建议 |
| `sop run`（gRPC/CLI）失败 | CLI/TUI 输出错误并按模式给出 StrategicPivot/LLM 相关可操作提示 |
| `execute/goal` LLM 依赖缺失 | 可解释错误（含 `hint` 时优先展示），建议改用 `sop run` 或补齐配置 |
| StrategicPivot/无真实结果 | 指引检查显式成功输出、阈值/反思与执行链路 |
| MCP/技能空态或失败 | 可发现性与下一步操作提示清晰 |

## 10. 关联资产

- 运行与验证：`single-node-operation-and-verification.md`
- 契约守护：`single-node-contract-guard.md`
- SOP 端到端：`single-node-sop-e2e-guide.md`
- 场景与恢复：`single-node-dev-scenarios.md`
- 典型流程：`single-node-typical-workflow.md`
- 性能基线：`docs/plans/single-node-performance-baseline.md`、`docs/benchmarks/`
- 路线与差距：`docs/plans/p1-p2-roadmap.md`、对标分析文档

## 11. 验收输出

- `summary.md`：分层判定、关键证据路径、阻塞项与风险
- 若 `FAIL`：最小复现、影响范围、建议修复与回归项
- 若 `BLOCKED`：外部依赖清单与可替代的降级验证结果（L0/L1/L2）
