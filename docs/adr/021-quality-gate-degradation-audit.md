# ADR-021: 质量门禁退化审计 —— 静态证据核对 + 语义退化检查（绝不静默放行）

状态：**Accepted**（2026-11-14）

日期：2026-11-14
关联：`scripts/audit_todo.py`、`scripts/audit_workflows.py`、`docs/known-issue-weak-model-json-fidelity.md`

## 背景

`audit_todo.py` 对 `AGENTS.md` 等文档中的 `- [x]` 勾选项做静态证据门禁：
证据必须可解析为真实存在、带测试属性、未被 `#[ignore]` 的 `path.rs::test_fn`，
或真实存在的 `make` 目标 / `scripts/*.py`。这挡住了「无证据勾选」，但暴露两类漏网：

1. **语义空泛**：证据格式合规但内容与描述无关（测试名真实存在却与勾选语义
   不沾边，或断言为空壳），静态核对无法识别；
2. **口径漂移**：swarm 多代理并行产出时，审计尺度随模型能力波动；弱模型下
   还叠加 JSON 保真度问题（`json_extract` 提取字段缺 fidelity，详见关联 known-issue）。

同类问题的已修实例：质量指标聚合在轮转子目录（`records/YYYY-MM/`）落地后
样本「消失」（`aggregate_dir` 曾只扁平读取）；`sandboxd` spawn 失败仅报通用
IO 错误，路径/errno 缺失导致 CI 误诊。

## 决策

1. **静态证据门禁保持为必要条件**，新增语义退化检查层：对勾选项做抽样核验
   （证据函数实际断言内容 ↔ 勾选语义的相关性、测试名与描述一致性），
   `scripts/audit_todo.py` 输出退化标记。
2. **绝不静默放行**（never silently approve）：发现语义空泛/伪造嫌疑时，
   该勾选项按未通过处理并在报告中显式标记，不得以「格式合规」放行。
3. **失败必须可见**：错误路径的可观测性本身算证据。spawn/启动类失败必须附带
   路径、存在性、IO kind/errno（`SandboxdClient::spawn` 诊断已按此增强）；
   模拟网关/降级链场景下，退化只改变成功率，不改变「失败可见」这一验证语义。
4. **文档计数约定**：文档中的单测计数只引用命令与实跑输出，不写硬编码数字
   （计数随演进取值漂移；证据以提交信息实跑输出为准）。
   **适用范围**：`docs/plans/`、`docs/runbooks/`、`book/`、`README`/`CONTRIBUTING`
   等**描述当前状态**的文档。**不追溯改写**历史档案（`docs/project-status-*`、
   `docs/project-retrospective-*`、`docs/adr/` 既有正文、`docs/benchmarks/**/runs/`
   原始输出、已归档的证据文档）——那些是当时运行的记录，改写即伪造历史；
   证据文档允许保留数字，但必须同时给出命令与输出文件路径。

## 落地现状（2026-11-14）

已落地：静态证据门禁（既有）、失败可见性增强（spawn 诊断 / 轮转递归聚合 /
样本规模自检 / L1 运行时证据）、文档计数约定**且已门禁化**
（`scripts/audit_doc_counts.py`，`make audit-doc-counts`，CI `Audit sources`；
含 `--self-check` 正反例与范围规则自检）、known-issue 记录。
待实施：语义退化检查层（抽样核验与退化标记输出）——本 ADR 冻结其契约
（绝不静默放行、失败必须可见），实现随 `scripts/audit_todo.py` 后续迭代落地。

## 后果

- 每次审计多一道语义抽样（抽样而非全量，成本可控）；
- `audit_todo.py` 退化规则表需随新退化模式持续演进（ratchet 只减不增的
  baseline 机制同步适用）；
- 错误诊断信息增强（路径/存在性/IO kind/errno），CI 误诊率下降；
- 质量指标聚合递归读取轮转子目录，样本不再因轮转「消失」。

## 替代方案（已否决）

- **仅扩充静态规则表**：规则永远滞后于新退化模式，且无法识别语义空泛；
- **全量人工复核**：不可持续，与自动化门禁初衷相悖；
- **全自动 LLM 裁决**：裁决本身不可审计，弱模型下会引入同类保真度问题。
