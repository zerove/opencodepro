# 单机集成与扩展样例（P2-C）

> 目标：展示以 Sisyphus 为平台底座的集成方式（REST/SDK/观测），突出统一接入与可编排优势。

## 1. REST 集成（最小）

```bash
BASE=http://127.0.0.1:8080

# 健康与发现
curl -s "$BASE/health"
curl -s "$BASE/api/sops"

# 只读观测
curl -s "$BASE/api/cluster/status"

# 执行（需配置）
# curl -s -X POST "$BASE/api/execute/goal" -H 'Content-Type: application/json' \
#   -d '{"sop_id":"<id>","goal":"..."}'
```

适用：CI/脚本/外部系统对接；配合 OpenAPI (`/openapi.json`) 生成客户端。

## 2. SDK 集成

- 见 `single-node-sdk-examples.md`（TS/Python/Go 最小调用）。
- 建议封装：统一错误处理（连通性/契约/业务错误分层）、重试策略（对幂等只读请求）、超时与可观测日志。

## 3. 观测与自动化联动

- **只读轮询**：`/health`、`/api/cluster/status`、执行状态/轨迹端点用于外部看板或自动化判断。
- **SSE/流式**：执行进度与认知地图流（按端点支持）用于实时 UI；注意 `sop_id`/`sse_sop_id` 订阅规则。
- **契约守护**：集成侧应跟随 `docs/runbooks/single-node-contract-guard.md` 与版本变更记录。

## 4. 扩展方向（与引擎主线一致）

- **SOP 模板化**：将常见任务封装为 SOP/DAG，通过 `execute/goal` 与观测端点形成可复用工作流。
- **工具/MCP 扩展**：按 `single-node-tools-mcp-guide.md` 接入与发现工具，保持依赖分层与安全边界。
- **多端接入**：WebUI/CLI/SDK 共用 bridge 契约，避免分叉实现。

## 5. 关联

- `docs/runbooks/single-node-dev-scenarios.md`（自动化开发场景手册）
- `docs/plans/p1-p2-roadmap.md`（P2-C）
- `docs/runbooks/single-node-contract-guard.md`
- `book/src/rest-api.md`、`book/src/sdk-reference.md`

## 6. 轮询与状态检查脚本骨架（示例）

```bash
#!/usr/bin/env bash
set -euo pipefail
BASE="${BASE:-http://127.0.0.1:8080}"

# 健康门禁
curl -fsS "$BASE/health" >/dev/null

# 只读观测快照
curl -fsS "$BASE/api/cluster/status" | jq '.enabled, .control_plane.configured' 2>/dev/null || \
  curl -fsS "$BASE/api/cluster/status"

# SOP 发现
curl -fsS "$BASE/api/sops"
```

> 生产/自动化集成应增加超时、重试与错误分类（连通性/契约/业务），并遵循契约守护清单。

## 7. 可运行脚本样例

- `scripts/single-node-readiness-check.sh`：健康门禁 + SOP 发现 + 集群只读状态检查（`BASE`/`WS`/`TIMEOUT` 可配置）。
- `scripts/single-node-demo-flow.sh`：端到端演示流程（就绪检查 → SOP 发现/详情 → CLI `sop run` 执行 → 观测入口）。
- `scripts/single-node-apply-sop-template.sh`：保存 SOP 模板并可选执行（`<template.json> [--run]`），适合场景化模板快速验证。
- `scripts/single-node-contract-smoke.sh`：P1-F 契约冒烟（`--quick/--full`，可选 SDK/bridge 就绪检查）。
- `scripts/single-node-bench-sample.sh`：CLI/bridge 运行时多次采样统计（P2-B）。
- `docs/examples/sop-templates/`：SOP/DAG 模板资产（顺序、并行审查、场景化模板）。

```bash
# 就绪检查
BASE=http://127.0.0.1:8080 ./scripts/single-node-readiness-check.sh

# 端到端演示
BASE=http://127.0.0.1:8080 BRIDGE_GRPC=http://127.0.0.1:50051 SOP_ID=verify_sop   ./scripts/single-node-demo-flow.sh

# 保存并执行场景模板
BASE=http://127.0.0.1:8080 BRIDGE_GRPC=http://127.0.0.1:50051   ./scripts/single-node-apply-sop-template.sh docs/examples/sop-templates/scenario-code-review-gate.json --run
```
