# 单机工具与依赖分层说明（S3）

> 目标：区分单机环境下“纯本地可用”与“需外部依赖”的能力，明确前置条件与验收边界。

## 1. 能力分层

| 层级 | 能力示例 | 前置条件 | 单机验收方式 |
|---|---|---|---|
| **纯本地（无外部服务）** | CLI 文件操作 `read/write/edit/glob/grep`、本地 `exec`、部分 git/构建辅助命令、本地配置解析、WebUI 静态页面与 HTMX 面板渲染（bridge 不可达时降级展示） | 可执行二进制与文件系统权限 | 直接命令/页面访问；断言输出与退出码 |
| **本地进程依赖** | bridge gRPC/REST、WebUI 对接本地 bridge、CLI 经 bridge 的会话/SOP/技能查询与只读观测 | bridge/WebUI 启动、端口可用、配置正确（顶层 `bridge_address`/`http_address`） | `curl` 健康检查 + 端点/页面冒烟 + 契约测试 |
| **可选外部依赖（LLM/网络）** | `POST /api/execute/goal` 的真实推理执行、LLM 驱动 agent/MCP 工具调用、联网搜索/远程技能源等 | `SISYPHUS_LLM_*` 凭据与端点、网络可达、对应 provider 可用 | 在具备凭据的环境做端到端执行；无凭据时验证“可解释错误/门禁” |
| **沙箱/隔离依赖** | WASM 工具执行（sandbox/sandboxd） | sandboxd 可执行与权限、wasmtime/编译环境（按配置） | 在具备权限的环境运行 sandboxd 相关测试/用例；受限环境按“环境限制”记录 |
| **分布式/专用环境（不纳入单机完成）** | 控制面真实网络多节点共识、专用 runner 证据矩阵 | N=3 真实网络节点、专用 runner、证书/网络条件 | 仅在专用环境执行；单机保持 `cluster.control_plane.enabled=false` |

## 2. 单机推荐验收顺序

1. **本地工具**：CLI 文件/exec（无 bridge）→ 确认可执行与基础行为。
2. **本地服务**：启动 bridge → `/health`、`/openapi.json`、SOP/技能/会话/集群只读端点。
3. **接入面**：WebUI 对接本地 bridge；SDK 最小只读调用。
4. **可选增强**：配置 LLM 后验证 `execute/goal` 与工具链；按需验证 MCP/沙箱。

## 3. 常见环境限制与处理

- **端口占用**：调整顶层 `http_address`/`bridge_address`（或对应 env），避免与已有服务冲突。
- **sandboxd 权限/不可用**：`sandboxd_client` 相关测试可能在受限环境失败；不阻塞非沙箱单机主流程，需单独记录。
- **无 LLM 凭据**：执行类能力应返回明确错误/未配置提示；不应静默成功或伪造结果。
- **嵌入降级**：`SISYPHUS_LLM_EMBEDDING_ALLOW_DUMMY=true` 仅用于本地验证流程，语义质量不代表生产。

## 4. 与后续阶段关系

- S4 验收表应按本分层标注“通过 / 受环境限制 / 未配置（非缺陷）”，避免将外部依赖缺失误判为功能缺失。

## 5. 错误处理与用户反馈（P1-D）

- **连通性**：CLI bridge 连接失败时提供可操作提示（启动 bridge、检查 `bridge_address`/`--bridge`）；WebUI 在 bridge 不可达时降级展示默认只读快照。
- **契约错误**：HTTP 4xx/5xx 与 gRPC 错误应携带可解释信息；SDK/WebUI 需正确映射并展示（避免静默吞错）。
- **依赖缺失**：LLM/MCP/工具权限等前置条件不足时，应明确指出缺失项与配置路径，而非返回模糊失败。
- **一致性**：同类失败场景在 CLI/WebUI/SDK 的提示语义应保持一致（“不可达/未配置/权限不足/超时”可区分）。
