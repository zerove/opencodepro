# 阶段 2：交互与治理（T2.1–T2.4） — 验收证据

> 采集日期：2026-11-12（UTC）　对应计划：`docs/plans/automation-development-plan-2026-11.md`
> 本目录汇总该阶段各项的**实际执行证据**与复现命令；未执行项标注 BLOCKED 并写明缺失依赖。

| 任务 | 状态 | 证据 | 复现命令 |
|---|---|---|---|
| T2.1 TUI 对话式执行 MVP | DONE（脚本化验收） | 四切片：S1 interrupt+Cancelled、S2 DevApprover 审批闸、S3 TUI 接缝（BgOp::DevAsk/DevRunHandle/审批卡片/Esc→Resume）、S4 验收测试；证据 `crates/sisyphus-tools-agent/tests/t2_1_mvp_acceptance.rs`（2 工具调用 / 2 审批 / 1 中断恢复） | ``cargo test -p sisyphus-tools-agent --test t2_1_mvp_acceptance`` |
| T2.1b TUI 真实交互冒烟 | DONE | `scripts/tui-dev-smoke.sh`（伪终端驱动）：`/dev` 对话框 → 目标输入 → Run → 运行中视图 → Esc 中断 → 恢复入口，3 项断言全过（`tui-dev-smoke.txt`）；录屏不可复跑，故以脚本+断言作等效证据 | `./scripts/tui-dev-smoke.sh ./target/debug/sisyphus` |
| T2.2b dev 工具闸接入统一权限体系 | DONE | `DevAskApprover` 先按 `CliPermission` 判定（glob 规则 / 危险默认 ask）：allow 直接放行、deny 直接拒绝并写审计、ask 才弹窗；3 个新单测（deny 不弹窗 / ask 弹窗并记 `asked-approved` / 非危险直接放行） | `cargo test -p sisyphus-cli --lib dev_approver` |
| T2.2 权限规则与审批审计 | DONE | glob 规则 + 危险默认 ask + JSONL 审计 + `permission check/audit`；TUI 弹窗审批（`crates/sisyphus-cli/src/approval.rs` + `Dialog::ToolApproval`） | ``cargo test -p sisyphus-cli --lib permission`` |
| T2.3 会话回放页 | DONE | `render_session_html` + 时间轴/工具折叠/diff 着色/trace 徽标；13 个渲染单测 | ``cargo test -p sisyphus-tools-session --lib session`` |
| T2.4 错误提示有效性抽样 | DONE | `sisyphus-cli::hints`：`HINTS` + `suggest_hint` + 抽样判定闭环；7 个单测 | ``cargo test -p sisyphus-cli --lib hints`` |

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
