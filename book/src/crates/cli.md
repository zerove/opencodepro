# sisyphus-cli — 命令行界面

CLI 二进制提供了从终端直接访问 Sisyphus 工具与代理的能力，支持一次性命令与交互式 TUI 模式。

## 安装

```bash
cargo build --release -p sisyphus-cli
./target/release/sisyphus --help
```

> 注意：crate 名是 `sisyphus-cli`，产物二进制名是 `sisyphus`。

## 命令结构

```
sisyphus [OPTIONS] <COMMAND>
```

### 全局选项

| 标志 | 环境变量 | 说明 |
|------|-----|-------------|
| `-i`, `--interactive` | | 启动交互式 TUI 模式 |
| `-b`, `--bridge` | `SISYPHUS_BRIDGE_ADDRESS` | Bridge gRPC 地址（默认：`http://[::1]:50051`） |
| `--data-dir` | `SISYPHUS_DATA_DIR` | 待办/会话存储的数据目录 |
| `--config` | `SISYPHUS_CONFIG` | TOML 配置文件路径 |

## 文件工具

```bash
# 读取文件
sisyphus read path/to/file.txt

# 写入内容到文件
sisyphus write path/to/output.txt "file content"

# 执行 shell 命令（危险命令默认需人工确认；非交互环境用 --yes 显式放行）
sisyphus exec "cargo test" --dir /path/to/project
sisyphus exec "cargo test" --yes        # 脚本 / CI 里显式放行

# 网络搜索
sisyphus search "Rust async/await" -n 10
```

### 编辑后自动验证（post-edit verify）

`write`、`edit`、`apply_patch` 落盘后可按配置执行一次短验证，避免“文件写坏了但命令仍报成功”。
该能力通过环境变量开启，不改变 `Config` schema：

| 变量 | 说明 |
|------|------|
| `SISYPHUS_POST_EDIT_VERIFY` | 验证命令模板。含 `{files}` 时替换为改动文件列表（shell 安全引用）；`auto` 表示按类型推断：`.rs` → 在最近的 Cargo 包根目录执行 `cargo check --quiet`；未设置或空串表示关闭 |
| `SISYPHUS_POST_EDIT_VERIFY_TIMEOUT_SECS` | 超时秒数，默认 `120`；非法值或 `0` 回落默认 |
| `SISYPHUS_POST_EDIT_VERIFY_STRICT` | `1`/`true`/`yes`/`on` 时验证失败返回非零退出（阻断“成功”）；默认仅告警 |

```bash
# 对改动的 Rust 文件执行 cargo check（按文件定位包根目录）
SISYPHUS_POST_EDIT_VERIFY=auto sisyphus edit src/lib.rs "old" "new"

# 自定义模板 + 严格模式：验证失败即命令失败
SISYPHUS_POST_EDIT_VERIFY='rustfmt --check {files}' \
SISYPHUS_POST_EDIT_VERIFY_STRICT=1 \
  sisyphus write src/main.rs "$(cat src/main.rs)"

# 限制超时
SISYPHUS_POST_EDIT_VERIFY='cargo clippy --quiet' \
SISYPHUS_POST_EDIT_VERIFY_TIMEOUT_SECS=60 \
  sisyphus apply-patch src/lib.rs -i fix.patch
```

输出为单行摘要（`✓/✗/-` + 耗时 + 命令）；失败时附带输出尾部 20 行。

## 验证与修复

### verify — 有界验证-修复循环

`diagnose` 只报告、`fix` 只修一轮；`verify` 把两者串成有界循环：**验证 → 未通过则修复 → 再验证**，
直到达到门禁或用尽轮数，并逐轮输出错误/警告/耗时与修复结果。

```bash
# 只验证（等价于一次 check + lint）
sisyphus verify

# 零警告门禁 + 最多 3 轮自动修复
sisyphus verify --max-warnings 0 --fix --attempts 3

# 修复时同时应用 lint 修复；输出机器可读 JSON
sisyphus verify --fix --lint-fixes --json

# 指定后端/包/工作目录
sisyphus verify --lang node -p web -C ./frontend
```

| 选项 | 说明 |
|---|---|
| `--lang` | 语言后端（rust/node/python/go/java/gradle/fe）；省略时按目录自动检测 |
| `-p`, `--package` | 目标包 |
| `--all` | 全部工作区成员 |
| `--no-lint` | 跳过 lint，仅 check |
| `--fix` | 未通过时执行一次修复并重试；未指定时只跑一轮验证 |
| `--lint-fixes` | 修复阶段同时应用 lint 修复（Rust 下为 `cargo fix --clippy`） |
| `--attempts N` | 最大验证轮数（默认 3，仅 `--fix` 时生效；上限保证终止） |
| `--max-warnings N` | 允许的最大警告数，`0` 表示零警告门禁；省略表示不限制 |
| `--json` | 输出结构化结果（轮次、耗时、修复记录、最终计数） |
| `--record <path>` | 把本次执行落成一条任务级质量记录（JSON），供 `sisyphus metrics quality` 聚合 |
| `-C` | 工作目录 |

门禁判定：`errors == 0` 且 `warnings <= max_warnings`（未设置时不限制）。未通过时命令以非零退出，
可直接用于 CI 或编排节点的判定条件。

### metrics — 任务级质量指标聚合

从任务记录目录汇总成功率 / 修复率 / 回归率 / 平均单任务耗时 / 平均单任务 token。记录由
`sisyphus verify --record <path>` 或 `sisyphus dev --record <path>` 产生；
无记录时各比率为 `null`（不伪造 0 值）。

```bash
# 聚合查看（人类可读）
sisyphus metrics quality --records docs/benchmarks/quality-metrics/records

# 机器可读
sisyphus metrics quality --records docs/benchmarks/quality-metrics/records --json
```

| 选项 | 说明 |
|---|---|
| `--records <dir>` | 任务记录目录（每个 `.json` 一条记录）；默认 `docs/benchmarks/quality-metrics/records` |
| `--json` | 输出结构化指标 |

token 维度只在 provider 上报了 usage 的记录上统计（`dev` 链路自动写入
`prompt_tokens` / `completion_tokens`）；没有 token 样本时该指标为 `null`。

聚合还会输出样本涉及的**引擎版本**（记录里的 `engine_commit` / `engine_dirty`）：
多于一组说明这批样本混用了不同引擎版本（脏工作区产出的样本单列为 `sha+dirty`），
此时成功率等比率**不可直接横向比较** —— 变化可能来自样本构成而非引擎本身。

指标口径与记录格式见 `docs/plans/single-node-quality-metrics.md`。

## 权限与审计

### 规则文件

默认读取工作目录下的 `./.sisyphus-permissions.json`：

```json
[
  { "pattern": "exec", "permission": "ask" },
  { "pattern": "builtin:*", "permission": "allow" },
  { "pattern": "jcode/*", "permission": "deny" }
]
```

- `pattern` 支持 glob（`*`、`?`、`[...]`），按声明顺序匹配，**首条命中生效**；语法非法的 pattern 退化为精确比较。
- 未命中任何规则时：危险工具（`exec`/`bash`/`bash_exec`/`builtin:bash_exec`/`shell`）→ `ask`，其余 → `allow`。

### 会执行命令的入口

以下入口在执行前都会走同一套权限判定与审计：

| 入口 | 危险动作 | 非交互环境的默认行为 |
|---|---|---|
| `sisyphus exec` | shell 命令 | 拒绝（`--yes` 显式放行） |
| `sisyphus command run --exec` | Shell 命令模板 | 拒绝（`--yes` 显式放行） |
| `sisyphus delegate inline/run` | 任务里的每条命令 | 拒绝（`--yes` 显式放行） |
| `sisyphus dev` | 计划里的 `RunCommand` | 拒绝（`--yes` 显式放行） |

stdin 已 EOF（`< /dev/null`、CI 里没有 tty）时视为「无法确认」→ 拒绝；显式
`echo y | sisyphus exec ...` 仍可用。显式 `deny` 规则**永远优先**，`--yes` 不会绕过它。

### 检查与审计

```bash
# 查看某个工具此刻的判定与来源
sisyphus permission check exec
# exec: ask (source: dangerous-default)

sisyphus permission check builtin:file_read --json

# 查看最近的审批/判定记录（JSONL 追加写入 <data-dir>/audit/permissions.jsonl）
sisyphus permission audit -n 20
sisyphus permission audit --json
```

审计覆盖 `allowed` / `denied` / `asked-approved` / `asked-denied` / `asked-proceeded-tui`
（TUI 内执行不弹窗，按既有约定记录为 `asked-proceeded-tui`）。
审计路径优先级：`SISYPHUS_PERMISSION_AUDIT` > `<data-dir>/audit/permissions.jsonl`。
写入失败只告警，不阻断主命令。

## 自定义开发命令

项目里放一个 markdown 就能得到一条命令，不需要改 Rust 代码。加载目录：

- 用户级：`~/.config/sisyphus/commands/*.md`
- 项目级：`<project>/.sisyphus/commands/*.md`（同名命令覆盖用户级）

文件由 YAML frontmatter + 正文组成，正文即模板，支持 `{{arg}}` 占位符：

```markdown
---
name: review
description: 审查指定文件
usage: review <path> [focus]
args:
  - name: path
    description: 目标文件
    required: true
  - name: focus
    description: 关注点
    default: correctness
---
请审查 {{path}}，重点关注 {{focus}}。
```

带 `run` 的是 Shell 命令（渲染后可执行），没有 `run` 的是提示词命令：

```markdown
---
name: count
description: 统计文件行数
args:
  - name: file
    required: true
run: "wc -l {{file}}"
---
```

```bash
# 发现（CLI 表格 / JSON）
sisyphus command list
sisyphus command list --json

# 查看参数与模板
sisyphus command show review

# 渲染提示词（不执行）
sisyphus command run review --arg path=src/lib.rs
# 请审查 src/lib.rs，重点关注 correctness。

# Shell 命令：默认只打印渲染结果，加 --exec 才执行
sisyphus command run count --arg file=src/lib.rs
sisyphus command run count --arg file=src/lib.rs --exec
```

规则：

- `required` 缺省时，带 `default` 的参数视为可选，否则必填；
- 必填参数缺失会拒绝渲染并打印用法（不会用空串蒙混）；
- 未声明的 `{{...}}` 原样保留，模板里可安全写 `${{ github.ref }}`；
- `--exec` 仅对 Shell 命令有效，并走与 `sisyphus exec` **相同**的权限规则、
  交互审批与 JSONL 审计；
- 非法 YAML 或空模板文件会被跳过，不影响其他命令加载。

TUI 内输入 `/commands` 可列出当前目录可发现的命令（补全列表也包含该入口）。

## 代理子命令

### librarian — 外部调研代理

搜索网络、获取文档、查找代码示例。

```bash
# 调研主题（web、docs、code 或全部）
sisyphus librarian research "Rust async" --scope all -n 5

# 从 URL 获取并展示文档
sisyphus librarian fetch-docs https://doc.rust-lang.org/std/fmt/

# 查找库或 API 的代码示例
sisyphus librarian examples "serde_json::from_str" -l rust

# 从 JSON 调研报告综合发现
sisyphus librarian synthesise findings.json
```

`librarian research` 通过 `--scope` 支持以下范围：
- `web` — 搜索 web 获取一般信息
- `docs` — 获取文档页面
- `code` — 查找代码示例
- `all` — 以上全部（默认）

### metis — 预规划分析代理

在实现前分析请求中的歧义与隐含需求。

```bash
# 分析内联请求文本
sisyphus metis analyse "Add user authentication with OAuth2 support"

# 从文件分析请求
sisyphus metis analyse --file request.txt
```

分析输出包括：
- **摘要**（Summary）— 请求的高层评估
- **歧义**（Ambiguities）— 需要澄清的方面，含严重级别与建议解决方案
- **隐含需求**（Implicit Requirements）— 可推断但未明确陈述的需求

### momus — 计划/代码/架构审查代理

审查计划、源代码与架构的完整性与一致性。

```bash
# 审查计划文件
sisyphus momus review-plan plan.md

# 审查源代码文件
sisyphus momus review-code src/main.rs

# 内联审查架构文档
sisyphus momus review-arch "The system uses microservices with message queues"

# 审查目录下所有 Rust 文件
sisyphus momus review-dir src/
```

## 任务与会话管理

```bash
# 添加待办项
sisyphus todo add "Fix login bug" -p high

# 列出待办事项
sisyphus todo list --status pending

# 创建新会话
sisyphus session create "Refactoring session"

# 列出可用会话
sisyphus session list
```

### 会话回放页（HTML 导出）

```bash
sisyphus --data-dir ./.sisyphus-data session export <id> replay.html --format html
```

导出的页面是**离线单文件**（不引用外部 CSS/JS/字体），可直接发给他人或用浏览器打开回看：

- **时间轴**：每条消息带 `#序号/总数` 与时间戳，按角色着色（user/assistant/system/other）；
- **工具调用折叠**：正文里的 `<tool_call>…</tool_call>` 与 ```` ```tool ```` 代码块折叠为 `🔧 tool call` 详情块；
- **diff 着色**：```` ```diff ```` 代码块按 `+`/`-`/`@@` 分别着色，便于审阅改动；
- **长消息折叠**：超过 1500 字符的消息折叠为“展开全文”，避免淹没时间轴；
- **trace 关联**：会话 `metadata` 中键名含 `trace` 或 `sop` 的条目渲染为徽标（按 key 排序，输出稳定）。

所有正文与元数据都会做 HTML 转义（`& < > "`），因此消息内容里的标签不会被执行。

## Bridge 命令

```bash
# 通过 bridge gRPC 执行标准操作程序
sisyphus sop run goal:deploy_to_staging

# 列出可用 SOP
sisyphus sop list

# 检查 bridge 健康状态
sisyphus health
```

## 工作区管理

工作区提供隔离的项目环境，带有独立状态（SOP、会话、待办）。

```bash
# 创建新工作区
sisyphus workspace create "My Project" --project-id proj_alpha --root-dir /path/to/project

# 列出所有工作区
sisyphus workspace list

# 切换到工作区
sisyphus workspace switch <workspace-id>

# 显示当前工作区
sisyphus workspace current

# 删除工作区（带确认提示）
sisyphus workspace delete <workspace-id>

# 删除工作区（跳过确认）
sisyphus workspace delete <workspace-id> -y
```

工作区内持久化的所有数据（SOPs、会话、待办、经验）通过 `project_id` 隔离。切换工作区会改变活动的项目上下文，使后续命令在该项目的存储命名空间内操作。

## 对话式开发（`dev`）

`dev` 让 LLM 规划并执行一个开发目标（建文件 / 编辑 / 跑命令），再按步骤验证。

```bash
sisyphus dev "<目标>" \
  --context "<当前状态/约束>" \
  [--parallel] [--resume] \
  [--timeout-secs 300] \
  [--verify-cmd "cargo clippy -- -D warnings"] \
  [--verify-fix-attempts 2] \
  [--record <path>] [--scenario <label>] \
  [--yes]
```

| 参数 | 说明 |
|------|------|
| `--context` | 项目现状、约束、相关代码片段；弱模型对上下文质量非常敏感 |
| `--parallel` | 并行执行步骤（最多 4 并发，写冲突由编排层串行化） |
| `--resume` | 从 checkpoint 恢复，跳过已完成步骤 |
| `--timeout-secs` | 整体超时（默认 300s）；本地/慢速模型需调大 |
| `--verify-cmd` | **客观验收命令**。通过才判定目标达成；未通过则自动修复，仍失败则整体判失败 |
| `--verify-fix-attempts` | 闸门失败后的自动修复次数（默认 2，`0` 表示只判定不修复） |
| `--record` | 把本次闭环写成一条任务级质量记录（JSON）；**必须同时指定 `--verify-cmd`**，否则直接拒绝 |
| `--scenario` | 记录里的场景标签（默认 `dev-closed-loop`），如 `diagnose-fix` / `quality-gate` / `feature` |
| `--yes` | 自动放行「需要人工确认」的命令。默认：交互式终端问 `[Y/n]`，**非交互环境直接拒绝**（fail-safe） |

> **TUI `/dev` 最小出口**：TUI 交互路径不提供 `--verify-cmd` 参数时，可通过环境变量注入客观闸门（未设置则保持模型自评）：
> - `SISYPHUS_DEV_VERIFY_CMD`：客观验收命令（非空生效，语义同 `--verify-cmd`）
> - `SISYPHUS_DEV_VERIFY_FIX_ATTEMPTS`：闸门失败后的自动修复次数（正整数生效，语义同 `--verify-fix-attempts`）

### 为什么需要 `--verify-cmd`

每个步骤自带 `verification`，由**模型在计划里自选**，可能与用户目标不一致。实测：目标是
`cargo clippy -- -D warnings` 通过，模型却声明 `CargoCheck`（警告不是错误 → 通过），
于是「全部步骤成功」而目标实际未达成。

`--verify-cmd` 是用户给定的**唯一客观判据**：

- 未通过 → 整体状态 `FAILED`，CLI 退出码非零；
- 通过但仍有步骤失败 → `PARTIAL FAILURE`（目标达成与过程失败分别记录）；
- 通过且步骤全成功 → `SUCCESS`。

闸门修复接受两种 LLM 输出形态：整文件重写 `{"files":[{path,content}]}`（优先）与
精确编辑 `[{path,old_string,new_string}]`；落盘前会剥离文件内容外的 markdown 围栏。
修复 prompt 附带错误输出点名的文件内容与 `git diff`（若工作区是 git 仓库）。

```bash
# 质量门示例：clippy 告警必须以错误级别清零
sisyphus dev "消除 src/lib.rs 的 clippy 告警" \
  --context "$(cat src/lib.rs)" \
  --verify-cmd "cargo clippy -- -D warnings"

# 脚本里据此判断成败（闸门失败 → 非零退出）
if ! sisyphus dev "实现 tests/feature.rs 引用的 API" --verify-cmd "cargo test"; then
  echo "目标未达成" >&2
fi
```

### 权限闸：模型提出的命令要先过权限体系

`dev` 的计划里可能有 `RunCommand`（任意 shell）。执行前会走**统一权限体系**
（与 TUI 的 dev 路径同一口径，规则文件 `.sisyphus-permissions.json`）：

| 判定 | 行为 |
|---|---|
| `allow` 规则命中 | 直接放行 |
| `deny` 规则命中 | 拒绝（`--yes` 也不放行） |
| 危险命令默认 `ask` | 交互式终端问 `[Y/n]`；**非交互环境直接拒绝** |

每次判定都写审计 `<data-dir>/audit/permissions.jsonl`
（`allowed` / `denied` / `asked-approved` / `asked-denied` / `asked-unanswered`）。

stdin **已 EOF**（`< /dev/null`、CI 里没有 tty）时判为「无法确认」并**拒绝** ——
不会把「没读到输入」当成同意；要显式放行请加 `--yes`（或 `echo y | sisyphus exec ...`）。

自动化场景（脚本 / CI）必须显式 `--yes`，否则模型无法执行命令 —— 这是刻意的
fail-safe：静默执行模型提出的任意命令是自动化入口最危险的行为。

> 权限闸（哪些命令允许执行）与 `--verify-cmd` 客观闸门（目标是否达成）是正交的
> 两件事；被拒绝的步骤会在报告里显示 `denied by user: builtin:bash_exec`。

### `--record`：把闭环结果接入质量指标

`sisyphus metrics quality` 聚合的是任务级记录（成功率 / 修复率 / 回归率 / 平均耗时）。
`dev --record` 把一次闭环落成同样的 `TaskRecord`：

```bash
sisyphus dev "修复 src/lib.rs 的编译错误" \
  --verify-cmd "cargo check" \
  --record docs/benchmarks/quality-metrics/records/l3-s1-diagnose-fix.json \
  --scenario diagnose-fix
```

- **闸门失败也落记录**（`status: fail`）—— 失败样本是成功率的分母；
- `initial_verify_passed` 取闸门**首轮**结果，`fix_attempts` 只统计闸门自动修复次数，
  步骤内自愈不计入；
- `regressed` 恒为 `false`：dev 不做跨任务回归检测，不为指标伪造数据；
- 没有客观闸门的 `dev` 无法判定成败，因此 `--record` 单独使用会被拒绝。

记录格式与聚合口径见 `docs/benchmarks/quality-metrics/records/README.md`。

> 本地 3B 模型的真实闭环验收（三场景 × 3 轮）与复现脚本见
> `docs/benchmarks/acceptance/2026-11-12/phase1/t1-4-l3-closed-loop.md`。

## 交互式 TUI

启动交互式终端界面：

```bash
sisyphus --interactive
```