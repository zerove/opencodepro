# L3 MCP resources/prompts 扩展验收（CLI 级）

## 1. 结论

在历史 MCP 场景（tools，CLI/引擎两级 17/17 PASS，证据 `l3-mcp-scenario.md`）基础上，
补齐 MCP **resources/prompts** 的客户端能力、CLI 子命令、独立 fixture 与 CLI 级验收：

- `./scripts/l3-mcp-check.sh --cli-only`：**PASS=21 FAIL=0**
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
| 引擎/bridge 级 resources/prompts | NOT DONE | bridge `/api/tools` 与 MCP 注册当前仅面向 tools；未暴露 resources/prompts API/验收（需先对齐设计） |
| OAuth / 分页 cursor / resource template 实读 | NOT DONE | 未覆盖 |

## 3. 复现与结果

```bash
cargo build -p sisyphus-cli
bash scripts/l3-mcp-check.sh --cli-only
```

原始输出：`runs/mcp-rp/l3-mcp-check-cli.out`（**PASS=21 FAIL=0**）。

关键检查项（节选）：

- stdio：`mcp resources` 列出 `memo://welcome` / `memo://status`，`mcp read-resource` 返回 `hello from mcp resource`，`mcp prompts` 列出 `greet`/`summary`，`mcp get-prompt` 返回渲染消息。
- HTTP：同等 resources/prompts 检查通过。
- 自家 server：`mcp info <cli> mcp serve` 可读取工具列表并含 `metis_analyse`；resources/prompts 返回空列表（非 “not supported”）。

## 4. 代码与提交

- `f296dbe` feat(mcp): 补齐 resources/prompts 客户端能力与 L3 验收
- `6a2a05c` feat(cli): TUI dev 支持环境变量注入客观闸门，mcp info 展示资源/提示
- `009aa90` test(mcp): 补充 resources/prompts 客户端方法单测
- `428a50c` fix(mcp): 自家 MCP server 支持空资源/提示列表并加固 L3 HTTP 检查

## 5. 说明

- 本文档是对历史 `l3-mcp-scenario.md`（2026-11-12，tools 17/17）的**增量补充**，
  不替代原始证据；两者覆盖范围不同（本次无引擎级 resources/prompts）。
- 结论基于本地独立 fixture 与 CLI 调用的外部可观察输出；不代表 bridge 引擎已支持
  resources/prompts，也不代表对任意第三方 MCP 服务器的完全兼容性。
