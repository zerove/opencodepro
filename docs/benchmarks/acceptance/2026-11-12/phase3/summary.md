# 阶段 3：生态与性能（T3.1–T3.4） — 验收证据

> 采集日期：2026-11-12（UTC）　对应计划：`docs/plans/automation-development-plan-2026-11.md`
> 本目录汇总该阶段各项的**实际执行证据**与复现命令；未执行项标注 BLOCKED 并写明缺失依赖。

| 任务 | 状态 | 证据 | 复现命令 |
|---|---|---|---|
| T3.1 自定义开发命令 | DONE | `custom_command` 模块 + `sisyphus command list/show/run` + TUI `/commands`；11 loader + 4 输出 + 2 审批单测 | ``cargo test -p sisyphus-tools-session --lib custom_command`` |
| T3.2 Provider 交互式引导 | BLOCKED | 缺可用 LLM 凭据，无法验证「5 分钟内完成首次真实调用」 | `—` |
| T3.3 启动分段归因与优化 | DONE | bridge `startup_phase_*_ms`（end_to_end_ready_ms≈1530ms）+ TUI 首帧 PTY 测量（p50≈14.3ms）；`scripts/measure-bridge-startup.sh`、`scripts/measure-tui-first-frame.sh` | ``./scripts/measure-tui-first-frame.sh ./target/debug/sisyphus 3`` |
| T3.4 并行冲突感知 | DONE | `team/conflict.rs`：`detect_conflicts` + `partition_by_write_conflicts`；冲突告警 + 波次串行化；6 个新单测 | ``cargo test -p sisyphus-tools-agent --lib conflict`` |

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
