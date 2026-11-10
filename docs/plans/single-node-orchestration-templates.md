# 单机编排协作模板（SOP/DAG，P2-A）

> 目标：以 Sisyphus 原生优势（认知循环 + SOP/DAG 编排 + 可观测）提供可复用的多步骤任务模板，而非复刻外部 Swarm 形态。

## 1. 设计原则

- **可审计**：步骤、依赖与结果可通过轨迹/观测端点追踪。
- **可复用**：模板参数化（目标、输入、约束），适配常见单机任务。
- **可降级**：依赖缺失（LLM/工具/MCP）时明确失败与指引，不静默成功。
- **单机优先**：不依赖分布式控制面写入能力。

## 2. 候选模板（示例方向）

| 模板 | 场景 | 关键步骤（示意） | 观测点 |
|---|---|---|---|
| 代码理解与总结 | 阅读仓库/模块并产出结构化摘要 | 检索/读取 → 分析 → 汇总报告 | 执行 trace、认知追踪 |
| 变更实施与验证 | 小范围修改 + 构建/测试 | 计划 → 编辑 → 执行命令/测试 → 反思 | 节点状态、命令结果 |
| 问题诊断 | 复现/定位/建议修复 | 收集信息 → 假设 → 验证 → 结论 | trace/日志关联 |
| 文档/规范同步 | 代码与文档一致性检查 | 扫描 → 对比 → 生成变更建议 | 输出与差异摘要 |

> 具体 SOP 定义与可用性取决于引擎内置/已注册 SOP 与工具配置；本文件作为模板规划与落地索引。

## 3. 落地路径

1. **盘点现有 SOP**：通过 `GET /api/sops`、CLI `sop list` 确认可用 SOP 与语义。
2. **定义最小模板**：为 1–2 个高价值场景确定输入/输出契约与观测要求。
3. **端到端验证**：按 `single-node-sop-e2e-guide.md` 在具备配置的环境执行并记录证据。
4. **文档化**：补充场景说明、示例请求与预期观测，纳入 `single-node-typical-workflow.md` 或独立 cookbook。

## 4. 验收要点（P2-A 初始）

- 至少 1 个模板具备可复现的执行与观测说明（在配置满足时）。
- 失败路径可解释（依赖/权限/契约）。
- 与 P1-E（SOP 端到端）与观测能力一致。

## 5. 关联

- `docs/plans/p1-p2-roadmap.md`（P2-A）
- `docs/runbooks/single-node-sop-e2e-guide.md`
- `docs/runbooks/single-node-typical-workflow.md`

## 6. 现有示例与模板资产（P2-A 落地）

### 6.1 内置示例：`verify_sop`（单机可发现，2026-11-10 实测）

- `GET /api/sops` → `["verify_sop"]`
- `GET /api/sops/verify_sop` 返回示例图：`a(Test: echo hi)` → `b(Test: echo ho)`，`edges=[[0,1]]`
- **实测执行**：CLI `sop run verify_sop` 成功返回 `Test Passed` 与 `CognitionReport`（直接 SOP 路径）；`POST /api/execute/goal` 在无可用 LLM 时返回可解释错误（路径差异见 `single-node-sop-e2e-guide.md`）。

### 6.2 可保存模板资产（含受控并行探索结构）

- `docs/examples/sop-templates/verify-sequence.json`：最小顺序模板。
- `docs/examples/sop-templates/quality-gate-sequence.json`：顺序四步质量门骨架。
- `docs/examples/sop-templates/parallel-explore-aggregate.json`：扇出/汇聚 DAG（`prepare -> {explore_a, explore_b} -> aggregate`），用于表达“受控探索性并行 + 汇总”的可审计编排。
- `docs/examples/sop-templates/parallel-triple-review.json`：三路并行审查 + 汇聚 DAG（准备 → 正确性/契约/可观测性并行审查 → 汇总）。
- `docs/examples/sop-templates/README.md`：保存、查询与使用说明（需 `X-Sisyphus-Workspace-Id`）。

**实测（2026-11-10）**：`parallel_explore_aggregate_template` 通过 `POST /api/sops` 保存成功（201），可被列出/查询；CLI `sop run parallel_explore_aggregate_template`（直接 SOP 路径）执行成功并返回结果与 CognitionReport（预测路径含 `prepare -> explore_path_a -> aggregate`）。

**模板化建议**：在骨架基础上替换为项目真实命令/节点类型，按需扩展并行分支与汇聚策略，并通过执行状态/trace/观测端点关联每步结果；失败路径需可解释。
