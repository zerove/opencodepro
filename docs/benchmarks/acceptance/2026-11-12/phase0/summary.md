# 阶段 0：基线与验收自动化（T0.1–T0.4） — 验收证据

> 采集日期：2026-11-12（UTC）　对应计划：`docs/plans/automation-development-plan-2026-11.md`
> 本目录汇总该阶段各项的**实际执行证据**与复现命令；未执行项标注 BLOCKED 并写明缺失依赖。

| 任务 | 状态 | 证据 | 复现命令 |
|---|---|---|---|
| T0.1 提交与仓库同步 | DONE | `git rev-parse HEAD origin/master` 两者一致；工作区干净 | ``git rev-parse HEAD origin/master`` |
| T0.2 性能采样口径升级 | DONE | `scripts/single-node-bench-sample.sh` 输出 n/min/p50/p95/avg/max + timestamp/build | ``N=3 ./scripts/single-node-bench-sample.sh cli --json`` |
| T0.3 L3 环境准备清单 | DONE | `docs/runbooks/single-node-automation-acceptance.md` §7.1 清单、§8 分位数口径 | `阅读文档` |
| T0.4 验收证据自动化 | DONE | `scripts/single-node-acceptance-evidence.sh --run-l0` 生成 l0-static.log | ``./scripts/single-node-acceptance-evidence.sh --run-l0`` |

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
