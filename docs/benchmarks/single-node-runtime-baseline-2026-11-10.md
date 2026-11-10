# 单机运行时基线（2026-11-10）

> 口径：本地开发环境单次采样（dev 构建、`./target/debug/sisyphus-bridge`），用于回归参考；不与 OpenCode/JCode 公开基准直接对标。

## 环境

- 日期：2026-11-10
- 提交：`a3014aa5c7460ae6ee72998c94d9158589e912e0`（采样时）
- OS：Linux
- Bridge：`SISYPHUS_LLM_EMBEDDING_ALLOW_DUMMY=true`，`cluster.enabled=false`
- 配置：`bridge_address=http://127.0.0.1:15061`，`http_address=http://127.0.0.1:15090`

## 结果

| 指标 | 结果 | 备注 |
|---|---:|---|
| Bridge 启动 → `/health` 就绪 | ~1085 ms | 轮询间隔 200ms，首次成功响应时间 |
| `GET /health` 延迟（5 次） | 0.004459–0.006749 s | 本地回环 |
| `GET /api/sops` 延迟（5 次） | 0.006291–0.007877 s | 本地回环 |
| `GET /api/sops` 响应 | `["verify_sop"]` | 单机默认/现有 SOP |
| `GET /api/cluster/status` | 200, 有效 JSON | 只读快照 |

## 复现步骤（示例）

1. 准备顶层配置（`bridge_address`/`http_address`，`[cluster].enabled=false`）。
2. 启动：`SISYPHUS_CONFIG=... SISYPHUS_LLM_EMBEDDING_ALLOW_DUMMY=true ./target/debug/sisyphus-bridge`
3. 轮询 `GET /health` 记录就绪时间；用 `curl -w '%{time_total}'` 采样只读端点延迟。
4. 结束后清理进程。

## 说明

- dev 构建与 release 构建性能差异显著；回归对比需固定构建类型与环境。
- `execute/goal` 端到端耗时强依赖 LLM/工具配置，本次未纳入。
