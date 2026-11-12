# L3 MCP resources/prompts 扩展验收（CLI 级）

## 1. 结论

在历史 MCP 场景（tools，CLI/引擎两级 17/17 PASS，证据 `l3-mcp-scenario.md`）基础上，
补齐 MCP **resources/prompts** 的客户端能力、CLI 子命令、独立 fixture、bridge 引擎级资源/提示目录快照与验收：

- `./scripts/l3-mcp-check.sh --cli-only`：**PASS=21 FAIL=0**
- `./scripts/l3-mcp-check.sh`（CLI + 引擎）：**PASS=36 FAIL=0**
- 覆盖：stdio + HTTP 两种传输；tools 调用 + resources 列表/读取 + prompts 列表/获取；
  含自家 `sisyphus mcp serve` 互通校验。

## 2. 范围与限制

| 项 | 状态 | 说明 |
|---|---|---|
| MCP 客户端 `resources/list`/`resources/read` | DONE | `crates/sisyphus-tools-mcp`（`McpClient` + 协议类型，`serde(default)` 兼容缺省 capability） |
| MCP 客户端 `prompts/list`/`prompts/get` | DONE | 同上 |
| CLI 子命令 | DONE | `sisyphus mcp resources` / `read-resource` / `prompts` / `get-prompt`；`mcp info` best-effort 展示 resources/prompts |
| 自家 MCP server | PARTIAL | `mcp serve` 声明 resources/prompts capability 并返回空列表（`read/get` 对不存在项报错）；不提供业务资源/提示 |
| 独立 fixture | DONE | `scripts/l3-mcp-server.py`（stdio）+ `scripts/l3-mcp-http-server.py`（HTTP，共用协议实现） |
| CLI 级验收 | DONE | `scripts/l3-mcp-check.sh --cli-only`，21/21 PASS |
| 引擎/bridge 级目录快照 | DONE | bridge 启动时采集 MCP resources/prompts，新增 `GET /api/mcp/resources`、`GET /api/mcp/prompts`（按 server prefix 聚合） |
| 引擎/bridge 级 read/get 代理 | DONE | 新增 `GET /api/mcp/resources/read`、`POST /api/mcp/prompts/get`，按 server prefix 代理到已连接 MCP 执行器；`scripts/l3-mcp-check.sh` level 2 覆盖 stdio+HTTP 两 server 的按需读/取（**36/36 PASS**） |
| OAuth / 分页 cursor / resource template 实读 | NOT DONE | 未覆盖 |

## 3. 复现与结果

```bash
cargo build -p sisyphus-cli
cargo build -p sisyphus-bridge
bash scripts/l3-mcp-check.sh --cli-only   # CLI 级
bash scripts/l3-mcp-check.sh              # CLI + 引擎级
```

原始输出：`runs/mcp-rp/l3-mcp-check-cli.out`（**PASS=21 FAIL=0**）、
`runs/mcp-rp/l3-mcp-check-full.out`（**PASS=36 FAIL=0**）。

关键检查项（节选）：

- stdio：`mcp resources` 列出 `memo://welcome` / `memo://status`，`mcp read-resource` 返回 `hello from mcp resource`，`mcp prompts` 列出 `greet`/`summary`，`mcp get-prompt` 返回渲染消息。
- HTTP：同等 resources/prompts 检查通过。
- 自家 server：`mcp info <cli> mcp serve` 可读取工具列表并含 `metis_analyse`；resources/prompts 返回空列表（非 “not supported”）。
- 引擎级：`/api/tools` 含 `l3:sum`/`l3http:sum` 且可执行；`/api/mcp/resources` 含 `l3`/`l3http` 与 `memo://welcome`；`/api/mcp/prompts` 含 `greet` 与 `l3http`；`/api/mcp/resources/read`（`l3`/`l3http`）与 `/api/mcp/prompts/get`（`l3`/`l3http`）按需代理返回预期内容。

## 4. 代码与提交

- `f296dbe` feat(mcp): 补齐 resources/prompts 客户端能力与 L3 验收
- `6a2a05c` feat(cli): TUI dev 支持环境变量注入客观闸门，mcp info 展示资源/提示
- `009aa90` test(mcp): 补充 resources/prompts 客户端方法单测
- `428a50c` fix(mcp): 自家 MCP server 支持空资源/提示列表并加固 L3 HTTP 检查
- `c0fbf60` feat(bridge): 暴露 MCP resources/prompts 目录快照并扩展引擎级验收（32/32）
- （本轮）bridge 增加 `GET /api/mcp/resources/read`、`POST /api/mcp/prompts/get` 按需代理，`l3-mcp-check.sh` 扩展至 **36/36 PASS**

## 5. 说明

- 本文档是对历史 `l3-mcp-scenario.md`（2026-11-12，tools 17/17）的**增量补充**，
  不替代原始证据；覆盖范围已扩展到 bridge 引擎级目录快照与按需 `resources/read`/`prompts/get` 代理（stdio+HTTP）。
- 结论基于本地独立 fixture 与 CLI/HTTP API 的外部可观察输出；目录快照在 bridge 启动时采集（运行期服务端变更不会自动刷新），但 read/get 代理为按需调用。
