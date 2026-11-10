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

- `docs/plans/p1-p2-roadmap.md`（P2-C）
- `docs/runbooks/single-node-contract-guard.md`
- `book/src/rest-api.md`、`book/src/sdk-reference.md`
