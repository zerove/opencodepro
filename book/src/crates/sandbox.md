# sisyphus-sandbox — WASM 运行时

使用 [Wasmtime](https://wasmtime.dev/) 提供沙箱化 WASM 运行时，安全执行工具代码。

## 关键类型：`WasmToolExecutor`

```rust
pub struct WasmToolExecutor {
    engine: Engine,
    tools_path: PathBuf,
    module_cache: Arc<DashMap<String, Module>>,
}
```

### 构造

```rust
// 默认 — 查找 CWD 相对 tools/ 目录
let exec = WasmToolExecutor::new()?;

// 自定义路径与内存限制
let exec = WasmToolExecutor::with_config(
    Some(PathBuf::from("/opt/tools")),
    Some(20),  // 页（20 × 64 KiB = 1.25 MiB）
)?;
```

创建带以下配置的 Wasmtime `Engine`：

- **池化实例分配**：预分配内存池，快速实例化
- **内存限制**：每实例最大内存可配置（默认 10 页 = 640 KiB）
- **燃料计量**：可配置燃料上限，防止失控执行（默认 10,000）
- **栈限制**：最大 1,000,000 个 WASM 栈帧

### 执行流程

1. **加载**：从 `{tools_path}/{tool_id}.wasm` 读取 `.wasm` 二进制
2. **编译**：编译为 Wasmtime `Module`（缓存在 `DashMap` 中避免重新编译）
3. **实例化**：创建带每次调用燃料分配的实例
4. **调用**：调用导出的 `run` 函数，传入输入字节
5. **返回**：返回输出字节，或燃料/内存耗尽错误

```rust
#[async_trait]
impl ToolExecutor for WasmToolExecutor {
    async fn execute(&self, tool_id: &str, args: Vec<u8>, fuel_limit: u64) -> Result<Vec<u8>, String> {
        // ...
    }
}
```

### 错误处理

| 错误 | 含义 | 认知循环响应 |
|-------|---------|------------------------|
| `ResourceExhausted: fuel` | WASM 指令耗尽 | 以 2× 燃料重试 |
| `ResourceExhausted: memory` | 超过线性内存限制 | 失败 — 重新设计节点 |
| `Unknown export: run` | WASM 模块没有 `run` 导出 | 失败 — 无效工具 |
| I/O 错误 | 工具文件未找到 | 失败 — 缺少工具 |

### 燃料系统

Wasmtime 的燃料计量跟踪 WASM 指令消耗。燃料耗尽时立即中止执行。认知循环将其解释为通过 `AdaptiveFuelPricer` 以增加燃料重试的信号。

### 控制台输出捕获

WASM 模块的 stdout/stderr 通过 `WasmtimeConsole` 捕获并经 `tracing` 路由。