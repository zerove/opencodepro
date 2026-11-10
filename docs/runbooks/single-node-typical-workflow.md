# 单机典型任务流（P1-C / P1-E）

> 目标：在单机环境下，按可复现步骤完成“本地工具 → 会话 → 服务接入 → SOP/观测”的典型工作流。  
> 前置：见 `single-node-operation-and-verification.md`；按需准备 LLM 配置（执行类能力）。

## 场景 A：纯本地（无 bridge）

适用：文件操作、快速命令、本地会话管理。

```bash
# 1) 本地文件工具
sisyphus read ./README.md
sisyphus write /tmp/demo.txt "hello"
sisyphus glob "*.md" .
sisyphus grep "Sisyphus" .
sisyphus exec "echo ok"

# 2) 本地会话（--data-dir 指定存储目录）
sisyphus --data-dir ./.sisyphus-data session create "demo"
sisyphus --data-dir ./.sisyphus-data session list
# 继续：add-message / get / export 等（见 sisyphus session --help）
```

**验收要点**：命令返回预期结果与退出码；会话可创建与列出（已在 S2 基线验证）。

## 场景 B：单机服务接入（bridge + 只读/基础能力）

适用：健康检查、SOP/技能/会话查询、集群只读观测。

```bash
# 1) 启动 bridge（示例：自定义端口，注意 bridge_address/http_address 为顶层字段）
# SISYPHUS_CONFIG=./config.toml SISYPHUS_LLM_EMBEDDING_ALLOW_DUMMY=true \
#   cargo run -p sisyphus-bridge --bin sisyphus-bridge

# 2) 健康与发现
curl -s http://127.0.0.1:8080/health
curl -s http://127.0.0.1:8080/openapi.json | head
curl -s http://127.0.0.1:8080/api/sops
curl -s http://127.0.0.1:8080/api/skills

# 3) 只读观测（单机默认 cluster.control_plane.enabled=false）
curl -s http://127.0.0.1:8080/api/cluster/status
curl -s http://127.0.0.1:8080/api/cluster/control/leases/lease:demo

# 4) WebUI（可选）
# WEBUI_BRIDGE_URL=http://127.0.0.1:8080 cargo run -p sisyphus-webui --bin sisyphus-webui
# 浏览器访问 http://127.0.0.1:3000 与 /cluster
```

**验收要点**：端点可达、返回与契约一致；WebUI 面板在 bridge 不可达时降级显示默认快照（只读）。

## 场景 C：SOP 目标执行（需配置）

适用：通过 bridge 执行目标（SOP/DAG + 认知循环）。

```bash
# REST：POST /api/execute/goal
curl -s -X POST http://127.0.0.1:8080/api/execute/goal \
  -H 'Content-Type: application/json' \
  -d '{"sop_id":"<sop-id>","goal":"完成示例目标"}'
# 预期：{"output":..., "report":...} 或可解释错误（依赖/配置缺失时不应静默成功）
```

SDK 调用方式见 `docs/examples/single-node-sdk-examples.md`（`executeSop/execute_sop/ExecuteSOP`）。

**验收要点**：在具备 LLM/引擎配置的环境下端到端成功；无配置时返回明确错误。

## 场景 D：会话与执行联动（经 bridge）

适用：服务端会话管理与消息流（按 bridge 契约）。

- `GET /api/sessions` / `POST /api/sessions` / `GET /api/sessions/{id}` / `POST /api/sessions/{id}/messages`
- WebUI `/sessions` 与 HTMX 端点提供可视化入口

**验收要点**：契约与 SDK/WebUI 一致；状态可查询与持久化（依运行时配置）。

## 常见问题（单机）

- **端口冲突**：调整顶层 `bridge_address` / `http_address`（非 `[bridge]` 子表）。
- **bridge 不可达**：先确认进程与端口，再检查 WebUI/SDK 的 `bridgeUrl`/`WEBUI_BRIDGE_URL`。
- **执行失败**：区分“契约/连通性问题”与“LLM/工具依赖缺失”；后者需按分层说明补齐前置条件。
- **控制面**：单机默认不启用生产控制面；只读观测可用，不提供写入路径。

## 关联文档

- `single-node-operation-and-verification.md`（S2）
- `single-node-tools-and-dependencies.md`（S3）
- `../examples/single-node-sdk-examples.md`
- `../plans/p1-p2-roadmap.md`（P1-C/P1-E 持续推进）
