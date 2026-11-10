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

## 补充：Debug CLI 本地命令耗时（2026-11-10，同环境参考）

| 命令 | real (s) | user (s) | sys (s) | 备注 |
|---|---:|---:|---:|---|
| `./target/debug/sisyphus --help` | 0.05 | 0.02 | 0.02 | 单次，`/usr/bin/time -p` |
| `./target/debug/sisyphus exec 'echo ok'` | 0.08 | 0.01 | 0.04 | 本地工具执行路径 |
| `./target/debug/sisyphus --data-dir /tmp/opencode/smoke-data session list` | 0.03 | 0.01 | 0.01 | 本地会话存储读取 |

> 单次采样仅作量级参考；回归对比需多次采样并固定构建/数据状态。

## 补充：Release 构建运行时采样（2026-11-10，同环境参考）

> 二进制：`target/release/sisyphus`、`target/release/sisyphus-bridge`（本地现有 release 产物，单次采样）。

| 指标 | 结果 | 备注 |
|---|---:|---|
| `./target/release/sisyphus --help` | real 2.34s / user 0.00s / sys 0.03s | 单次；可能受环境/首次加载影响，需多次复测确认 |
| `./target/release/sisyphus exec 'echo ok'` | real 0.01s / user 0.00s / sys 0.01s | 单次 |
| `./target/release/sisyphus --data-dir ... session list` | real 0.00s / user 0.00s / sys 0.00s | 单次（空/小数据集） |
| Bridge 启动 → `/health` 就绪 | ~3287 ms | 轮询间隔 200ms，`cluster.enabled=false` |
| `GET /health` 延迟（5 次） | 0.002275–0.002751 s | 本地回环 |
| `GET /api/sops` 延迟（5 次） | 0.002968–0.003407 s | 本地回环 |

**说明**：release 与 debug、不同机器/负载/配置下数值不可直接比较；本节用于建立同环境参考点。CLI `--help` 单次耗时偏高需后续复测排查（冷启动、动态链接、终端/环境因素等）。
