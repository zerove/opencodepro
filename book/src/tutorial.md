# 端到端教程

本文档提供三个完整的端到端示例，覆盖 Sisyphus 的三种典型使用场景：

1. **CLI 场景**：纯命令行执行 SOP
2. **Bridge 场景**：通过 REST API 程序化调用
3. **多 Agent 协作场景**：使用 DelegationPolicy 与 fallback agents

每个示例都可以独立运行，复制即可在本地复现。

---

## 示例 1：CLI 场景 — 创建一个 Markdown 文件

**目标**：通过 TUI 输入自然语言目标，让引擎生成 SOP、执行并落盘文件。

### 步骤

```bash
# 1. 启动 bridge（在终端 A）
cargo run -p sisyphus-bridge

# 2. 启动 TUI（在终端 B）
cargo run -p sisyphus-cli

# 3. 在 TUI 中按 Ctrl+N 创建新会话
# 4. 输入目标（自然语言）：
#    "Create a Markdown file named hello-sisyphus.md with content: # Hello, Sisyphus!"

# 5. 观察认知循环的执行：
#    - 引擎分解目标为子步骤（DAG）
#    - 每个步骤通过 WASM 工具执行
#    - 反思阶段检查偏差，必要时迭代
#    - 目标达成时收敛
```

### 预期输出

完成后，工作目录会出现 `hello-sisyphus.md` 文件，内容为：

```markdown
# Hello, Sisyphus!
```

TUI 面板会显示：
- 执行步骤数与最终结果
- 偏差评分与置信度
- token 消耗（如果启用了 token_budget）

---

## 示例 2：Bridge 场景 — REST API 提交目标

**目标**：完全通过 HTTP API 与引擎交互（适合 CI/CD、批处理场景）。

### 步骤

```bash
# 1. 启动 bridge
cargo run -p sisyphus-bridge &

# 2. 验证健康度
curl http://localhost:8080/health/live
# → {"status":"ok","kind":"liveness","version":"..."}

curl http://localhost:8080/health/ready
# → {"status":"ok","kind":"readiness","components":{...}}

# 3. 提交目标（异步执行）
curl -X POST http://localhost:8080/api/execute/goal \
  -H "Content-Type: application/json" \
  -d '{
    "goal": "List all .rs files in the current directory",
    "sop_id": "list-rs-files",
    "project_id": "tutorial-demo"
  }'
# → 返回 goal_id

# 4. 查询目标状态
GOAL_ID="..."
curl http://localhost:8080/api/execute/goal/${GOAL_ID}/status

# 5. 获取决策 trace（用于调试或可视化）
curl http://localhost:8080/api/cognitive/trace?limit=20

# 6. 通过 /api/metrics 抓 Prometheus 指标（可对接 Grafana）
curl http://localhost:8080/api/metrics
# → text/plain; version=0.0.4 格式的 Prometheus 指标
```

### K8s 集成

把上述健康度端点接入 K8s Deployment：

```yaml
livenessProbe:
  httpGet:
    path: /health/live
    port: 8080
  initialDelaySeconds: 5
  periodSeconds: 10

readinessProbe:
  httpGet:
    path: /health/ready
    port: 8080
  initialDelaySeconds: 3
  periodSeconds: 5
```

`/api/metrics` 可由 Prometheus 通过 `kubernetes_sd_configs` 自动发现并 scrape。

---

## 示例 3：多 Agent 协作场景 — Delegation + Fallback

**目标**：配置多 Agent 协作，让一个 Agent 失败时回退到另一个。

### 配置文件

```toml
# config.example.toml 的相关段
[agents]
# Agent A: 首选，处理反射式任务
[agents.reflector]
role = "reflector"
model = "gpt-4o"
system_prompt = "You are a code reviewer. Provide concise feedback."

# Agent B: 回退，OpenAI 不可用时使用
[agents.reflector_fallback]
role = "reflector"
model = "claude-3-5-sonnet"
system_prompt = "You are a code reviewer. Provide concise feedback."
```

### 通过任务 args 触发委派

在 SOP 任务的 args 中加入 `_delegation` 字段：

```json
{
  "tool_id": "llm_reflect",
  "args": {
    "code": "fn add(a, b) -> i32 { a + b }",
    "_delegation": {
      "primary": "reflector",
      "fallback_agents": ["reflector_fallback"],
      "retries": 2,
      "timeout_ms": 30000
    }
  }
}
```

行为：
1. 引擎先调用 `reflector`，失败时按 `fallback_agents` 顺序回退。
2. 每次回退都附加 `_delegated_agent` 字段到结果，便于追溯。

### 验证

```bash
# 启动时启用 tracing (RUST_LOG=info) 观察委派链
RUST_LOG=info cargo run -p sisyphus-bridge

# 在 TUI 中执行多 Agent 任务，TUI 会显示：
# - 实际调用的 Agent 名（_delegated_agent）
# - 委派耗时与重试次数
```

---

## 下一步

- [配置](./configuration.md) — 调整 token_budget / delegation_policy / chaos 配置
- [REST API](./rest-api.md) — 完整端点列表与请求/响应 schema
- [性能](./performance.md) — bench 基准 + 调优建议
- [示例工程](../examples/) — 可编译运行的 Rust 示例

> **提示**：所有示例均使用项目根目录的 `config.example.toml` 作为基础配置。
> 复制后按需调整 LLM endpoint / 模型 / workspace 路径。