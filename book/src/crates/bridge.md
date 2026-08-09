# sisyphus-bridge — 入口点与 API

将各组件整合为可运行的 gRPC 服务的二进制 crate。

## 入口点

### `main.rs`（gRPC 服务器）

监听 `[::1]:50051` 的 Tonic gRPC 服务器：

```protobuf
service SisyphusBridge {
  rpc ExecuteSOP (ExecuteSOPRequest) returns (SOPResponse);
  rpc ExecuteTool (ExecuteToolRequest) returns (ExecuteToolResponse);
  rpc HealthCheck (HealthCheckRequest) returns (HealthCheckResponse);
  rpc ListSOPs (ListSOPsRequest) returns (ListSOPsResponse);
  rpc StreamExecution (ExecuteSOPRequest) returns (stream ExecutionEvent);
}
```

**`ExecuteSOP`** 支持两种模式：
- **目标执行**：若 `sop_id` 以 `"goal:"` 开头，bridge 合成、执行并运行认知循环
- **直接执行**：否则按 ID 加载已保存的 SOP 并执行

### 请求/响应

```protobuf
message ExecuteSOPRequest {
  string sop_id = 1;
  map<string, string> args = 2;
}

message SOPResponse {
  string status = 1;   // "SUCCESS" 或错误字符串
  string result = 2;   // 执行结果 + 认知报告
}
```

## 初始化流程

```
Config (TOML + 环境变量)
    │
    ▼
MfsStore ← SQLite 连接
    │
    ▼
WasmToolExecutor ← tools/ 目录
    │
    ▼
SisyphusIntel ← 模拟器 + 反思器 + 演进器
    │
    ▼
GoalDrivenSynthesizer ← 工具执行器
    │
    ▼
SisyphusKernel ← state_store + tool_executor + intel + synthesizer
    │
    ▼
MySisyphusBridge ← 包装 kernel
    │
    ▼
gRPC 服务器 ← tonic 监听 [::1]:50051
```

## 配置

Bridge 读取 `config.toml`（或 `$SISYPHUS_CONFIG` 路径）并调用 `config.apply_env_overrides()` 合并环境变量。指标在 `config.metrics.prometheus_port` 指定的端口初始化。

### ExecuteTool — 直接工具执行

通过 `ExecutorRouter` 直接调用任意内置或代理工具。可用于远程访问 `librarian`、`metis`、`momus` 以及所有文件/shell/web 工具。

```protobuf
message ExecuteToolRequest {
  string tool_id = 1;      // 例如 "file_read", "librarian/research", "metis/analyse"
  string args_json = 2;    // JSON 编码的参数（schema 见 crates/tools.md）
  uint64 fuel_limit = 3;   // 最大执行单位（0 = 默认）
}

message ExecuteToolResponse {
  string status = 1;       // "SUCCESS" 或错误消息
  string result_json = 2;  // JSON 编码的结果
}
```

通过 CLI 的 `BridgeClient` 使用：

```rust
let mut client = BridgeClient::connect("http://[::1]:50051").await?;
let resp = client.execute_tool(
    "librarian/research",
    r#"{"query": "Rust async", "scope": "web", "num": 5}"#,
).await?;
println!("{}", resp.result_json);
```

## 运行

```bash
# 构建
cargo build --release -p sisyphus-bridge

# 使用默认配置运行
./target/release/sisyphus-bridge

# 自定义配置
SISYPHUS_CONFIG=/etc/sisyphus/prod.toml ./target/release/sisyphus-bridge
```