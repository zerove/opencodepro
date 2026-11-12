# 单机工具与 MCP 使用指南（P1-B）

> 目标：提升工具与 MCP 能力的可发现性与可用性（单机优先），明确前置条件与常见路径。

## 1. 发现与查看

### 1.1 CLI 工具与技能

```bash
# 技能
sisyphus skill list
sisyphus skill show <name>
sisyphus skill install <path-to-skill.md>
sisyphus skill remove <name>

# 工具注册/列表/自举（按本地实现与配置）
sisyphus tool list
sisyphus tool register-mcp ...
sisyphus tool bootstrap
sisyphus tool --help
```

### 1.2 MCP

```bash
# 连接并查看 MCP 服务器信息与工具
sisyphus mcp info <...>

# 调用 MCP 工具
sisyphus mcp call <...>

# 以 stdio 方式暴露本地代理工具（作为 MCP server）
sisyphus mcp serve
```

> 具体参数与子命令以 `sisyphus mcp <subcommand> --help`、`sisyphus tool <subcommand> --help` 为准；不同子命令可能需要服务器地址、配置文件或环境变量。

可直接跑通的例子（仓库内自带一个按 MCP 规范实现的测试服务器）：

```bash
# 查看服务器信息与工具
sisyphus mcp info python3 scripts/l3-mcp-server.py

# 调用工具（位置参数：工具名 → JSON 参数 → 连接方式）
sisyphus mcp call sum '{"a":2,"b":3}' python3 scripts/l3-mcp-server.py
# → { "content": [ { "type": "text", "text": "5" } ], "isError": false }
```

HTTP（Streamable HTTP）传输同理：

```bash
python3 scripts/l3-mcp-http-server.py --port 11520 &   # 本地测试服务器
sisyphus mcp info http://127.0.0.1:11520/mcp
sisyphus mcp call sum '{"a":20,"b":22}' http://127.0.0.1:11520/mcp
```

一条命令跑完整验收（CLI 级 + bridge 引擎级）：

```bash
bash scripts/l3-mcp-check.sh
```

说明与证据：`docs/benchmarks/acceptance/2026-11-12/phase1/l3-mcp-scenario.md`。

### 1.3 Bridge 侧技能/工具（需 bridge）

```bash
curl -s http://127.0.0.1:8080/api/skills
curl -s -X POST http://127.0.0.1:8080/api/skills/install \
  -H 'Content-Type: application/json' -d '{"skill_id":"..."}' -i
# 工具执行（按契约）
# curl -s -X POST http://127.0.0.1:8080/api/execute -H 'Content-Type: application/json' \
#   -d '{"tool_id":"...","args_json":"{}","fuel_limit":0}'
```

## 2. 分层与前置条件

| 类别 | 示例 | 前置条件 |
|---|---|---|
| 本地内置/文件类工具 | `read/write/edit/glob/grep/exec` | 本地文件系统与权限 |
| 需 bridge 的能力 | SOP/技能查询、工具执行 API、会话服务端操作 | bridge 运行且地址正确 |
| 需 LLM/外部服务 | LLM 驱动推理、联网/远程 MCP、部分 agent 工具 | 凭据、端点、网络、MCP 服务器可达 |
| 沙箱相关 | WASM 工具执行 | sandbox/sandboxd 配置与环境权限 |

详见 `single-node-tools-and-dependencies.md`。

## 3. MCP 使用建议（单机）

- **先发现后调用**：使用 `mcp info` 确认可用工具与参数，再执行 `mcp call`。
- **配置管理**：MCP 服务器连接信息与鉴权按项目/环境配置；敏感信息避免硬编码。
- **错误定位**：区分“连接/鉴权失败”“工具不存在/参数错误”“工具执行失败（依赖/权限）”。

## 4. 可发现性改进方向（持续）

- 在 CLI `--help` 与文档中保持参数/示例同步。
- 对常见失败（未配置 MCP、bridge 不可达、权限不足）提供明确提示（与 P1-D 对齐）。
- 场景化示例纳入 `single-node-typical-workflow.md` 与 SDK/REST 文档。

## 5. 关联文档

- `single-node-tools-and-dependencies.md`
- `single-node-typical-workflow.md`
- `../plans/p1-p2-roadmap.md`
