# 阶段 4：质量指标与回归（T4.1–T4.5） — 验收证据

> 采集日期：2026-11-12（UTC）　对应计划：`docs/plans/automation-development-plan-2026-11.md`
> 本目录汇总该阶段各项的**实际执行证据**与复现命令；未执行项标注 BLOCKED 并写明缺失依赖。

| 任务 | 状态 | 证据 | 复现命令 |
|---|---|---|---|
| T4.1 质量指标定义与采集 | IN_PROGRESS | 指标模型与聚合链路落地（`sisyphus-metrics::quality` 6 单测、`verify --record`、`metrics quality`）；实跑 2 条记录 → success_rate=0.5。**缺仓库真实任务样本**（需 L3 环境） | ``cargo test -p sisyphus-metrics --lib quality`` |
| T4.2 性能回归门禁 | DONE | `scripts/perf-regression-check.sh` + `make perf-regression`；基线 `docs/benchmarks/perf-regression-baseline-2026-11-10.json`；实跑 rc=0 | ``./scripts/perf-regression-check.sh cli`` |
| T4.3 evals 子集入 CI | DONE | eval-runner `--cases` 过滤 + `scripts/evals-ci-run.sh` + `make eval-ci`；ci.yml `evals-ci` job；实跑 2/2 PASS | ``./scripts/evals-ci-run.sh "metis/metis-001,momus/momus-plan-002"`` |
| T4.4 统一 trace 关联 | DONE | trace id 工具 / HTTP 中间件 / gRPC 拦截器 / SSE 负载 / CLI 注入 / span 字段 / 契约文档；10 个 trace 单测；实跑透传·生成·非法替换三态 | ``cargo test -p sisyphus-bridge --lib trace_id`` |
| T4.4b gRPC trace 端到端 | DONE | CLI → bridge → 认知循环：拦截器生成的 trace id 被 `grpc.execute_sop` 及其**全部嵌套 span**（run_cognitive_loop / cognitive_iteration / execute_phase / reflect）继承，实现「一次请求可查到全部子步骤」（`trace-grpc-e2e.txt`） | `sisyphus --bridge <addr> remote exec <sop>` |
| T4.5 长窗口稳定性 | IN_PROGRESS | 60 分钟窗口 715 采样：探活 100%，RSS +5.8 KB/min（stable）、FD 20→11、线程 18→14。**「数小时」窗口仍需重采** | ``./scripts/long-run-stability.sh 60`` |

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
