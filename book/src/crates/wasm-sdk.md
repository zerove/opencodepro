# sisyphus-wasm-sdk — WASM 工具 SDK

WASM SDK 提供开发在 Sisyphus WASM 沙箱内运行的工具的接口。工具作者实现 `SisyphusTool` trait 并注册工具以供运行时发现。

## 核心类型

### SisyphusTool

每个 WASM 工具都必须实现的 trait：

```rust
pub trait SisyphusTool: Send + Sync {
    fn execute(&self, ctx: &ToolContext) -> Result<ToolOutput, String>;
}
```

### ToolContext

调用工具时传入的输入：

```rust
pub struct ToolContext {
    pub tool_id: String,
    pub input: Value,   // JSON 输入参数
    pub state: Value,   // 当前执行状态
}
```

### ToolOutput

工具产生的输出：

```rust
pub struct ToolOutput {
    pub result: Value,              // JSON 结果
    pub state_update: Option<Value>, // 可选状态变更
}

impl ToolOutput {
    pub fn json(val: Value) -> Self; // 便捷构造器
}
```

## 工具注册表

工具在线程安全的全局注册表中注册：

```rust
use sisyphus_wasm_sdk::{register_tool_as, list_tools, tool_count};

// 使用显式 ID 注册工具
struct MyTool;
impl SisyphusTool for MyTool {
    fn execute(&self, ctx: &ToolContext) -> Result<ToolOutput, String> {
        Ok(ToolOutput::json(serde_json::json!({"status": "ok"})))
    }
}

register_tool_as("my_tool", MyTool);

// 列出所有已注册工具
let tools = list_tools(); // vec!["my_tool"]
assert_eq!(tool_count(), 1);
```

### 注册表 API

| 函数 | 说明 |
|----------|-------------|
| `register_tool_as(id, tool)` | 使用显式 ID 注册工具 |
| `register_tool(tool)` | 使用类型名作为 ID 注册 |
| `list_tools()` | 列出所有已注册工具 ID |
| `tool_count()` | 获取已注册工具数量 |
| `remove_tool(id)` | 移除工具，存在时返回 true |

## 示例：实现一个工具

```rust
use sisyphus_wasm_sdk::{SisyphusTool, ToolContext, ToolOutput};
use serde_json::json;

struct FileReader;

impl SisyphusTool for FileReader {
    fn execute(&self, ctx: &ToolContext) -> Result<ToolOutput, String> {
        let path = ctx.input["path"]
            .as_str()
            .ok_or("missing 'path' in input")?;

        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read {}: {}", path, e))?;

        Ok(ToolOutput {
            result: json!({"content": content}),
            state_update: None,
        })
    }
}

// 注册工具
register_tool_as("file_read", FileReader);
```

## 状态管理

工具通过 `ToolContext.state` 与 `ToolOutput.state_update` 通信状态：

```rust
impl SisyphusTool for Counter {
    fn execute(&self, ctx: &ToolContext) -> Result<ToolOutput, String> {
        let current = ctx.state["count"].as_u64().unwrap_or(0);
        Ok(ToolOutput {
            result: json!({"count": current + 1}),
            state_update: Some(json!({"count": current + 1})),
        })
    }
}
```

## 集成

WASM SDK 被以下 crate 使用：
- **sisyphus-intel** — 工具代码生成引用 SDK 类型
- **sisyphus-tests** — E2E 测试测试工具注册
- **sisyphus-core** — 运行时发现并执行已注册工具

## 测试

crate 包含以下单元测试：
- 工具注册与检索
- 重复注册处理
- 工具移除
- 类型序列化/反序列化
- 输出构造