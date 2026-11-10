# 单机 SDK 最小示例（S3）

> 目标：提供 TypeScript / Python / Go 对接本地 bridge 的最小可运行示例（单机）。  
> 前提：bridge 已启动（见 `docs/runbooks/single-node-operation-and-verification.md`），默认 HTTP `http://127.0.0.1:8080`。  
> 说明：以下示例仅演示契约正确的调用方式；真实 SOP/LLM 执行效果依赖引擎与 LLM 配置。

## TypeScript (`@sisyphus/sdk`)

```ts
import { SisyphusClient } from "@sisyphus/sdk";

const client = new SisyphusClient({
  bridgeUrl: "http://127.0.0.1:8080",
  // workspaceId: "default", // 需要时附加 X-Sisyphus-Workspace-Id
});

async function main() {
  const sops = await client.listSops();
  console.log("sops:", sops);

  // 目标执行（SOP）：executeSop(sopId, goal, projectId?, eventTx?)
  // const res = await client.executeSop("my-sop", "完成示例目标");
  // console.log(res.output, res.report);

  const cluster = await client.clusterStatus();
  console.log("cluster:", cluster.enabled, cluster.control_plane?.security?.effective_armed);
}

main().catch(console.error);
```

> 实际方法名以 `ide/typescript-sdk` 当前导出为准（本批次已对齐 bridge 契约：`executeSop/executeTool/listSops/getSop/listSkills/installSkill/clusterStatus/controlLease` 等）。

## Python (`sisyphus-sdk`)

```python
from sisyphus_sdk import SisyphusClient

client = SisyphusClient(
    bridge_url="http://127.0.0.1:8080",
    # workspace_id="default",
)

sops = client.list_sops()
print("sops:", sops)

# 目标执行（SOP）：execute_sop(sop_id, goal, project_id=None, event_tx=None)
# res = client.execute_sop("my-sop", "完成示例目标")
# print(res.output, res.report)

cluster = client.cluster_status()
print("cluster enabled:", cluster.enabled)
```

## Go (`github.com/sisyphus/sdk-go`)

```go
package main

import (
	"fmt"
	"log"

	sisyphus "github.com/sisyphus/sdk-go"
)

func main() {
	c := sisyphus.NewClient("http://127.0.0.1:8080")
	// c.WorkspaceID = "default"

	sops, err := c.ListSOPs()
	if err != nil {
		log.Fatal(err)
	}
	fmt.Println("sops:", sops)

	// 目标执行：ExecuteSOP(sopID, goal, projectID)
	// res, err := c.ExecuteSOP("my-sop", "完成示例目标", "")
	// if err != nil { log.Fatal(err) }
	// fmt.Println(res.Output, res.Report)

	status, err := c.GetClusterStatus()
	if err != nil {
		log.Fatal(err)
	}
	fmt.Println("cluster enabled:", status.Enabled)
}
```

## 验证建议

- 先用 `curl http://127.0.0.1:8080/health` 确认 bridge 可达。
- 只读观测（`/api/cluster/status`、`/api/cluster/control/leases/{key}`）在单机默认配置下应可返回稳定快照。
- 执行类调用依赖 SOP/LLM/工具配置；无配置时应获得可解释错误而非静默成功。
