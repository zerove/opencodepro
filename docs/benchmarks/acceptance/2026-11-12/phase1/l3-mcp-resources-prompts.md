# L3 MCP resources/prompts 扩展验收（CLI 级）

## 1. 结论

在历史 MCP 场景（tools，CLI/引擎两级 17/17 PASS，证据 `l3-mcp-scenario.md`）基础上，
补齐 MCP **resources/prompts** 的客户端能力、CLI 子命令、独立 fixture、bridge 引擎级资源/提示目录快照与验收：

- `./scripts/l3-mcp-check.sh --cli-only`：**PASS=41 FAIL=0**
- `./scripts/l3-mcp-check.sh`（CLI + 引擎）：**PASS=65 FAIL=0**
- 覆盖：stdio + HTTP 两种传输；tools 调用 + resources 列表/读取 + prompts 列表/获取；
  含自家 `sisyphus mcp serve` 互通校验；**错误语义**（未知工具/资源/提示、缺少参数、
  未知 server）在 CLI/HTTP/引擎代理三层均有断言。

## 2. 范围与限制

| 项 | 状态 | 说明 |
|---|---|---|
| MCP 客户端 `resources/list`/`resources/read` | DONE | `crates/sisyphus-tools-mcp`（`McpClient` + 协议类型，`serde(default)` 兼容缺省 capability） |
| MCP 客户端 `prompts/list`/`prompts/get` | DONE | 同上 |
| CLI 子命令 | DONE | `sisyphus mcp resources` / `read-resource` / `prompts` / `get-prompt`；`mcp info` best-effort 展示 resources/prompts |
| 自家 MCP server | PARTIAL | `mcp serve` 声明 resources/prompts capability 并返回空列表（`read/get` 对不存在项报错）；不提供业务资源/提示 |
| 独立 fixture | DONE | `scripts/l3-mcp-server.py`（stdio）+ `scripts/l3-mcp-http-server.py`（HTTP，共用协议实现） |
| CLI 级验收 | DONE | `scripts/l3-mcp-check.sh --cli-only`，41/41 PASS（含分页自动合并/读取/获取、资源模板实例读取与错误语义） |
| 引擎/bridge 级目录快照 | DONE | bridge 启动时采集 MCP resources/prompts，新增 `GET /api/mcp/resources`、`GET /api/mcp/prompts`（按 server prefix 聚合） |
| 引擎/bridge 级 read/get 代理、分页与模板实读 | DONE | 新增 `GET /api/mcp/resources/read`、`POST /api/mcp/prompts/get`；fixture 提供 `nextCursor` 分页与 `resourceTemplates`，客户端/bridge/CLI 自动合并分页并暴露模板，level 2 覆盖分页/模板目录、按模板实例 URI 的按需读取与错误语义（**65/65 PASS**） |
| OAuth | NOT DONE | 未覆盖（缺少可控 OAuth 授权服务器/fixture 与凭据） |

## 3. 复现与结果

```bash
cargo build -p sisyphus-cli
cargo build -p sisyphus-bridge
bash scripts/l3-mcp-check.sh --cli-only   # CLI 级
bash scripts/l3-mcp-check.sh              # CLI + 引擎级
```

原始输出：`runs/mcp-rp/l3-mcp-check-full.out`（**PASS=65 FAIL=0**）、
`runs/mcp-rp/l3-mcp-check-cli.out`（`--cli-only`，**PASS=41 FAIL=0**）。

关键检查项（节选）：

- stdio：`mcp resources` 列出 `memo://welcome` / `memo://status`，`mcp read-resource` 返回 `hello from mcp resource`，`mcp prompts` 列出 `greet`/`summary`，`mcp get-prompt` 返回渲染消息。
- HTTP：同等 resources/prompts 检查通过。
- 错误语义：`mcp call no_such_tool` / `sum {}` / `echo {}` 返回 `isError: true`；
  `read-resource memo://missing` 报 `resource not found: memo://missing`；
  `get-prompt missing` 报 `prompt not found: missing`；stdio 与 HTTP 两种传输均断言，
  引擎代理另断言未知 URI（502 包裹原始错误）与未知 server（404 `MCP server not found`）。
- 自家 server：`mcp info <cli> mcp serve` 可读取工具列表并含 `metis_analyse`；resources/prompts 返回空列表（非 “not supported”）。
- 引擎级：`/api/tools` 含 `l3:sum`/`l3http:sum` 且可执行；`/api/mcp/resources` 含 `l3`/`l3http` 与 `memo://welcome`；`/api/mcp/prompts` 含 `greet` 与 `l3http`；`/api/mcp/resources/read`（`l3`/`l3http`）与 `/api/mcp/prompts/get`（`l3`/`l3http`）按需代理返回预期内容。

## 4. 代码与提交

- `f296dbe` feat(mcp): 补齐 resources/prompts 客户端能力与 L3 验收
- `6a2a05c` feat(cli): TUI dev 支持环境变量注入客观闸门，mcp info 展示资源/提示
- `009aa90` test(mcp): 补充 resources/prompts 客户端方法单测
- `428a50c` fix(mcp): 自家 MCP server 支持空资源/提示列表并加固 L3 HTTP 检查
- `c0fbf60` feat(bridge): 暴露 MCP resources/prompts 目录快照并扩展引擎级验收（32/32）
- `0319c91` bridge 增加 `GET /api/mcp/resources/read`、`POST /api/mcp/prompts/get` 按需代理，`l3-mcp-check.sh` 扩展至 **36/36 PASS**
- `21ced70` fixture/check 扩展 `nextCursor` 分页场景，外部验收 **48/48 PASS**
- （上一轮）fixture/check 扩展 `resourceTemplates` 与模板实例 URI 读取（`memo://note/{name}` -> `memo://note/*`），外部验收 **54/54 PASS**
- （本轮）`l3-mcp-check.sh` 增加 11 项**错误语义**检查（未知工具/URI/提示、缺少参数、
  未知 server；stdio+HTTP+引擎代理），外部验收 **65/65 PASS**；同时把后台进程
  `trap` 提前到脚本早期，避免 fixture 在失败路径上变成孤儿进程导致下次运行
  「端口已被占用」——本次复跑正是被上一轮残留 fixture 阻塞后修复的。

## 5. 说明

- 本文档是对历史 `l3-mcp-scenario.md`（2026-11-12，tools 17/17）的**增量补充**，
  不替代原始证据；覆盖范围已扩展到 bridge 引擎级目录快照与按需 `resources/read`/`prompts/get` 代理（stdio+HTTP）。
- 结论基于本地独立 fixture 与 CLI/HTTP API 的外部可观察输出；`/api/mcp/resources`、`/api/mcp/prompts` 列表按需实时拉取并在失败时回退启动快照，`resources/read`、`prompts/get` 为按需代理调用。fixture 提供带 `nextCursor` 的分页响应与 `resourceTemplates`（`memo://note/{name}`），验收覆盖 CLI/HTTP/引擎目录自动合并、模板暴露、分页资源读取、模板实例 URI 读取、分页提示获取与错误语义（**65/65**）；客户端分页另有 `cargo test -p sisyphus-tools-mcp --lib` 单测佐证。
- 验收脚本的后台进程（HTTP fixture、bridge）统一由脚本早期安装的 `trap` 清理；
  此前的写法在 fixture 启动后、`trap` 安装前失败会泄漏进程，导致下一次运行
  因端口占用失败（本次已实际遇到并修复）。
