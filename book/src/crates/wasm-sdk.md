# sisyphus-wasm-sdk — WASM Tool SDK

The WASM SDK provides the interface for developing tools that run inside the Sisyphus WASM sandbox. Tool authors implement the `SisyphusTool` trait and register their tools for discovery by the runtime.

## Core Types

### SisyphusTool

Trait that every WASM tool must implement:

```rust
pub trait SisyphusTool: Send + Sync {
    fn execute(&self, ctx: &ToolContext) -> Result<ToolOutput, String>;
}
```

### ToolContext

Input passed to a tool when invoked:

```rust
pub struct ToolContext {
    pub tool_id: String,
    pub input: Value,   // JSON input arguments
    pub state: Value,   // Current execution state
}
```

### ToolOutput

Output produced by a tool:

```rust
pub struct ToolOutput {
    pub result: Value,              // JSON result
    pub state_update: Option<Value>, // Optional state mutations
}

impl ToolOutput {
    pub fn json(val: Value) -> Self; // Convenience constructor
}
```

## Tool Registry

Tools are registered in a thread-safe global registry:

```rust
use sisyphus_wasm_sdk::{register_tool_as, list_tools, tool_count};

// Register a tool with an explicit ID
struct MyTool;
impl SisyphusTool for MyTool {
    fn execute(&self, ctx: &ToolContext) -> Result<ToolOutput, String> {
        Ok(ToolOutput::json(serde_json::json!({"status": "ok"})))
    }
}

register_tool_as("my_tool", MyTool);

// List all registered tools
let tools = list_tools(); // vec!["my_tool"]
assert_eq!(tool_count(), 1);
```

### Registry API

| Function | Description |
|----------|-------------|
| `register_tool_as(id, tool)` | Register a tool with explicit ID |
| `register_tool(tool)` | Register using type name as ID |
| `list_tools()` | List all registered tool IDs |
| `tool_count()` | Get number of registered tools |
| `remove_tool(id)` | Remove a tool, returns true if it existed |

## Example: Implementing a Tool

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

// Register the tool
register_tool_as("file_read", FileReader);
```

## State Management

Tools communicate state through `ToolContext.state` and `ToolOutput.state_update`:

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

## Integration

The WASM SDK is used by:
- **sisyphus-intel** — Tool code generation references SDK types
- **sisyphus-tests** — E2E tests exercise tool registration
- **sisyphus-core** — Runtime discovers and executes registered tools

## Testing

The crate includes unit tests for:
- Tool registration and retrieval
- Duplicate registration handling
- Tool removal
- Type serialization/deserialization
- Output construction
