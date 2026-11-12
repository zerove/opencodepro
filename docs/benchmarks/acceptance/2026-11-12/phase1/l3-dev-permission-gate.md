# L3「dev 权限闸」验收 — 证据

## 1. 结论

CLI 的 `sisyphus dev` 此前**完全绕过权限体系**：计划里的 `RunCommand`（任意
shell）直接执行，既不匹配规则、也不写审计、更不会询问。TUI 的 dev 路径一直有
审批闸（`DevAskApprover`），两条路径行为不一致 —— 而 CLI 恰恰是脚本 / CI /
agent 调用的入口。

本次补上统一权限闸并做确定性验收：**8/8 PASS**。

## 2. 缺陷

`Developer` 侧的审批闸本身是好的（T2.1 就有 `DevApprover` 与工具侧单测），
缺的是**CLI 侧的接线**：

```rust
// crates/sisyphus-tools-agent/src/developer.rs
async fn approve_step(&self, step: &DevStep) -> Result<bool, String> {
    let Some(approver) = self.approver.as_ref() else {
        return Ok(true);          // ← CLI 没注入 approver → 全部放行
    };
    ...
}
```

`sisyphus dev`（CLI）构造 `Developer` 时没有 `with_approval_handler(...)`，
于是 `approve_step` 直接返回 `true`：`RunCommand` → `builtin:bash_exec`
（在 `DANGEROUS_TOOLS` 里，权限判定本应 `ask`）被静默执行。

## 3. 修复

新增 `crates/sisyphus-cli/src/dev_approval.rs`（`CliDevApprover`），与 TUI
**同一套判定口径**：

| 判定 | 行为 | 审计 |
|---|---|---|
| `Allow` | 放行 | `allowed` |
| `Deny` | 拒绝（`--yes` 也不放行） | `denied` |
| `Ask` + 交互式终端 | 问 `[Y/n]` | `asked-approved` / `asked-denied` |
| `Ask` + 非交互 | **拒绝**（fail-safe） | `asked-denied` |
| `Ask` + `--yes` | 放行（显式授权） | `asked-approved` |

要点：

- **非交互默认拒绝**：自动化入口不能静默执行模型提出的任意命令；需要时显式
  `--yes`（或写权限规则 allow）。
- **`--yes` 不能绕过显式 deny 规则**。
- 审计写进 `<data-dir>/audit/permissions.jsonl`，与其它 CLI 入口同一文件、同一
  `resolved` 取值口径（`allowed` / `denied` / `asked-approved` / `asked-denied`）。
- 提示与审计都带命令文本（截断 300 字符），人能看懂拒绝了什么。

CLI 侧新增 `--yes`（别名 `--auto-approve`）。
- `delegate inline` / `delegate run`：**每条**命令执行前过闸（deny → 一条都不执行）。
- `require_tool` 与 `CliDevApprover`：stdin EOF（读到 0 字节）判为
  `asked-unanswered` → **拒绝**；真正回车（读到换行）仍是默认同意，显式
  `echo y | sisyphus exec ...` 继续可用（向后兼容）。
  解析逻辑抽成可测的 `parse_approval(read_result, buf)`。

## 4. 验收方法

`scripts/l3-dev-permission-check.sh`（**确定性**：用桩 LLM
`scripts/l3-mock-llm.py` 返回固定计划，计划里含一条 `RunCommand` 步骤，
不依赖真实模型的输出随机性）。

| # | 场景 | 期望 | 结果 |
|---|---|---|---|
| 1 | `dev` 非交互、无 `--yes` | 命令**未执行** + 输出说明 + 审计 `asked-denied` | PASS |
| 2 | `dev` 非交互、`--yes` | 命令**已执行** + 审计 `asked-approved` | PASS |
| 3 | `dev` 权限规则 `deny` + `--yes` | 命令**未执行** + 审计 `denied` | PASS |
| 4 | `exec` stdin EOF | 命令**未执行** + 明确错误（`stdin reached EOF`） | PASS |
| 5 | `exec` 显式 `echo y` | 命令**已执行**（向后兼容） | PASS |
| 6 | `delegate inline` 未获授权 | 命令**未执行** + 「权限策略拒绝」 | PASS |
| 7 | `delegate run`（任务文件）未获授权 | 命令**未执行** | PASS |

原始输出：`runs/dev-permission-check.out`（`PASS=14 FAIL=0`；每项都断言
「命令是否真的执行」与审计记录）。

复现：

```bash
bash scripts/l3-dev-permission-check.sh
```

## 5. 影响面与后续

- **TUI 的 dev 路径不受影响**（它本来就有 `DevAskApprover`）。
- **L3 场景脚本已显式加 `--yes`**（`scripts/l3-run-scenarios.sh`）：验收脚本是
  自动化场景，需要模型能执行 `cargo check` 之类的命令。改完后 s1–s3 重跑
  **3/3 PASS**。
- **`dev --verify-cmd` 的客观闸门与权限闸是正交的两件事**：闸门决定「目标是否
  达成」，权限闸决定「哪些命令允许执行」。被拒绝的步骤会在报告里显示
  `denied by user: builtin:bash_exec`，不影响闸门判定。
- 遗留：`ComplexChange` 动作不走审批（`dev_tool_id` 返回 `None`），它做的是
  文件编辑（`builtin:file_write`/`file_edit` 在权限体系里本就不是危险工具），
  但如果将来让 `ComplexChange` 能执行命令，必须一并接进审批闸。

## 6. 诚实性说明

- 本场景用桩 LLM 驱动 `dev`（固定计划），验证的是**权限闸接线与判定**，
  不是模型的规划能力。
- 只覆盖了 `RunCommand` 一类危险动作；`CreateFile` / `EditFile` / `ReadFile`
  在权限体系里默认 `allow`（非危险工具），`ComplexChange` 目前不经过审批。
- 交互式 `[Y/n]` 分支未做自动化断言（需要伪终端），逻辑与 `require_tool`
  一致；非交互分支、显式 deny、`--yes`、stdin EOF、显式 `y` 五条路径均有断言。
