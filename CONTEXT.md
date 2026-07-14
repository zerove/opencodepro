# Sisyphus 项目上下文 (CONTEXT.md)

## 1. 项目愿景与目标
Sisyphus 是一个基于 DAG (有向无环图) 的认知引擎实现。其核心目标是构建一个能够通过“预测 $\to$ 执行 $\to$ 反思 $\to$ 演进”这一闭环认知循环，自主完成复杂任务的系统。

不同于简单的线性 Pipeline，Sisyphus 允许任务被分解为依赖图，并根据执行结果动态地调整后续行为。

## 2. 核心认知模型
系统的核心是**认知循环 (Cognitive Loop)**：
- **Predict (预测)**: 根据当前状态和目标，规划下一步需要执行的操作（生成 DAG 节点）。
- **Execute (执行)**: 调用具体工具或代理执行节点任务。
- **Reflect (反思)**: 分析执行结果，对比预期目标，识别偏差或错误。
- **Evolve (演进)**: 基于反思结果，修正认知模型或调整执行计划，进入下一个循环。

## 3. 系统架构
项目采用 Rust 工作区管理，分为多个专业化 Crate：

### 核心层 (Core)
- `sisyphus-core`: ★ 认知循环的实现中心。负责 DAG 调度、认知状态管理和 SOP (标准作业程序) 编排。
- `sisyphus-types`: 定义全局共享的类型、配置和 Trait。
- `sisyphus-mem`: 负责状态的持久化与内存管理。

### 接口层 (Interface)
- `sisyphus-bridge`: gRPC 桥接服务，将内核能力暴露给外部。
- `sisyphus-cli`: 基于 `ratatui` 的 TUI 客户端，提供交互界面。
- `sisyphus-bridge-api`: 定义 gRPC 的 `.proto` 接口。

### 工具层 (Tools)
采用高度模块化的工具集 (`sisyphus-tools-*`)，包括：
- `sisyphus-tools-llm`: LLM 适配器。
- `sisyphus-tools-system`: 文件、Bash、Web 操作。
- `sisyphus-tools-mcp`: Model Context Protocol 集成。
- `sisyphus-tools-build`: 构建、测试、LSP 交互。
- `sisyphus-tools-agent`: 代理注册与委托。

## 4. 关键工作流
### 任务处理流程
用户请求 $\to$ `sisyphus-cli` $\to$ `sisyphus-bridge` $\to$ `sisyphus-core` (认知循环) $\to$ `sisyphus-tools-*` $\to$ 结果回传 $\to$ 用户。

### 开发者快速上手
- **编译全部**: `cargo build-all`
- **运行测试**: `cargo test-all`
- **核心基准测试**: `cargo bench -p sisyphus-core`
- **检查代码**: `cargo check-all`

## 5. 文档地图
为了快速理解项目，请参考以下文档：
- **[AGENTS.md](./AGENTS.md)**: 定义了项目中的专业代理及其指令（这是 AI 代理的首选指南）。
- **[UBIQUITOUS_LANGUAGE.md](./UBIQUITOUS_LANGUAGE.md)**: 领域统一语言表，定义核心术语。
- **[mdbook 文档](./book/)**: 详细的概念讲解（认知循环、DAG 执行等）。
- **[架构决策记录 (ADR)](./docs/adr/)**: 记录关键技术选型及其理由。
- **[路线图](./docs/ROADMAP.md)**: 项目演进方向。

## 6. 开发规范
- **错误处理**: 严禁 `unwrap()`，必须使用 `thiserror` 定义的枚举错误并在调用链中传播。
- **异步编程**: 统一使用 `tokio` + `async-trait`。
- **配置管理**: 变更配置需同步更新 `config.example.toml` 和相关文档。
