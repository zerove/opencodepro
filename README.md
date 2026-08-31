# Sisyphus 认知引擎

基于 DAG 的认知执行引擎，实现预测 → 执行 → 反思 → 演进循环。

## 架构

```
sisyphus/
├── crates/           # 28 个 Rust crate
│   ├── sisyphus-core/        # DAG 执行、认知循环、SOP 编排
│   ├── sisyphus-cli/         # TUI 客户端 (ratatui + crossterm)
│   ├── sisyphus-bridge/      # gRPC + REST 桥接服务 (tonic + axum)
│   ├── sisyphus-types/       # 共享类型、配置、trait
│   ├── sisyphus-mem/         # 持久化/存储 (SQLite/Postgres + 向量)
│   ├── sisyphus-intel/       # 情报分析 (仿真、反思、演进)
│   ├── sisyphus-tools-*/     # 工具 crate (agent、build、executor、llm、mcp、scm、session、system)
│   ├── sisyphus-roadmap/     # 战略规划 (Project → Milestone → Phase → Goal)
│   ├── sisyphus-api/         # REST API 处理器 (axum + OpenAPI 3.1)
│   ├── sisyphus-sdk/         # Rust SDK 客户端
│   ├── sisyphus-knowledge/   # 知识图谱与认知资产存储
│   ├── sisyphus-visual/      # 视觉感知与嵌入
│   ├── sisyphus-wasm-sdk/    # WASM 工具 SDK (guest crate)
│   ├── sisyphus-sandboxd/    # WASM 沙箱守护进程 (进程隔离)
│   ├── sisyphus-tests/       # 集成测试
│   └── ...
├── webui/            # Web 仪表盘 (axum + HTMX + SSE)
├── evals/            # Agent 能力评测
├── ide/              # IDE 扩展
│   ├── typescript-sdk/       # TypeScript 客户端 SDK
│   ├── python-sdk/           # Python 客户端 SDK
│   ├── go-sdk/               # Go 客户端 SDK
│   ├── vscode/               # VS Code 扩展
│   └── zed/                  # Zed 扩展
└── book/             # 文档 (mdbook)
```

## 快速开始

```bash
# 安装
cargo install sisyphus-cli

# 运行 TUI
sisyphus

# 使用自定义 bridge 地址
sisyphus --bridge http://localhost:50051
```

## 功能特性

### 核心引擎
- **DAG 执行**：基于拓扑排序的并行节点调度
- **认知循环**：预测 → 执行 → 反思 → 演进
- **SOP 编排**：标准作业程序即可执行图
- **子图递归**：带上下文隔离的嵌套 SOP 执行
- **推测性执行**：后继节点预测预计算

### TUI 终端界面
- **斜杠命令**：输入 `/` 唤起命令补全弹窗
- **@文件引用**：使用 `@path/to/file.rs` 引用文件
- **模糊搜索**：基于评分的全命令模糊匹配
- **会话管理**：创建、切换、分叉、撤销会话
- **命令面板**：Ctrl+K 快速导航
- **DAG 可视化**：带状态颜色的 ASCII 艺术图 DAG 渲染

### Web 仪表盘
- **SSE 实时推送**：Server-Sent Events 实现实时更新
- **WebSocket**：双向通信
- **DAG 查看器**：交互式 SVG 可视化
- **会话详情**：消息历史与发送消息表单
- **SOP 执行**：从浏览器运行 SOP
- **技能市场**：浏览与安装技能
- **Prometheus 指标**：/metrics 端点

### IDE 扩展
- **VS Code**：状态栏、DAG 查看器、Agent 面板、LSP 诊断
- **Zed**：扩展脚手架与安装提示

### SDK
- **TypeScript**：`@sisyphus/sdk` - 全类型 REST 客户端
- **Python**：`sisyphus_sdk` - 基于 dataclass 的客户端
- **Go**：`sisyphus` - 惯用 Go 客户端

### 性能优化
- **流水线批处理**：节点执行自适应批大小
- **输出去重缓存**：内容哈希去重 (1000 条目)
- **上下文压缩**：大值截断
- **执行分析器**：缓存命中率、最慢节点

### 安全
- **API Key 认证**：Bearer token 认证
- **外部目录权限**：可配置的文件访问控制
- **RBAC 模型**：基于角色的访问控制 (规划中)

### 可观测性
- **Prometheus 指标**：请求计数、会话统计、运行时间
- **链路追踪**：基于 tracing-subscriber 的结构化日志
- **SSE 事件**：实时执行事件流

## 配置

完整选项见 `config.example.toml`。关键设置：

```toml
[bridge]
address = "[::1]:50051"

[llm]
provider = "openai"
model = "gpt-4"

[tools]
shell_enabled = true
file_access_root = "."
```

环境变量可用 `{env:VAR}` 或 `{env:VAR|default}` 替换。

## 开发

```bash
# 一键本地验证（fmt + clippy + lib tests + bridge）
make dev

# 快速检查（不含重型 crate）
make ci-quick

# 单独步骤
make dev-fmt       # cargo fmt --all --check
make dev-clippy    # cargo clippy -D warnings (排除重型)
make dev-test      # cargo test --lib (排除重型)

# 完整命令
cargo check-all          # 检查全部 crate
cargo test-all           # 测试全部 crate
cargo build --release -p sisyphus-cli  # 构建 release TUI
make test-core           # 完整 core 测试
cargo bench -p sisyphus-core  # 基准测试
```

## 许可证

MIT