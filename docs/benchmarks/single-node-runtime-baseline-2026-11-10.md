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

### 单次采样（`/usr/bin/time -p`）

| 命令 | real (s) | user (s) | sys (s) | 备注 |
|---|---:|---:|---:|---|
| `./target/debug/sisyphus --help` | 0.05 | 0.02 | 0.02 | 单次 |
| `./target/debug/sisyphus exec 'echo ok'` | 0.08 | 0.01 | 0.04 | 本地工具执行路径 |
| `./target/debug/sisyphus --data-dir /tmp/opencode/smoke-data session list` | 0.03 | 0.01 | 0.01 | 本地会话存储读取 |

### 多次采样（`scripts/single-node-bench-sample.sh cli`，N=5，debug）

| 命令 | n | min (s) | avg (s) | max (s) |
|---|---:|---:|---:|---:|
| `sisyphus --help` | 5 | 0.010000 | 0.020000 | 0.030000 |
| `sisyphus exec 'echo ok'` | 5 | 0.010000 | 0.012000 | 0.020000 |
| `sisyphus --data-dir /tmp/opencode/bench-sn session list` | 5 | 0.010000 | 0.012000 | 0.020000 |

> 采样结果受机器状态/缓存/构建类型影响；回归对比需固定口径多次复测。

## 补充：Release 构建运行时采样（2026-11-10，同环境参考）

> 二进制：`target/release/sisyphus`、`target/release/sisyphus-bridge`（本地现有 release 产物）。

| 指标 | 结果 | 备注 |
|---|---:|---|
| `./target/release/sisyphus --help` | 首次单次 2.34s；复测 ~0.01s（多次） | 存在环境/冷启动波动，需固定条件多次复测 |
| `./target/release/sisyphus exec 'echo ok'` | ~0.01s | 单次参考 |
| `./target/release/sisyphus --data-dir ... session list` | ~0.00s | 单次参考（空/小数据集） |
| Bridge 启动 → `/health` 就绪 | ~3287 ms | 轮询间隔 200ms，`cluster.enabled=false` |
| `GET /health` 延迟（5 次） | 0.002275–0.002751 s | 本地回环 |
| `GET /api/sops` 延迟（5 次） | 0.002968–0.003407 s | 本地回环 |

**说明**：release 与 debug、不同机器/负载/配置下数值不可直接比较；CLI 启动耗时以多次复测与同口径对比为准。

## 补充：Bridge 多次采样（debug，2026-11-10，同环境参考）

> 工具：`scripts/single-node-bench-sample.sh bridge`

| 指标 | n | min | avg | max | 备注 |
|---|---:|---:|---:|---:|---|
| Bridge 启动 → `/health` 就绪 (ms) | 2 | 123 | 2981.5 | 5840 | N=2，早期采样；存在高波动 |
| `GET /health` 延迟 (s) | 2 | 0.001794 | 0.0018295 | 0.001865 | N=2 |
| `GET /api/sops` 延迟 (s) | 2 | 0.002557 | 0.0027105 | 0.002864 | N=2 |
| Bridge 启动 → `/health` 就绪 (ms) | 3 | 1123 | 1505.333333 | 2269 | N=3（采样中断前有效样本，`HTTP=http://127.0.0.1:15331`） |
| `GET /health` 延迟 (s) | 3 | 0.001847 | 0.002176667 | 0.002407 | N=3 |
| `GET /api/sops` 延迟 (s) | 3 | 0.002570 | 0.002844667 | 0.003162 | N=3 |
| Bridge 启动 → `/health` 就绪 (ms) | 5 | 123 | 791.6 | 2462 | N=5（debug，`HTTP=http://127.0.0.1:15351`；首轮偏慢后续稳定） |
| `GET /health` 延迟 (s) | 5 | 0.001727 | 0.0019382 | 0.002144 | N=5 |
| `GET /api/sops` 延迟 (s) | 5 | 0.002401 | 0.002523 | 0.002635 | N=5 |
| Bridge 启动 → `/health` 就绪 (ms) | 4 | 123 | 610.75 | 1144 | N=4（N=10 采样中断前有效样本，debug，`HTTP=http://127.0.0.1:15491`） |
| `GET /health` 延迟 (s) | 4 | 0.001849 | 0.00218425 | 0.002766 | N=4 |
| `GET /api/sops` 延迟 (s) | 4 | 0.002787 | 0.0031475 | 0.003394 | N=4 |
| Bridge 启动 → `/health` 就绪 (ms) | 10 | 124 | 883.9 | 1144 | N=10（分两批采集：4+6，debug，dummy embedding；环境对长命令有中断） |
| `GET /health` 延迟 (s) | 10 | 0.001729 | 0.0020239 | 0.002441 | N=10（同上批次聚合） |
| `GET /api/sops` 延迟 (s) | 10 | 0.002408 | 0.0026694 | 0.002902 | N=10（同上批次聚合） |
| Bridge 启动 → `/health` 就绪 (ms) | 5 | 123 | 709.6 | 1130 | N=5（debug，手工循环采集，`HTTP=http://127.0.0.1:15551`） |
| `GET /health` 延迟 (s) | 5 | 0.001844 | 0.0019886 | 0.002396 | N=5 |
| `GET /api/sops` 延迟 (s) | 5 | 0.002586 | 0.0028364 | 0.003429 | N=5 |

> 已获得 N=10 聚合样本（非单次连续命令，受环境中断影响分批采集）；仍建议在稳定专用环境按固定冷热条件做连续 `N>=10` 复测并区分 debug/release 后用于回归判定。

## 补充：Release 多次采样（2026-11-10，同环境参考）

> 二进制：`target/release/sisyphus`、`target/release/sisyphus-bridge`（本地现有产物）；采样过程受环境中断影响时采用分步/手工循环采集。

### Release CLI（`scripts/single-node-bench-sample.sh cli`，N=5）

| 命令 | n | min (s) | avg (s) | max (s) | 备注 |
|---|---:|---:|---:|---:|---|
| `sisyphus --help` | 5 | 0.000000 | 0.174000 | 0.870000 | `/usr/bin/time -p` 粒度较粗，存在单次高值 |
| `sisyphus exec 'echo ok'` | 5 | 0.000000 | 0.000000 | 0.000000 | 低于计时粒度 |
| `sisyphus --data-dir /tmp/opencode/bench-sn session list` | 5 | 0.000000 | 0.000000 | 0.000000 | 低于计时粒度 |

### Release Bridge（手工循环采样，N=5，dummy embedding，`cluster.enabled=false`）

| 指标 | n | min | avg | max | 备注 |
|---|---:|---:|---:|---:|---|
| Bridge 启动 → `/health` 就绪 (ms) | 4* | 124 | 827.25 | 1126 | 首轮计时样本异常（-1885ms）已剔除；HTTP `127.0.0.1:15531` |
| `GET /health` 延迟 (s) | 5 | 0.001137 | 0.0012678 | 0.001455 | 本地回环 |
| `GET /api/sops` 延迟 (s) | 5 | 0.001535 | 0.0017432 | 0.002069 | 本地回环 |

> Release 与 debug 不可直接横比；CLI 需更高精度计时与冷热分离复测，Bridge 建议在稳定环境连续采样并记录构建/配置/负载条件。
