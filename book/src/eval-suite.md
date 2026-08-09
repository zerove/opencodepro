# 评测套件

Sisyphus 通过 `evals/` 目录对专业代理（Metis、Momus、Librarian、Scribe、SecurityOracle）进行量化能力评测。评测以 JSON 用例驱动，运行结果输出得分表。

## 运行方式

```bash
make eval            # 等价于 cargo run -p eval-runner
make eval-check      # 仅检查编译
```

评测器在 `evals/runner.rs` 中实现，自动加载 `evals/<agent>/cases.json` 下的全部用例，按代理分组执行，并汇总得分。

## 用例结构

每个代理目录下的 `cases.json` 是一组用例（Case）。字段如下：

| 字段 | 类型 | 说明 |
|----------|---------|----------|
| `id` | string | 用例唯一标识 |
| `agent` | string | 目标代理：`metis`、`momus`、`librarian`、`scribe`、`SecurityOracle` |
| `input` | object | 传入工具的参数（随代理而异） |
| `expected` | object | 基准输出或关键组成 |
| `criteria[]` | array | 判据列表 |
| `metadata` | object（可选）| 类别与难度（`easy`/`medium`/`hard`） |

> 用例结构由 `evals/schema.json`（JSON Schema draft-07）定义。`eval-runner` 启动时会用该 schema 校验全部用例文件，不合规则立即报错退出，防止无效用例混入。

## 判据类型

每个 `criteria` 条目通过 `check_type` 选择验证方式：

| 类型 | 说明 | 示例 `expected_value` |
|------|------|--------|
| `keyword` | 输出文本包含指定关键字 | `## Context` |
| `regex` | 输出文本匹配正则 | `\[Core Conclusions\].*\[Detailed Evidence\]` |
| `json_schema` | 输出 JSON 匹配子 schema | `{"type": "object", ...}` |
| `llm_judge` | LLM 判官评估输出是否符合语义标准 | `The decision must explicitly state ...` |

`weight`（默认 1.0）控制判据在用例总分中的占比。用例得分为通过的判据加权和除总权重，全部通过才算 PASS。

## 代理与用例覆盖

当前共 57 个用例：

| 代理 | 覆盖主题 | 用例数 |
|------|----------|--------|
| `metis` | 需求分析：可执行性判断、模糊识别、隐含需求（背压/异步/连接池等） | 12 |
| `momus` | 评审：计划（测试/指标/顺序）、代码（UAF/死锁/复杂度/SRP）、架构（过度设计/耦合/循环依赖） | 11 |
| `librarian` | 资料综合：去冗余、归因、冲突消解、深度技术合成、噪声过滤、版本差异 | 10 |
| `scribe` | 文档：ADR（MADR 格式）、文档更新、决策提取 | 12 |
| `security` | 安全审计：内存安全、注入、权限、协议 DoS | 12 |

## 评测模式

`EvalRunner` 依据环境变量选择 LLM 客户端：

- `LLM_API_KEY=mock`（默认）：使用内置 `MockLlmClient`，所有代理工具直接返回固定结构。此模式用于验证管线与结构，无真实智能，`llm_judge` 判据会按结构匹配。
- 设置真实 `LLM_API_KEY`/`LLM_ENDPOINT`/`LLM_MODEL`：使用 `DynamicLlmClient` 调用真实模型，全量判据（含 `llm_judge`）按语义评估，得到真实能力得分。

## 基线报告

`evals/capability_baseline_report.md` 记录 2026-06-24 的初始基线（总体 0.86，47/57 通过）。每次重大代理提示词或工具变更后，应重新运行 `make eval` 并更新报告。

## 添加用例

1. 确定目标代理与覆盖缺口，选择 `evals/<agent>/cases.json` 追加用例；
2. 明确 `check_type`：结构固定用 `keyword`/`regex`，语义判断用 `llm_judge`，大对象校验用 `json_schema`；
3. 运行 `make eval-check` 确认通过编译；
4. 运行 `make eval` 确认新用例被加载并校验。