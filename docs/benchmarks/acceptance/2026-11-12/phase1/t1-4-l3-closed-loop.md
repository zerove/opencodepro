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
- 过程中发现并修复 **4 个真实缺陷**（见 §5），其中最重要的是：**成功标准原本由模型自选**，
  导致报告 SUCCESS 而目标未达成。

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
    └── pre-diff-s2-failure.out     # 加 diff 上下文之前的一次真实失败（见 §5.4）
```

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

## 6. 诚实性说明与局限

- **本地 3B 模型**：结果不代表云端大模型能力；`qwen2.5-coder:3b` 在 CPU 上
  单次计划调用约 40–90s，是耗时主因。
- **成本字段**：本地推理无计费，归档只记录 token 用量（prompt/completion），
  不填 USD 金额。
- **trace**：CLI 目前不发送 trace 头，`llm.jsonl` 中的 `l3-00N` 是记录代理生成的
  调用序号 id，用于归档内关联；**端到端 trace 传播仍属 T4.4 范围**。
- **样本量**：最终 3 轮 9/9；改动前存在 s2 失败（2/4）。模型输出具随机性，
  该数据不构成通过率保证。
- 场景工作区是最小 Rust crate，不是 sisyphus 仓库自身的任务；尚未覆盖 MCP /
  多文件重构 / 长上下文场景。
- 所有判定均来自外部 `cargo` 命令退出码；CLI 自报状态仅作对照，不作证据。
