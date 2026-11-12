# L3「MCP 场景」验收 — 证据

## 1. 结论

MCP 是计划中最后一个未覆盖的 L3 场景。本次用一个**独立实现**的 MCP 服务器
（`scripts/l3-mcp-server.py`，按 MCP 规范自行实现，不复用本仓代码）做端到端
验收，**12/12 PASS**。

过程中暴露并修复了 **4 个真实互操作缺陷**（此前 MCP 只有自家 mock 的
round-trip 测试，全部掩盖了它们）：

| # | 缺陷 | 影响 | 修复 |
|---|---|---|---|
| 1 | 线上字段用 snake_case | 连不上任何**合规** MCP 服务器（规范要求 camelCase） | `#[serde(rename_all = "camelCase")]` |
| 2 | 通知（notification）同步等响应 | `initialize` 握手的 `notifications/initialized` 卡满 120s 才报错 | `McpTransport::notify()` 只写不读 |
| 3 | `sisyphus mcp call` 参数顺序非法 | clap 断言失败 → **启动即 panic** | `connect` 去掉 `required`（clap 要求必需位置参数排在可选之前） |
| 4 | 自家 MCP server 回复 notification | 响应滞留在管道 → 流错位，客户端读到的下一个响应没有 `result` | server 对无 id 的通知不回复 |

## 2. 场景与方法

| 项 | 值 |
|---|---|
| 服务器 | `scripts/l3-mcp-server.py`（stdio，newline-delimited JSON） |
| 工具 | `sum(a,b)` / `word_count(text)` / `echo(text)` |
| CLI | `sisyphus mcp info` / `sisyphus mcp call` |
| 自家 server | `sisyphus mcp serve`（客户端 ↔ 自家 server，同一套协议两端） |
| 引擎 | bridge + `GET /api/tools`、`POST /api/tools/execute`（tool_id `l3:sum`） |
| 复现 | `bash scripts/l3-mcp-check.sh`（`--cli-only` 只跑 CLI 级） |

脚本 `scripts/l3-mcp-check.sh` 逐项判定并打印 PASS/FAIL，退出码非零即失败。

## 3. 结果

原始输出：`runs/mcp/l3-mcp-check.out`（`PASS=12 FAIL=0`），bridge 侧连接日志
`runs/mcp/bridge.log`（`Connected MCP server 'l3' via stdio: ... (3 tools)`）。

CLI 级（8 项）：

```
PASS  mcp info 握手并列出 3 个工具
PASS  mcp info 工具名 sum
PASS  mcp info 工具名 word_count
PASS  mcp call sum(2,3) = 5
PASS  mcp call word_count = 3
PASS  mcp call 参数缺失返回 isError
PASS  自家 MCP server 可被协议客户端读取工具列表
PASS  自家 MCP server 工具名 metis_analyse
```

引擎级（4 项）：

```
PASS  引擎 /api/tools 含 MCP 工具 l3:sum
PASS  引擎 /api/tools 标注来源为 mcp
PASS  引擎调用 MCP 工具 l3:sum(40,2)
PASS  引擎调用返回 SUCCESS
```

## 4. 缺陷细节

### 4.1 线上字段是 snake_case（致命）

MCP 规范规定线上字段为 camelCase（`protocolVersion` / `serverInfo` /
`inputSchema` / `listChanged` / `isError`）。本仓的 MCP 类型全部用 snake_case，
于是 `initialize` 的响应解析直接失败：

```
Error: MCP info 失败 ... Caused by: 1: Todo error: deserialize result: missing field `protocol_version`
```

既有单测之所以没抓到：mock 响应是用本仓自己的类型序列化出来的（snake_case），
再做 round-trip —— 自洽但不合规。

修复：给 12 个 MCP 线上类型加 `#[serde(rename_all = "camelCase")]`（Rust 字段名
保持 snake_case，不影响其它调用方），并新增两个回归测试：
`test_wire_format_is_camel_case`、`test_deserializes_camel_case_from_spec_server`。
既有 mock 响应也改成 camelCase，避免继续用「自家格式」当规范。

### 4.2 通知同步等响应（握手必然超时）

`send_notification` 走的是 `transport.request()` —— 读一行响应。而 MCP 通知
是 fire-and-forget，合规服务器**不会**回。于是 `initialize` 卡在通知上，直到
`REQUEST_TIMEOUT`（120s）才报错。

修复：给 `McpTransport` 增加 `notify()`（默认实现仍走 `request`，对「什么都回一行」
的传输安全），stdio / http 覆写为只写不读；`send_notification` 改调 `notify`。
回归测试 `test_initialize_sends_notification_without_consuming_response` 断言
通知已发出且**不消费**响应队列。

### 4.3 `mcp call` 启动即 panic

`Call { tool, args: Option<String>, connect: Vec<String> }` 里 `connect` 标了
`required = true`，而 clap 要求必需位置参数排在可选位置参数**之前**
（`tool` < `args` < `connect` 违反）。debug 构建在**参数解析阶段**直接 panic：

```
thread 'main' panicked at clap_builder-4.6.0/src/builder/debug_asserts.rs:657:
Found non-required positional argument with a lower index than a required positional argument: "args"
```

即 `sisyphus mcp call` 在 debug 构建下**完全不可用**（release 构建关闭
debug_assert，行为未定义）。

修复：`connect` 不再标 `required`（缺失时由 `resolve_client` 给出明确报错
「Provide either `<url>` for HTTP or `<command>` `[args...]` for stdio」）。

### 4.4 自家 MCP server 回复 notification（修完客户端才暴露）

`run_serve` 对**每条**输入行都写一行响应，包括通知（无 `id`）。而协议规定
通知不得有响应。那条多出来的响应会滞留在管道里，被客户端的下一个请求读走 ——
流就此错位：

```
Connected to MCP server: sisyphus-cli v0.1.0
Error: ... Caused by: 1: Todo error: list_tools: missing result
```

这个缺陷此前被缺陷 2 掩盖：旧客户端会**同步等**通知的响应，于是刚好把这条
多余响应消费掉。修好客户端（不再等通知）后，它才显形 —— 这也说明两个缺陷
必须一起修，单修一个反而更糟。

修复：`run_serve` 对无 `id` 的通知不再写响应。验收里加了两项「客户端 ↔ 自家
server」检查守住这条。

## 5. 诚实性说明

- 本场景验证的是 **MCP 协议互通性**（客户端 ↔ 独立实现的服务器），不涉及 LLM。
- 覆盖的是 stdio 传输；HTTP 传输（`mcp info http://...`）未在本机验证（需外部
  MCP 服务器），但 `notify()` 的 HTTP 实现与 stdio 同为「只写不读」。
- 未覆盖：MCP 服务器的 OAuth 流程（`crates/sisyphus-tools-mcp/src/mcp/oauth.rs`
  未做端到端验证）、resources / prompts 等非 tools 能力。
- 引擎级只验证了「注册 + 调用一次 MCP 工具」，未验证多服务器并存、
  工具名前缀路由与权限审批在 MCP 场景下的行为。
- 自家 server 只验证了「工具列表」；调用其工具需要真实 LLM（metis/momus/
  librarian），本机 ollama 可跑但不属于 MCP 协议覆盖范围，未纳入本场景。
