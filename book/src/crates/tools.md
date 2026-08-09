# sisyphus-tools — 内置工具与代理

工具生态拆分为 10 个子 crate，以获得更快的编译速度与更清晰的依赖跟踪。CLI 与 bridge 都直接依赖子 crate 完成实际工作。

| 子 crate | 说明 |
|-----------|-------------|
| `sisyphus-tools-core` | 基础：错误类型、权限、钩子、反馈、bridge 类型 |
| `sisyphus-tools-llm` | LLM 抽象：客户端 traits、提供商、模型路由、metis 分析 |
| `sisyphus-tools-scm` | 源码控制：git、patch、hashline、github 集成 |
| `sisyphus-tools-build` | 构建系统：lsp、benchmark、测试生成、自动修复 |
| `sisyphus-tools-system` | 系统工具：bash、file、web、权限、explore、scaffold |
| `sisyphus-tools-mcp` | MCP 协议：客户端、工具注册表、自定义工具定义 |
| `sisyphus-tools-llm-tools` | LLM 驱动代理：librarian、oracle、momus、scribe、security |
| `sisyphus-tools-session` | 会话管理：session、skill、todo、plan、precommit |
| `sisyphus-tools-executor` | 工具执行引擎：BuiltinToolExecutor、MCP 执行器 |
| `sisyphus-tools-agent` | 代理编排：代理注册、团队、委托、开发者 |

## 架构

```
Tool Executor (BuiltinToolExecutor)
  ├── 文件工具  (read, write, edit, glob, grep)
  ├── Shell 工具 (bash_exec)
  ├── Web 工具   (web_search, web_fetch)
  ├── Agent 工具 →  Librarian (research, fetch_docs, examples, synthesise)
  │                  ├── Metis   (analyse)
  │                  └── Momus   (review_plan, review_code, review_arch)
  └── MCP 工具 →  via stdio 或 HTTP 的外部 MCP 服务器
```

## BuiltinToolExecutor

按字符串 `tool_id` 分发工具调用的 `ToolExecutor`，参数为 JSON：

```rust
// 所有工具遵循相同接口
async fn execute(&self, tool_id: &str, args: Vec<u8>, fuel_limit: u64) -> Result<Vec<u8>, String>
```

### 文件工具

| 工具 ID | 参数 | 结果 |
|---------|------|--------|
| `file_read` | `{"path": "..."}` | `{"content": "..."}` |
| `file_write` | `{"path": "...", "content": "..."}` | `{"bytes_written": N}` |
| `file_edit` | `{"path": "...", "old": "...", "new": "..."}` | `{"replaced": true}` |
| `file_edit_all` | `{"path": "...", "old": "...", "new": "..."}` | `{"count": N}` |
| `file_read_lines` | `{"path": "...", "offset": N, "limit": N}` | `{"content": "..."}` |
| `glob` | `{"pattern": "...", "root": "..."}` | `{"files": ["..."]}` |
| `grep` | `{"pattern": "...", "path": "...", "include": "..."}` | `{"matches": [...]}` |

### Shell 工具

| 工具 ID | 参数 | 结果 |
|---------|------|--------|
| `bash_exec` | `{"command": "...", "workdir": "...", "timeout_ms": N}` | `{"stdout": "...", "stderr": "...", "exit_code": N}` |

### 网络工具

| 工具 ID | 参数 | 结果 |
|---------|------|--------|
| `web_search` | `{"query": "...", "num": N}` | `{"results": [...]}` |
| `web_fetch` | `{"url": "..."}` | `{"content": "...", "content_type": "..."}` |

### Agent 工具

| 工具 ID | 参数 | 结果 |
|---------|------|--------|
| `librarian/research` | `{"query": "...", "scope": "web\|docs\|code\|all", "num": N}` | `ResearchReport` JSON |
| `librarian/fetch_docs` | `{"url": "..."}` | `DocPage` JSON |
| `librarian/examples` | `{"query": "...", "language": "..."}` | `[CodeExample]` JSON |
| `librarian/synthesise` | `{"findings": [...], "max_points": N}` | `{"synthesis": "..."}` |
| `metis/analyse` | `{"request": "..."}` | `MetisAnalysis` JSON |
| `momus/review_plan` | `{"plan_text": "...", "plan_name": "..."}` | `MomusReview` JSON |
| `momus/review_code` | `{"path": "...", "content": "..."}` | `MomusReview` JSON |
| `momus/review_code_batch` | `{"files": [{"path": "...", "content": "..."}]}` | `[MomusReview]` JSON |
| `momus/review_arch` | `{"arch_text": "..."}` | `MomusReview` JSON |

## Agent 模块

### Librarian — 外部调研代理

通过搜索网络、获取文档与查找代码示例执行多源调研，返回带归属发现的结构化报告。

```rust
// 跨 web、docs 或 code 调研主题
pub async fn research(query: &str, scope: ResearchScope, max_results: usize) -> ToolResult<ResearchReport>;

// 从 URL 获取并解析文档
pub async fn fetch_documentation(url: &str) -> ToolResult<DocPage>;

// 查找库或 API 的代码示例
pub async fn find_examples(query: &str, language: &str) -> ToolResult<Vec<CodeExample>>;

// 将发现列表综合为连贯摘要
pub fn synthesise(findings: &[ResearchFinding], max_points: usize) -> String;
```

### Metis — 预规划分析代理

实现开始前分析自然语言请求，挖掘歧义与隐含需求。

```rust
pub fn analyse(request: &str) -> MetisAnalysis;
```

`MetisAnalysis` 包含 `summary`，一组 `ambiguities`（各有 severity、aspect、options、recommendation），以及一组 `implicit_requirements`（带置信度与理由）。

### Momus — 审查代理

审查计划、代码与架构，评估完整性、一致性以及最佳实践遵循度。

```rust
pub fn review_plan(plan_text: &str, plan_name: &str) -> MomusReview;
pub fn review_code(path: &Path, content: &str) -> ToolResult<MomusReview>;
pub fn review_code_batch(files: &[(&Path, &str)]) -> ToolResult<Vec<MomusReview>>;
pub fn review_architecture(arch_text: &str) -> MomusReview;
```

## ConfigStore

加载并監控 TOML 配置文件变更。返回共享的 `Arc<RwLock<Config>>`，可运行时热重载。

```rust
// 从文件加载并开始轮询变更
pub fn load(path: &str) -> Result<(Self, Arc<RwLock<Config>>), String>;

// 从现有配置创建 store（无文件监听）
pub fn with_config(config: Arc<RwLock<Config>>) -> Self;

// 检查文件变更的后台循环
pub async fn start_watching(&self);
```

## MCPToolExecutor

通过 stdio或 HTTP 连接外部 MCP（Model Context Protocol）服务器。每个 MCP 服务器在 `ExecutorRouter` 中以前缀注册。