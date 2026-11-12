# T3.2 Provider 交互式引导 —— 端到端验收证据

> 采集日期：2026-11-12　脚本：`scripts/t3-2-provider-onboarding.sh`
> Mock 服务：`scripts/t3-2-mock-llm.py`
> **真实 provider**：本机 ollama `qwen2.5-coder:3b`（OpenAI 兼容端点），
> 用于验证「真实 HTTP 调用 + 真实 completion」，不依赖云端凭据。

## 复现

```bash
env -u RUSTUP_TOOLCHAIN cargo build -p sisyphus-cli
bash scripts/t3-2-provider-onboarding.sh
```

预期：`PASS=12 FAIL=0 SKIP=0`（真实 provider 不可达时 A 段记 SKIP，不算通过）。

## 覆盖范围

| 段 | 场景 | 断言 |
|---|---|---|
| A | 成功路径 | 真实调用通过后才写配置；`[[llm.providers]]` 落盘；含明文密钥时 0600；`list` 可见；`test` 返回 completion |
| B1 | 401 密钥无效 | 非 0 退出 + 状态码 + 可执行 hint + **不写配置** |
| B2 | 404 端点/模型不存在 | 同上 |
| B3 | 协议选错（端点返回 HTML） | 失败 + hint 指向 kind + 不写配置 |
| B4 | 端点不可达 | 失败 + hint + 不写配置 |
| C | remove | 移除条目；移除不存在项报错可读 |

## 结果

# T3.2 provider onboarding acceptance

run_at: 2026-11-12T07:59:57+08:00
cli: ./target/debug/sisyphus
endpoint: http://127.0.0.1:11434/v1/chat/completions
model: qwen2.5-coder:3b

| 结果 | 检查项 |
|---|---|
| PASS | A0 真实 provider 可达 |
| PASS | A1 provider login 通过真实连通性测试并写配置 |
| PASS | A2 配置写入 [[llm.providers]] 且名称为 e2e-local |
| PASS | A3 含明文密钥的配置权限为 0600 |
| PASS | A4 provider list 显示已注册 provider |
| PASS | A5 provider test 真实返回 completion |
| PASS | B1 401 密钥无效 失败 + 状态码 + 可执行 hint + 未写配置 |
| PASS | B2 404 端点/模型不存在 失败 + 状态码 + 可执行 hint + 未写配置 |
| PASS | B3 协议选错（HTML）失败 + hint 指向 kind + 未写配置 |
| PASS | B4 端点不可达失败 + hint + 未写配置 |
| PASS | C1 provider remove 移除条目 |
| PASS | C2 remove 不存在的 provider 报错可读 |

PASS=12 FAIL=0 SKIP=0

## 关键输出

### A1 provider login（真实 ollama 调用，29.7s → 通过后才写配置）

```
连通性测试中…（prompt: Say hello in one sentence）
  ✅ PASS  latency=1106ms
  response: Hello! How can I assist you today?
Backup: /tmp/opencode/t3-2-whOQ6s/config.toml.bak
Permissions: 0600（配置内含明文密钥）
Registered provider 'e2e-local' -> /tmp/opencode/t3-2-whOQ6s/config.toml
Next: `sisyphus provider test e2e-local`
```

### A5 provider test（真实 completion）

```
Testing provider 'e2e-local' (kind: openai, model: qwen2.5-coder:3b)
  endpoint: http://127.0.0.1:11434/v1/chat/completions
  prompt: Say hello in one sentence
  ✅ PASS  model=qwen2.5-coder:3b latency=912ms
  response: Hello! How can I assist you today?
```

### B1 401 失败路径

```
连通性测试中…（prompt: Say hello in one sentence）
  ❌ FAIL  Auth: mock error 401
  hint: 密钥无效或已过期（401）：用 `provider login` 重新录入，或用 `--api-key env:VAR` 引用环境变量
Error: 连通性测试失败，未写入配置。修好后重试，或确认可用时加 `--skip-test` 强制写入
```

### B2 404 失败路径

```
连通性测试中…（prompt: Say hello in one sentence）
  ❌ FAIL  Permanent: API error (404): mock error 404
  hint: 端点或模型不存在（404）：OpenAI 兼容端点通常以 `/v1/chat/completions` 结尾；模型名需与 provider 侧完全一致
Error: 连通性测试失败，未写入配置。修好后重试，或确认可用时加 `--skip-test` 强制写入
```

### B3 协议选错（HTML）失败路径

```
连通性测试中…（prompt: Say hello in one sentence）
  ❌ FAIL  Transient: Failed to parse response: error decoding response body
  hint: 响应不是 OpenAI 兼容 JSON：多半是 kind 选错（google/anthropic/cohere 用各自协议），或端点指向了网页而非 API
Error: 连通性测试失败，未写入配置。修好后重试，或确认可用时加 `--skip-test` 强制写入
```

### B4 端点不可达失败路径

```
连通性测试中…（prompt: Say hello in one sentence）
  ❌ FAIL  Transient: HTTP request failed: error sending request for url (http://127.0.0.1:1/v1/chat/completions)
  hint: 无法建立连接：确认服务已启动、地址/端口正确、网络可达（可先 curl 该端点）
Error: 连通性测试失败，未写入配置。修好后重试，或确认可用时加 `--skip-test` 强制写入
```


## 顺带修复的真实缺陷

1. **`request_timeout_secs` 被 client 级 5s 硬编码钳制**（`crates/sisyphus-tools-llm/src/llm/dynamic.rs`）：
   旧实现把 `.timeout(5s)` 钉在 reqwest client 上，外层再套 `tokio::time::timeout(config)`，
   两者取小 → 配置调到 60s 也仍然 5s 失败。本地模型生成 100 token 约 9s，**必然超时且无法自救**。
   现改为按配置在每次请求下发 `.timeout()`；回归测试 `request_timeout_secs_is_not_clamped`
   （6s 慢响应，负向对照：还原旧实现后 5.02s 失败）。
2. **超时与连接失败被混报**为 `HTTP request failed`：现在区分
   `LLM connect failed ... (endpoint unreachable or port not listening)` 与
   `LLM request timed out after Ns ...`，`provider test` 的 hint 才能指向正确的修复方向。
3. **`provider test` 原是桩实现**（打印 "Test call would go here" 不发请求）；现在真实调用并对
   401/403/404/429/解析失败/连接失败分类给出可执行建议。
4. **连通性测试改为单次尝试**：运行时 `chat_with_fallback` 对不可达端点做 3 次指数退避（≈7s 纯等待），
   诊断场景应快速反馈，因此 `provider test/login` 走 registry 的单次 `chat()`。
5. **配置备份权限**：`.bak` 可能含上一轮写入的明文密钥，现与主文件同为 0600。

## 未覆盖（诚实标注）

- **云端 provider 首次真实调用**：本机无有效云端凭据
  （`GEMINI_API_KEY` 存在但 API 返回 `API_KEY_INVALID`，其余 provider env 为空）。
  因此「新用户 5 分钟内完成首次真实调用」的**云端**分支仍未被外部验证；
  本地真实 provider 分支已端到端通过。
- Google/Anthropic/Cohere 的自定义协议分支未做真实调用（无凭据），仅有 registry 构造路径的既有单测覆盖。
