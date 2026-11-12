# 任务级质量记录目录

本目录存放**任务级质量记录**（每个文件一条 JSON），供 `sisyphus metrics quality` 聚合：

- 任务成功率 = pass / (pass + fail)（blocked 单独计数）
- 任务修复率 = 初次验证失败且最终通过 / 初次验证失败
- 任务回归率 = 引入回归 / 执行过修复
- 平均单任务耗时

## 记录格式

```json
{
  "task_id": "my-task",
  "scenario": "diagnose-fix",
  "status": "pass",
  "initial_verify_passed": false,
  "final_verify_passed": true,
  "fix_attempts": 1,
  "regressed": false,
  "duration_ms": 188,
  "prompt_tokens": 1024,
  "completion_tokens": 256,
  "engine_commit": "79bebcf826ee",
  "engine_dirty": false,
  "recorded_at": "2026-11-11T00:00:00Z"
}
```

`prompt_tokens` / `completion_tokens` 只在 provider 真的上报 usage 时存在
（`dev` 链路自动写入）；缺失表示「未知」，聚合时该记录不参与 token 均值。

`engine_commit` / `engine_dirty` 是**样本归因**字段：记录这条样本的是哪个引擎
版本、当时工作区是否干净。缺了它们，「成功率 0.857」无法归因到具体版本 ——
不同版本的样本混在这个扁平目录里聚合，比率变化可能只是样本构成变了，而不是
引擎变好/变坏。`engine_dirty: true` 表示产出样本时工作区**有未提交改动**，
该样本**不对应任何已提交版本**，聚合时会单独分组（`sha+dirty`）并显式提示。

本目录现有的 7 条记录产出于这两个字段引入之前，因此没有版本信息；聚合时显示
「引擎版本: 未知」。**这是刻意的** —— 不去猜测历史版本号，猜错比不知道更糟。

## 生成方式

两条链路都会写出同样格式的 `TaskRecord`：

```bash
# 1) 验证-修复循环（不涉及 LLM 规划）
sisyphus verify --record docs/benchmarks/quality-metrics/records/<task>.json

# 2) dev 闭环（LLM 规划 + 客观闸门）；--record 必须配 --verify-cmd
sisyphus dev "<目标>" \
  --verify-cmd "cargo test" \
  --record docs/benchmarks/quality-metrics/records/<task>.json \
  --scenario feature
```

`dev --record` 的语义：

- **闸门失败也会落记录**（`status: fail`）—— 失败样本是成功率的分母，不能只记成功；
- `initial_verify_passed` 是闸门**首轮**结果，`fix_attempts` 是闸门自动修复次数，
  二者共同定义修复率；
- `regressed`：`dev` 链路现在会检测「既有公开 API 被删除」（闸门检查不到这一类
  破坏：实测模型整文件覆盖删掉 `pub fn` 而闸门仍 PASS）并写入该字段；
  `verify` 链路不检测这一类破坏，恒为 `false`。

`scripts/l3-run-scenarios.sh --records-dir <dir>` 会在三场景运行时自动落记录。

聚合与采集：

```bash
sisyphus metrics quality --records docs/benchmarks/quality-metrics/records --json
./scripts/quality-metrics-collect.sh
```

无记录时各比率为 `null`（不伪造 0 值）。

## 目录内容

| 文件 | 来源 | 说明 |
|---|---|---|
| `repo-sisyphus-tools-build.json` | 真实仓库任务 | `sisyphus verify -p sisyphus-tools-build --max-warnings 0`（PASS，0 错 0 警，78.977s） |
| `repo-sisyphus-types.json` | 真实仓库任务 | 同上（`sisyphus-types`，PASS，19.782s） |
| `repo-sisyphus-metrics.json` | 真实仓库任务 | 同上（`sisyphus-metrics`，PASS，46.028s） |
| `repo-sisyphus-tools-agent.json` | 真实仓库任务 | 同上（`sisyphus-tools-agent`，PASS，90.770s） |
| `l3-s1-diagnose-fix.json` | L3 闭环（本地 ollama `qwen2.5-coder:3b`） | `dev` 诊断修复场景（闸门 `cargo check`）：首轮 FAIL → 修复 1 次 → PASS，48.0s，1392 token |
| `l3-s2-quality-gate.json` | L3 闭环（本地 ollama `qwen2.5-coder:3b`） | `dev` 质量门场景（闸门 `cargo clippy -- -D warnings`）：首轮 FAIL → 修复 1 次 → PASS，35.9s，1632 token |
| `l3-s3-feature.json` | L3 闭环（本地 ollama `qwen2.5-coder:3b`） | `dev` TDD 跨文件场景（闸门 `cargo test`）：首轮 FAIL → 修复 2 次仍 FAIL，**失败样本**（3B 模型把函数当模块写），140.0s，2657 token |

> 上述 s3 失败样本的根因后来被定位并修掉：**闸门修复的上下文里没有测试文件**，
> 模型看不到「`use x::y;` 是函数导入」，于是把函数写成了模块。修复后 s3 单独
> 复跑 4/4、完整轮次 2×3/3 通过（见 `phase1/t1-4-l3-closed-loop.md` §4.4c）。
> 这条失败记录**保留在目录里**：它是失败样本、是成功率的分母，也是该缺陷存在过
> 的证据；删掉它只会让指标更好看。

### 第二、三轮（带引擎版本归因）

修复落地后重跑两轮完整 L3（s1+s2+s3），6/6 PASS。这 6 条记录**携带引擎版本**
（`engine_commit: 51503830c53b` / `engine_dirty: false`），可用于按版本归因；
前面 7 条（4 仓库 `verify` + 3 条旧 L3）产出于版本字段引入之前，没有版本信息，
聚合时单列为「未携带版本信息」。

| 文件 | 来源 | 说明 |
|---|---|---|
| `l3-run2-s1-diagnose-fix.json` | L3 第二轮 | `dev` 诊断修复（闸门 `cargo check`）：首轮 FAIL → 修复 1 次 → PASS，1403 token |
| `l3-run2-s2-quality-gate.json` | L3 第二轮 | `dev` 质量门（闸门 `cargo clippy -- -D warnings`）：PASS，1638 token |
| `l3-run2-s3-feature.json` | L3 第二轮 | `dev` TDD 跨文件（闸门 `cargo test`）：PASS，1860 token |
| `l3-run3-s1-diagnose-fix.json` | L3 第三轮 | 同上，PASS，1392 token |
| `l3-run3-s2-quality-gate.json` | L3 第三轮 | 同上，PASS，1374 token |
| `l3-run3-s3-feature.json` | L3 第三轮 | 同上，PASS，1709 token |

> 目录内**多轮同名场景并存**是刻意的：每条记录是一次独立执行，聚合时各算一个
> 样本。同一文件名的后一轮会覆盖前一轮，所以归档时用 `run2` / `run3` 后缀区分。

> 归档的是**一整轮真实运行**的结果，含失败样本 —— 失败样本是成功率的分母，
> 只留成功样本会让指标失真。该轮的判定已加严（s3 还要求既有公开 API 保留，
> 见 `phase1/t1-4-l3-closed-loop.md` §4.4b）。

受控 fixture 样本（用于验证口径与链路，非仓库任务）单独放在
`../fixture-samples/`，不混入本目录。
