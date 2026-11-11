# 阶段 4：质量指标与回归（T4.1–T4.5） — 验收证据

> 采集日期：2026-11-12（UTC）　对应计划：`docs/plans/automation-development-plan-2026-11.md`
> 本目录汇总该阶段各项的**实际执行证据**与复现命令；未执行项标注 BLOCKED 并写明缺失依赖。

| 任务 | 状态 | 证据 | 复现命令 |
|---|---|---|---|
| T4.1 质量指标定义与采集 | DONE | 指标模型与聚合链路（`sisyphus-metrics::quality` 6 单测、`verify --record`、`metrics quality`、采集脚本接入真实聚合）；**首轮真实数据**：3 个受控 fixture 任务→ 成功率 0.667、修复率 0.5、回归率 0.0、平均 640ms（`docs/benchmarks/quality-metrics/2026-11-11-6f1bcbd-fixture-samples.json`）；同时修正两处真实缺陷：`cargo fix --clippy` 非法参数（lint 修复必然失败）、采集端点/指标名与真实契约不符。**剩余**：仓库真实任务样本需 L3 环境（T1.4） | `cargo test -p sisyphus-metrics --lib quality` |
| T4.2 性能回归门禁 | DONE | `scripts/perf-regression-check.sh` + `make perf-regression`；基线 `docs/benchmarks/perf-regression-baseline-2026-11-10.json`；实跑 rc=0 | ``./scripts/perf-regression-check.sh cli`` |
| T4.3 evals 子集入 CI | DONE | eval-runner `--cases` 过滤 + `scripts/evals-ci-run.sh` + `make eval-ci`；ci.yml `evals-ci` job；实跑 2/2 PASS | ``./scripts/evals-ci-run.sh "metis/metis-001,momus/momus-plan-002"`` |
| T4.4 统一 trace 关联 | DONE | trace id 工具 / HTTP 中间件 / gRPC 拦截器 / SSE 负载 / CLI 注入 / span 字段 / 契约文档；10 个 trace 单测；实跑透传·生成·非法替换三态 | ``cargo test -p sisyphus-bridge --lib trace_id`` |
| T4.4b gRPC trace 端到端 | DONE | CLI → bridge → 认知循环：拦截器生成的 trace id 被 `grpc.execute_sop` 及其**全部嵌套 span**（run_cognitive_loop / cognitive_iteration / execute_phase / reflect）继承，实现「一次请求可查到全部子步骤」（`trace-grpc-e2e.txt`） | `sisyphus --bridge <addr> remote exec <sop>` |
| T4.4c HTTP→SSE 载体 | DONE | 广播处理器改为从请求头取 trace id 注入事件 payload（与执行事件同构）；端到端：订阅 `/api/events` + `POST /api/events/broadcast`（带 `x-trace-id: trace-sse-e2e`）→ 事件 payload 含 `"trace_id":"trace-sse-e2e"`（`trace-sse-e2e.txt`）；单测 2 个 | `cargo test -p sisyphus-bridge --lib trace_id` |
| T4.5 长窗口稳定性 | DONE | **4 小时窗口**（2881 采样，14398s）：探活与 /api/metrics 成功率 100%；RSS 55404→55996 KB（斜率 -0.83 KB/min，decreasing）、FD 20→11、线程 18→14 不增反降 → 无泄漏迹象（`docs/benchmarks/long-run/2026-11-11-31d14c3/`） | `./scripts/long-run-stability.sh 240` |

## 复现方式

```bash
# 门禁
env -u RUSTUP_TOOLCHAIN cargo fmt --all --check
env -u RUSTUP_TOOLCHAIN cargo check-all
env -u RUSTUP_TOOLCHAIN cargo clippy -p <crate> --all-targets
```

## 诚实性说明

- 本目录只收录**实际执行过**的命令与观察值；未执行的一律标注为 BLOCKED 并写明缺失依赖。
- 指标类数值在样本缺失时记 `null`，不填 0 或估算值。
