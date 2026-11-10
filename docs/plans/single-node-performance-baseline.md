# 单机性能与资源基线计划（P2-B）

> 目标：建立单机关键路径的**可复现测量口径**与回归记录，支撑体验优化与工程质量（不直接与 JCode/OpenCode 公开数值对标）。

## 1. 测量范围（建议）

| 路径 | 指标示例 | 口径说明 |
|---|---|---|
| CLI 启动/帮助 | `time sisyphus --help`（冷/热） | 固定机器、工具链与构建配置；记录环境 |
| Bridge 启动就绪 | 进程启动 → `/health` 可达耗时 | 本地配置、端口、依赖就绪状态需一致 |
| WebUI 启动就绪 | 进程启动 → 首页/健康代理可达 | 同上 |
| 典型只读请求 | `/health`、`/api/sops`、`/api/cluster/status` p50/p95 | 本地回环、固定负载与采样次数 |
| 典型执行（可选） | `execute/goal` 端到端耗时/资源 | **强依赖 LLM/模型**；需单独标注环境与不可比性 |

## 2. 环境记录模板

- 日期/提交：`git rev-parse HEAD`
- 机器：CPU/内存/OS/内核
- 工具链：`rustc -Vv`、`cargo -V`
- 构建：`--release` 或 dev；相关 feature
- 配置：关键 env/config（脱敏）

## 3. 执行与记录

- 每次采样记录原始数据与统计摘要（均值/中位数/p95/样本数）。
- 结果存入 `docs/benchmarks/`（按日期/主题命名），并在本计划中索引。
- 回归判定：以同口径基线对比，显著退化需定位原因或记录权衡。

## 4. 当前状态

- 已有部分 benchmark/基线文档（见 `docs/benchmarks/`）；单机产品路径（CLI/bridge/WebUI 启动与只读接口）的统一口径待按本计划补齐。
- 首次采样建议作为 P2-B 初始基线，在具备稳定环境后执行并落盘。

## 5. 关联

- `docs/plans/p1-p2-roadmap.md`（P2-B）
- `docs/benchmarks/`（历史与后续基线）

## 6. 初始采样记录

- `docs/benchmarks/single-node-runtime-baseline-2026-11-10.md`：dev 构建下 bridge 就绪与只读端点延迟基线；debug CLI 命令耗时补充（单次参考）。
- `docs/benchmarks/single-node-runtime-baseline-2026-11-10.md`（补充）：release 构建（现有 `target/release` 产物）CLI/bridge 就绪与只读端点延迟采样；CLI `--help` 单次耗时偏高已标注待复测。

> 后续：在固定环境下多次复测 release CLI 启动（冷热）、WebUI 就绪与更多只读路径分布，按同口径落盘。
