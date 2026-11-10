# SOP 模板资产（单机，P2-A）

本目录提供可复用的 SOP/DAG 模板示例（JSON），用于演示与二次定制（含受控并行分支结构）。

## 文件

- `verify-sequence.json`：最小顺序两步模板（对应 `verify_sop` 结构的可保存形式）。
- `quality-gate-sequence.json`：顺序四步“质量门”模板骨架（格式/静态检查/测试/汇总占位命令，可按项目替换）。
- `parallel-explore-aggregate.json`：**扇出/汇聚** DAG 模板（`prepare -> {explore_a, explore_b} -> aggregate`），用于表达“受控探索性并行 + 汇总”的编排模式（实际并行调度语义以引擎执行器为准，可审计结构优先）。
- `parallel-triple-review.json`：**三路并行审查 + 汇聚** DAG 模板（准备 → 正确性/契约/可观测性并行审查 → 汇总），用于质量评审或多视角探索场景。
- `scenario-code-review-gate.json`：场景模板（并行代码评审门禁：正确性/契约兼容/质量观测并行检查 + 汇总）。
- `scenario-diagnose-fix-verify.json`：场景模板（诊断→修复→验证顺序流程，占位命令需替换为项目实际命令）。

## 保存与使用（需 bridge + 工作区上下文）

`POST /api/sops` 接受 `SopSaveRequest`：`{id,name,version,config}`，需 `X-Sisyphus-Workspace-Id`。

```bash
BASE="${BASE:-http://127.0.0.1:8080}"
WS="${WS:-default}"

curl -s -X POST "$BASE/api/sops" \
  -H 'Content-Type: application/json' \
  -H "X-Sisyphus-Workspace-Id: $WS" \
  --data-binary @docs/examples/sop-templates/parallel-explore-aggregate.json

curl -s "$BASE/api/sops"
curl -s -H "X-Sisyphus-Workspace-Id: $WS" "$BASE/api/sops/parallel_explore_aggregate_template"
```

> 实际命令、节点类型与参数应按目标环境调整；执行前确认执行器权限与依赖。`Test` 节点 `workdir` 可为 `null` 或指定路径。

## 关联

- `docs/plans/single-node-orchestration-templates.md`
- `docs/runbooks/single-node-sop-e2e-guide.md`
- `docs/runbooks/single-node-typical-workflow.md`
- `scripts/single-node-demo-flow.sh`（端到端演示流程）
