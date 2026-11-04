# 快速开始

本指南将带你完成 Sisyphus 的安装设置、认知引擎的运行，以及第一个 SOP 的执行。

## 环境要求

开始之前，请确保系统满足以下要求：

| 组件 | 要求 | 说明 |
|-----------|-------------|-------|
| **Rust** | `nightly-2026-08-08`（见 `rust-toolchain.toml`） | 版本由仓库 pin，`rustup` 会自动安装；不要手动装浮动 nightly（rustfmt 输出会漂移）。 |
| **Go** | 1.21+ | 构建 Go SDK（`crates/sisyphus-sdk-go`）所需。 |
| **Node.js** | 20+ | TypeScript SDK 与 MCP 工具服务器（`workers/ts-tools-mcp`）所需。 |
| **操作系统** | Linux / macOS | Windows 暂不支持 bridge 与 sandbox。 |

安装 Rust（版本由 `rust-toolchain.toml` 决定，进入仓库目录后 `rustup` 会自动安装）：

```bash
# 可选：提前显式安装 pin 住的版本（含 rustfmt/clippy）
rustup toolchain install "$(sed -n 's/^channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' rust-toolchain.toml)" \
    --component rustfmt --component clippy
```

验证环境：

```bash
rustc --version  # 应显示 nightly-2026-xx-xx 或类似版本
cargo --version
```

## 快速开始

### 1. 克隆并构建

```bash
git clone https://github.com/zeronhq/sisyphus-wasm.git
cd sisyphus-wasm

# 检查工作区（为提速排除重型 bridge/sandbox crate）
make check-all

# 构建完整工作区
make build-all
```

> **注意：** 完整 `cargo check --workspace` 在内存低于 16 GB 的机器上可能 OOM，因为 `sisyphus-sandbox` 和 `sisyphus-bridge` 引入了 wasmtime/cranelift。日常开发请使用 `make check-all`（排除重型 crate）。

### 2. 配置

复制示例配置并按需调整：

```bash
cp config.example.toml config.toml
```

默认配置可直接使用位于 `http://localhost:8000/v1/chat/completions` 的 LLM 端点。设置 `SISYPHUS_LLM_API_KEY` 和 `SISYPHUS_LLM_ENDPOINT` 指向您的 LLM 提供商：

```bash
export SISYPHUS_LLM_API_KEY="your-api-key"
export SISYPHUS_LLM_ENDPOINT="https://api.openai.com/v1/chat/completions"
export SISYPHUS_LLM_MODEL="gpt-4o"
```

查看[配置](./configuration.md)页面了解所有可用选项。

### 3. 运行 Bridge

gRPC bridge 是引擎的主要入口，同时暴露 gRPC（端口 50051）与 HTTP REST（端口 8080）API：

```bash
cargo run -p sisyphus-bridge
```

默认情况下 bridge 监听 `http://[::1]:50051`（IPv6，自动回退 IPv4）。可用以下方式覆盖：

```bash
cargo run -p sisyphus-bridge -- --address http://127.0.0.1:50051
```

您会看到 bridge 已启动的日志输出：

```
[2026-07-30T11:40:00Z INFO  sisyphus_bridge] Starting gRPC bridge on [::1]:50051
[2026-07-30T11:40:00Z INFO  sisyphus_bridge] HTTP REST API on [::1]:8080
```

### 4. 连接 TUI

在另一个终端中启动终端用户界面（Terminal User Interface，TUI）：

```bash
cargo run -p sisyphus-cli
```

如果 bridge 运行在非默认地址，通过 `--bridge` 标志指定：

```bash
cargo run -p sisyphus-cli -- --bridge http://127.0.0.1:50051
```

TUI 连接 bridge 并显示主控制台。按 `Ctrl+N` 创建新会话，然后用自然语言输入一个目标。

参见[ TUI 指南](./tui-guide.md) 了解键盘快捷键与功能。

### 5. 用健康检查验证

bridge 运行后，用快速 REST API 调用验证连通性：

```bash
curl http://[::1]:8080/health
```

预期响应：

```json
{"status":"ok","version":"0.2.0"}
```

## 简单示例：执行一个 SOP

bridge 运行且 TUI 已连接后，按以下步骤操作：

1. **新建会话** — 在 TUI 中按 `Ctrl+N`。
2. **输入目标** — 输入自然语言目标，例如：
   ```
   Create a Markdown file named "hello-sisyphus.md" with the content "# Hello, Sisyphus!"
   ```
3. **观察认知循环** — 引擎将：
   - 合成 SOP（子步骤 DAG）
   - 使用 WASM 工具或 LLM 调用执行每个步骤
   - 反思结果，必要时演进 SOP
   - 目标达成时收敛
4. **查看结果** — 完成的会话显示执行的步骤、偏差分数与最终输出。

您也可以通过 REST API 交互：

```bash
# 提交目标
curl -X POST http://[::1]:8080/v1/goals \
  -H "Content-Type: application/json" \
  -d '{"goal": "Create a Markdown file named hello-sisyphus.md with content: # Hello, Sisyphus!"}'

# 查询目标状态（将 {id} 替换为返回的目标 ID）
curl http://[::1]:8080/v1/goals/{id}

# 列出所有目标
curl http://[::1]:8080/v1/goals
```

### 以库方式运行 SOP（Rust）

如果您将 Sisyphus 作为库集成：

```rust
use sisyphus_core::kernel::SisyphusKernel;
use sisyphus_types::config::Config;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_file("config.toml")?;
    let mut kernel = SisyphusKernel::new(config).await?;

    let result = kernel.execute_goal("Create a file named hello.md").await?;
    println!("Result: {:?}", result);

    Ok(())
}
```

## 运行测试

Sisyphus 拥有全面的测试套件，按不同范围使用以下命令：

```bash
make test-all          # 运行全部测试（CI 等价）
make test-core         # 完整 sisyphus-core 测试套件（含 wasmtime）
make test-core-quick   # 仅 lib 的 sisyphus-core 测试（约 2 秒）
make test-tools        # 测试全部 10 个工具子 crate（仅 lib，快速）
cargo test -p sisyphus-mem  # 存储层测试
cargo bench -p sisyphus-core  # 认知循环基准测试
```

## 下一步

- [架构总览](./architecture.md) — 了解系统设计与认知循环
- [配置](./configuration.md) — 了解所有配置选项与环境变量
- [TUI 指南](./tui-guide.md) — 掌握终端界面的键盘快捷键与斜杠命令
- [Web 仪表盘](./web-dashboard.md) — 在浏览器中实时监控与 DAG 可视化
- [SDK 参考](./sdk-reference.md) — 使用 TypeScript、Python 或 Go SDK 构建集成
- [REST API](./rest-api.md) — 通过 HTTP 端点编程控制引擎
- [安全](./security.md) — 认证、沙箱与部署加固
- [部署](./deployment.md) — 生产部署指南与最佳实践
- [性能](./performance.md) — 调优认知循环、缓存与沙箱配置