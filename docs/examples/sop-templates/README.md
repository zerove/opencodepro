# SOP 模板资产（单机，P2-A）

本目录提供可复用的 SOP/DAG 模板示例（JSON），用于演示与二次定制。

## 文件

- `verify-sequence.json`：最小顺序两步模板（对应 `verify_sop` 结构的可保存形式）。
- `quality-gate-sequence.json`：顺序四步“质量门”模板骨架（格式/静态检查/测试/汇总占位命令，可按项目替换）。

## 保存与使用（需 bridge + 工作区上下文）

`POST /api/sops` 接受 `SopSaveRequest`：`{id,name,version,config}`，需 `X-Sisyphus-Workspace-Id`。

```bash
BASE="${BASE:-http://127.0.0.1:8080}"
WS="${WS:-default}"

curl -s -X POST "$BASE/api/sops" \
  -H 'Content-Type: application/json' \
  -H "X-Sisyphus-Workspace-Id: $WS" \
  --data-binary @docs/examples/sop-templates/verify-sequence.json

curl -s "$BASE/api/sops"
curl -s -H "X-Sisyphus-Workspace-Id: $WS" "$BASE/api/sops/verify_sequence_template"
```

> 实际命令、节点类型与参数应按目标环境调整；执行前确认执行器权限与依赖。`Test` 节点 `workdir` 可为 `null` 或指定路径。

## 关联

- `docs/plans/single-node-orchestration-templates.md`
- `docs/runbooks/single-node-sop-e2e-guide.md`
- `docs/runbooks/single-node-typical-workflow.md`
