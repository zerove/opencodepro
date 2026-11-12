# T1.4 L3 真实闭环 — 验收证据

> 采集日期：2026-11-12　对应计划：`docs/plans/automation-development-plan-2026-11.md` §6 T1.4
> 被测代码：`a82dd1a`（`feat(dev)` 闸门 + `test(l3)` 脚本 + `fix(dev)` diff 上下文）；
> 之后仅新增「闸门失败 → 非零退出」硬化（见 §5.5），不影响通过路径的归档结果
> 状态：**DONE（本地真实模型）**；「云端 provider 首次真实调用」仍外部后置

## 1. 结论

三个真实仓库任务（诊断修复 / 质量门 / 功能新增）在本地真实 LLM 上形成
「改文件 → 验证失败 → 自动修复 → 验证通过」的完整闭环：

- **3 轮完整运行 × 3 场景 = 9/9 PASS**（每场景判定由外部命令退出码决定，不是 CLI 自报状态）。
- s2 质量门多次出现「首次修复不完整 → 闸门抓出 → 二次修复 → 通过」，修复次数与输出均已归档。
- 过程中发现并修复 **10 个真实缺陷**（见 §5），其中最重要的是：**成功标准原本由模型自选**，
  导致报告 SUCCESS 而目标未达成。§5.6–§5.9 由多文件重构探索（§4.5）暴露：
  计划重试丢失目标、`ComplexChange` 吞编辑失败、开发失败退出码为 0、
  闸门修复看不到被重写文件的当前内容、覆盖已有文件未如实标注。

## 2. 环境与方法

| 项 | 值 |
|---|---|
| 模型 | `qwen2.5-coder:3b`（本机 ollama，OpenAI 兼容端点） |
| 推理硬件 | 本机 CPU（无 GPU 加速；`OLLAMA_VULKAN=0`） |
| 客户端 | `target/debug/sisyphus --config <l3 配置> dev <goal> --verify-cmd <cmd>` |
| 请求归档 | `scripts/l3-llm-proxy.py` 插在 CLI 与 ollama 之间，逐次写 JSONL |
| 运行脚本 | `scripts/l3-run-scenarios.sh s1 s2 s3` |
| 超时 | 每场景 900s（整体另有 +60s 兜底） |

每次 LLM 调用归档字段：`ts / seq / trace_id / model / messages / json_mode /
status / duration_ms / usage(prompt+completion tokens) / response_content`。

复现：

```bash
# 需要本机 ollama 已启动并具备 qwen2.5-coder:3b
bash scripts/l3-run-scenarios.sh s1 s2 s3
# 原始归档：/tmp/opencode/l3/llm.jsonl 与 summary.md
```

## 3. 场景定义与判定

| 场景 | 初始工作区 | 验收命令（闸门=判定） | 附加判定 |
|---|---|---|---|
| s1 诊断修复 | `src/main.rs` 的 `let total = add(1, 2)` 缺分号 | `cargo check` | — |
| s2 质量门 | `src/lib.rs` 同时有 `bool_comparison` 与 `needless_bool` | `cargo clippy -- -D warnings` | — |
| s3 功能新增 | `tests/multiply.rs` 引用未定义的 `multiply`（独立文件，禁止修改） | `cargo test` | `src/lib.rs` 含 `fn multiply`，且 ≥1 个测试通过 |

s3 的测试放在**独立文件**：若放在 `src/lib.rs` 内，模型用 `CreateFile` 覆盖该文件时
会把测试一起删掉，`cargo test` 空跑通过 —— 实测发生过一次，因此改为外部测试文件。

## 4. 结果

### 4.1 三轮汇总

| run | 时间 | s1 | s2 | s3 | 合计 | LLM 调用 | prompt/completion tokens | LLM 总耗时 |
|---|---|---|---|---|---|---|---|---|
| run1 | 10:20:25 | PASS 42s / 1 次 | PASS 101s / 3 次 | PASS 52s / 1 次 | 3/3 | 5 | 2656 / 1635 | 183.3s |
| run2 | 10:23:33 | PASS 87s / 1 次 | PASS 44s / 2 次 | PASS 50s / 1 次 | 3/3 | 4 | 2070 / 1044 | 171.9s |
| run3 | 10:26:04 | PASS 52s / 2 次 | PASS 39s / 2 次 | PASS 51s / 1 次 | 3/3 | 5 | 2457 / 797 | 131.6s |

（耗时 = 该场景端到端墙钟时间，含 cargo 编译；LLM 调用数 = 该场景代理记录条数。）

### 4.2 闭环证据摘录

- **run1 s2（质量门，1 次修复）**：计划步骤全部 Success，闸门仍报
  `needless_bool` → `Step 9001` 整文件重写 → 闸门 `PASSED (1 fix attempt)`。
- **run3 s1（诊断修复，计划本身失败）**：计划中的 `EditFile` 因 `old_string`
  不匹配失败 → `Step 9001` 修复 → 闸门 `PASSED (1 fix attempt)`，
  整体状态 **PARTIAL FAILURE (1 / 2 steps completed)** —— 目标达成与过程失败分别记录，不互相掩盖。
- **s3（功能新增）**：模型在 `src/lib.rs` 实现 `multiply`，独立测试
  `multiply_works` 通过（`test result: ok. 1 passed`），闸门 0 次修复。

### 4.3 归档文件

```
phase1/
├── t1-4-l3-closed-loop.md          # 本文
├── summary.md                      # 阶段 1 汇总（T1.4 行已更新）
└── runs/
    ├── run{1,2,3}/
    │   ├── llm.jsonl               # 该轮全部请求/响应/token/耗时/trace
    │   ├── summary.md              # 该轮场景判定汇总
    │   └── s{1,2,3}-run.log        # 各场景 CLI 完整输出
    ├── run{1,2,3}.out              # 该轮脚本控制台输出
    ├── s4-exploratory/             # 多文件重构探索（FAIL，见 §4.5）
    ├── gate-exit-code.log          # 闸门失败退出码实测（附录 §5.5）
    ├── dev-exit-code-llm-failure.log  # 开发失败退出码实测（附录 §5.8）
    └── pre-diff-s2-failure.out     # 加 diff 上下文之前的一次真实失败（见 §5.4）
```

> 仓库根 `.gitignore` 的 `*.log` 曾把这些验收日志挡在版本库外（文档引用、实际未入库）。
> 现已在 `.gitignore` 为 `docs/benchmarks/acceptance/**/*.log` 开例外并补交缺失文件。

### 4.4 同一闭环进入质量指标（T4.1）

`dev --record <path>` 把一次闭环落成 `TaskRecord`，使 L3 结果不只是日志，
而是可聚合的样本。下表为一次独立的记录运行（2026-11-12，同一模型与场景，
3/3 PASS）：

| 场景 | 闸门命令 | 首轮 | 最终 | 闸门修复次数 | 耗时 | token（输入+输出） |
|---|---|---|---|---|---|---|
| s1 `diagnose-fix` | `cargo check` | FAIL | PASS | 1 | 48.0s | 1082 + 310 = 1392 |
| s2 `quality-gate` | `cargo clippy -- -D warnings` | FAIL | PASS | 1 | 35.9s | 1262 + 370 = 1632 |
| s3 `feature` | `cargo test` | FAIL | **FAIL** | 2 | 140.0s | 2016 + 641 = 2657 |

这一轮**含失败样本**（s3 失败：3B 模型把函数当模块写 `pub mod multiply;`）。
记录已归档到 `docs/benchmarks/quality-metrics/records/l3-s{1,2,3}-*.json`，
与既有 4 条仓库 `verify` 记录合并后共 7 条，聚合结果：

```
success_rate 0.857 / fix_rate 0.667 / regression_rate 0.0 / mean_duration 65.6s
mean_total_tokens 1893.7（仅 3 条有 token 的记录参与；verify 记录为 null）
```

> 失败样本是成功率的分母 —— 只归档成功样本会让指标失真（上一版记录全是成功
> 样本，聚合出来 1.0，看着比实际好）。

token 来自 provider 上报的 usage（`prompt_tokens` / `completion_tokens`）：
`dev` 链路自动写入记录，provider 未上报时留空而不是补 0，聚合时也不参与均值。
产物见 `docs/benchmarks/quality-metrics/` 下最新快照（bridge 未运行时
token/http 全局指标为 `null`，不伪造）。

口径说明：

- `initial_verify_passed` 取闸门首轮，`fix_attempts` 只计闸门自动修复，
  步骤内自愈不计入 —— 因此 s1/s2 的「首轮失败 → 修复通过」才能被修复率看见；
- 闸门失败也会落记录（`status: fail`），失败样本是成功率的分母；
- `regressed` 恒为 `false`：dev 不做跨任务回归检测，该字段在 dev 链路不具信息量；
- `--record` 必须配 `--verify-cmd`，否则 CLI 直接拒绝 —— 没有客观闸门就没有成败。

### 4.4b 归档的「9/9 PASS」包含了静默破坏（判定加严后的更正）

原始归档（§4.1 的 9/9）用的是**只判闸门**的判定：s3 只要求「`fn multiply`
存在 + `cargo test` 通过」。复核归档日志发现，模型当时的动作是
`CreateFile: src/lib.rs` —— **整文件覆盖**，把原有的 `pub fn add` 删掉了；
因为测试只用到 `multiply`，闸门照样 PASS，于是被记成成功：

```
Step 1: Status: Success
Output: Created file: src/lib.rs        ← 整文件覆盖：pub fn add 已被删掉
Objective verification: PASSED (`cargo test`, 0 fix attempt(s))
```

即：**「9/9 PASS」里有一部分是「任务通过但顺手毁了不相关代码」**。现在把 s3 的
判定加严为「既有的 `pub fn add` 必须保留」，本地 3B 模型**做不到**（它总是
整文件覆盖），s3 在严格判定下 FAIL：

| 场景 | 旧判定（只判闸门） | 新判定（闸门 + 保留既有公开 API） |
|---|---|---|
| s1 诊断修复 | PASS | PASS |
| s2 质量门 | PASS | PASS |
| s3 功能新增 | PASS（**含删除 `pub fn add`**） | **FAIL**（3B 模型整文件覆盖） |

这是**测量口径变严**导致的结果，不是回归：旧口径把「静默破坏」算成成功。
引擎侧已加两道防线（见 §5.11–§5.12）：闸门修复不得删除公开 API（回滚），
以及闭环结束后的公开 API 删除检测与提示。

### 4.5 多文件重构缺口（s4 探索场景，3B 模型 FAIL）

在三个场景之外新增了 `s4`（多文件重构）以覆盖原「未覆盖」项，**opt-in**：

```bash
bash scripts/l3-run-scenarios.sh s4     # 不在默认集合内
```

任务：`add()` 与 `format_sum()` 同处 `src/lib.rs`，测试从 `l3_s4::math::add`
引用 → 要求把 `add()` 拆到新文件 `src/math.rs` 并声明模块。判定：`cargo test`
通过、`math.rs` 存在、`lib.rs` 不再定义 `add`（复制粘贴不算重构）。

本地 `qwen2.5-coder:3b` 在 3 次尝试（不同提示策略）均 FAIL，典型失败形态：

- 规划阶段把 `ReadFile`（动作类型）当作 `verification.type` → 计划解析失败；
- 闸门修复重写 `src/lib.rs` 时凭空加入 `#[cfg(test)] mod tests;`（仓库里没有
  `src/tests.rs`）、或把 `format_sum` 的 `format!` 写残；
- 第一次探索甚至把整个目标换成「新建 Hello World 项目」并覆盖了 `Cargo.toml`。

其中前两项是**产品缺陷**（见 §5.6–§5.8），已修复；最后一项属模型能力边界：
3B 模型可完成单文件小改动（s1–s3 稳定通过），但多文件重构超出其能力。

本轮任务记录（FAIL）：`runs/s4-exploratory/l3-s4-refactor.json`
（`initial_verify_passed=false`、`fix_attempts=2`、`duration_ms=51295`），
原始请求/响应 `runs/s4-exploratory/llm.jsonl`，CLI 输出 `s4-run.log`。

**结论**：s4 目前用作「缺口覆盖 + 缺陷发现器」，不计入通过率；
MCP 场景仍未覆盖。

## 5. 过程中发现并修复的缺陷（均来自真实运行）

### 5.1 成功标准由模型自选 → 假 SUCCESS（核心缺陷）

首次运行 s2：模型在计划里声明 `verification: CargoCheck`，只修了
`bool_comparison`、漏掉 `needless_bool`。由于警告不是错误，`cargo check` 通过，
报告 **SUCCESS**，而 `cargo clippy -- -D warnings` 仍失败。

修复：新增 `dev --verify-cmd <CMD>` 作为**用户给定的客观闸门**，未通过则整体判
Failed；失败时带着输出自动修复（`--verify-fix-attempts`，默认 2）。

### 5.2 弱模型的编辑形态不兼容

- 修复环节要求裸数组 `[{path,old_string,new_string}]`，3B 模型返回单对象、
  `{"edits":[...]}` 包裹或 ```json 围栏 → 增加 `parse_edit_requests` 兼容。
- 更关键：模型很难精确复现 `old_string`（换行/缩进一错即 `no match`）。增加**整文件
  重写**通路 `{"files":[{path,content}]}`，并在 prompt 中优先推荐。

### 5.3 文件内容被套上 markdown 围栏

模型返回的 `content` 形如 ` ```rust\npub fn ...\n``` `，直接落盘后
`error: expected one of ! or ::, found keyword pub`。落盘前剥离首尾围栏
（`strip_code_fence`，仅首尾都是围栏时生效）。

### 5.4 模型看不到自己引入的回归

s2 的一次运行：模型把 `pub fn is_ready` 改成私有函数，`clippy -D warnings`
报 `dead_code`。修复环节只看当前文件与错误行，两次尝试继续输出私有版本
（其中一次用 `#[allow(dead_code)]` 压告警），闸门如实判 FAILED。

修复：闸门修复 prompt 注入 `git diff`（工作区相对 HEAD）并要求「不得降低可见性/
破坏公开 API」；场景脚本为每个工作区建立 git 基线。加此改动前同一模型下
s2 曾 2/4 失败，加之后 3 轮 3/3 通过（样本小，见 §6）。

原始失败输出：`runs/pre-diff-s2-failure.out`。

### 5.5 闸门失败仍返回 0，自动化无法感知

验收脚本以退出码判断成败，而 `handle_dev` 在闸门失败时只打印 `❌` 并返回 `Ok`，
`rc=0` —— 脚本必须再跑一遍外部命令才能发现失败。修复：闸门失败直接
`anyhow::bail!`，CLI 退出码非零。

实测（真实模型 + 必然失败的闸门）：

```bash
sisyphus --config /tmp/opencode/l3-ollama.toml dev "创建一个文件 note.txt，内容为 ok" \
  --verify-cmd false --verify-fix-attempts 0 --timeout-secs 300
# ... ❌ Objective verification failed after 0 fix attempt(s): `false`
# rc=1
```

输出见 `runs/gate-exit-code.log`。该改动晚于 §4 的归档运行；通过路径（闸门 PASS）
的退出码仍为 0，故归档结果不受影响。

### 5.6 计划重试丢失原始目标

`plan()` 在首次 JSON 解析失败后，只把报错信息发给模型重试，**没有重发目标与
项目上下文**。实测 3B 模型因此把「把 add 拆到 src/math.rs」重试成「新建
Hello World 项目」，并且该计划被真正执行 —— 用户仓库的 `Cargo.toml` 被覆盖。

修复：重试 prompt 带上原始 `user_prompt`（目标 + 上下文）、上一次响应片段，
并显式列出合法的 `verification.type`（模型常把动作类型当验证类型）。
单测 `plan_retry_keeps_goal_and_context` 断言重试 prompt 仍含目标与上下文标记。

### 5.7 `ComplexChange` 把编辑失败吞成成功

`ComplexChange` 分支对每个编辑失败只记录 `Warning: ...`，最后仍返回
`StepStatus::Success` —— 上层认为步骤完成，不再修复。与本项目已修的
「成功标准由模型自选」是同一类缺陷。

修复：任一编辑失败（或分解出 0 条编辑）即返回 `Err`；单测
`complex_change_edit_failure_is_an_error` 覆盖。

### 5.8 开发失败时 CLI 退出码仍为 0

闸门失败此前已修为非零（§5.5），但「连计划都拿不到」「LLM 不可达」「超时」
仍走 `eprintln! + Ok(())`，自动化脚本无法感知。修复后实测：

```bash
# 指向不存在的模型
sisyphus --config <bad-model> dev "do nothing" --timeout-secs 60
# ❌ Development failed: ... LLM connect failed ...
# rc=1
```

输出见 `runs/dev-exit-code-llm-failure.log`。

### 5.9 闸门修复在看不到当前内容的情况下重写文件

修复 prompt 只注入错误点名的文件；对于「修复时顺手重写另一个已改动文件」
的情况，模型看不到该文件当前内容，只能凭记忆重建 —— 实测把 `src/lib.rs`
的 `format_sum` 整段丢掉。修复：`gate_file_context` 现在还会收集
`git status --porcelain` 里已改动/新增的源文件（最多 4 个，每个 3000 字符），
并在提示中要求保留不相关代码。

### 5.10 覆盖已有文件时没有如实标注

`CreateFile` 落到已存在的文件上时，输出恒为「Created file: …」，与真正的新建
无法区分。§4.5 的探索里，模型把用户仓库的 `Cargo.toml` 覆盖成了另一个项目，
而报告里看不出任何覆盖痕迹。

现在区分标注：新建为 `Created file: {path}`，覆盖为
`Overwrote existing file: {path} (was N bytes)`。**不**改成拒绝覆盖 ——
弱模型常用 `CreateFile` 表达「整文件重写」（精确 Edit 对它们太难，这正是
§5.2 的背景），拒绝会把可完成的任务变成不可完成；改为如实标注 + 由客观闸门
兜底。单测 `create_file_reports_overwrite_of_existing_file`。

### 5.11 provider 会静默截断超长 prompt（目标与 schema 都被裁掉）

本地 ollama 的有效窗口远小于模型标称的 32k（实测 37KB 的 prompt 被裁到
**2050 token**，保留**尾部**、丢掉**开头**，且**不报错**）。后果：

- 目标写在 prompt 最前面 → **被裁掉** → 模型把「拆 add 到 src/math.rs」误解成
  「实现 helper_320」；
- system prompt 里的完整 schema 被裁掉 → 模型只回一句摘要而不是计划。

修复（三处）：

1. **检测**：比较「我们发出的 token 估算」与「provider 上报的 prompt token」，
   差距 > 30% 即判定被裁剪（`DynamicLlmClient::prompt_truncated()`），CLI 在
   `dev` 成功与失败两条路径上都提示「模型可能没有看到上下文开头」。
2. **目标前后都放**：保留「第一句是目标」（弱模型友好），同时在末尾重复一次
   （尾部不会被裁）。
3. **末尾加精简 schema 提醒**：长上下文时 system prompt 的 schema 会被裁掉，
   末尾提醒能保住格式要求。**刻意用描述而非可照抄的 JSON 样例** —— 带占位值
   的样例会被弱模型直接抄进输出（实测它把 `"path": "..."` 原样写进计划，
   导致 s2/s3 连续回归）。

### 5.12 闸门修复会删掉用户的公开 API，而闸门照样 PASS

闸门修复的目标是「让命令通过」，但弱模型在全文件重写时会把不相关的公开函数
一起删掉；而闸门只检查被测行为，**发现不了**。实测 s3 里修复把 `pub fn add`
删了，`cargo test` 依然 PASS（测试只用到 `multiply`）。

修复分两层：

1. **补回**（首选）：应用改动前留快照，比对公开 API 签名；新内容**遗漏**了
   既有公开 API 时，把缺失的代码块**补回去**，并如实报告补了什么
   （「已补回 N 处被遗漏的既有公开 API」）。
   为什么是补回而不是拒绝：拒绝会让「加个函数」这类任务在弱模型下**永远**
   做不成（实测 s3 三次全败），而补回既保住用户的代码，也让任务真正完成。
2. **允许显式删除**：闸门验证输出**点名**要删的 API 时允许删（否则「删除某个
   函数」类任务永远做不成）。

同一套防线也用在**计划步骤**上（整文件重写同样会遗漏既有 API），以及闭环
结束后的 `git diff` 检测与提示（CLI）。

### 5.13 计划 JSON 差一点就整份作废

`serde` 严格解析要求每个字段齐全：弱模型漏写 `goal`、或把动作类型当
`verification.type`（`{"type":"ReadFile"}`），都会让**整份计划**作废并触发
重试（3B 每次重试要 30–60s）。现在严格解析失败时走**宽松解析兜底**：缺
`goal` 用引擎已知的目标补上，未知 `verification` 退化为 `None`（步骤级自检
不是目标判据），动作本身认不出仍然报错（不猜）。单测见
`plan_parsing::tests::*`。

### 5.14 执行失败（编辑匹配不上）不走模型纠错路径

`run_step_with_fix` 原本只在**验证失败**时让模型纠错；**执行失败**
（`EditFile` 的 old_string 匹配不上、命令跑不起来）直接失败返回 —— 而这类
错误信息恰恰是模型最需要的上下文。现在执行失败也会走一次纠错。

## 6. 诚实性说明与局限

- **本地 3B 模型**：结果不代表云端大模型能力；`qwen2.5-coder:3b` 在 CPU 上
  单次计划调用约 40–90s，是耗时主因。
- **成本字段**：本地推理无计费，归档只记录 token 用量（prompt/completion），
  不填 USD 金额。
- **trace**：CLI 目前不发送 trace 头，`llm.jsonl` 中的 `l3-00N` 是记录代理生成的
  调用序号 id，用于归档内关联；**端到端 trace 传播仍属 T4.4 范围**。
- **样本量**：最终 3 轮 9/9（**旧判定口径**，见 §4.4b 的更正）；改动前存在 s2
  失败（2/4）。模型输出具随机性，该数据不构成通过率保证。
- **判定加严后的真实通过率**：加严判定 + 补回机制后为 **7/9**（s1/s2 稳定
  PASS，s3 偶发把函数当模块写）。本地 3B 模型的稳定性有限，样本量也小，
  该数字不构成通过率保证。
- 场景工作区是最小 Rust crate，不是 sisyphus 仓库自身的任务；多文件重构已由
  s4 探索场景覆盖（3B FAIL，见 §4.5）；**MCP 已由独立实现的标准服务器覆盖**
  （CLI + 引擎两级 10/10 PASS，见 `l3-mcp-scenario.md`）；长上下文仍未覆盖。
- §4 的 9/9 归档来自修复前代码；本轮产品修复后已重跑 s1–s3 = 3/3 PASS
  （`s1` 122s / `s2` 29s / `s3` 69s），未替换已归档记录。
- 所有判定均来自外部 `cargo` 命令退出码；CLI 自报状态仅作对照，不作证据。
