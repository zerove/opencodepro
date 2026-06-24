# sisyphus-tools — Built-in Tools & Agents

The crate that provides all built-in tool execution, agent functions, and utility operations. Both the CLI and the bridge use this crate to perform real work.

## Architecture

```
Tool Executor (BuiltinToolExecutor)
  ├── File tools   (read, write, edit, glob, grep)
  ├── Shell tools  (bash_exec)
  ├── Web tools    (web_search, web_fetch)
  ├── Agent tools  →  Librarian (research, fetch_docs, examples, synthesise)
  │                  ├── Metis   (analyse)
  │                  └── Momus   (review_plan, review_code, review_arch)
  └── MCP tools    →  External MCP servers via stdio or HTTP
```

## BuiltinToolExecutor

A `ToolExecutor` that dispatches tool calls by string `tool_id` with JSON-encoded arguments:

```rust
// All tools follow the same interface
async fn execute(&self, tool_id: &str, args: Vec<u8>, fuel_limit: u64) -> Result<Vec<u8>, String>
```

### File tools

| Tool ID | Args | Result |
|---------|------|--------|
| `file_read` | `{"path": "..."}` | `{"content": "..."}` |
| `file_write` | `{"path": "...", "content": "..."}` | `{"bytes_written": N}` |
| `file_edit` | `{"path": "...", "old": "...", "new": "..."}` | `{"replaced": true}` |
| `file_edit_all` | `{"path": "...", "old": "...", "new": "..."}` | `{"count": N}` |
| `file_read_lines` | `{"path": "...", "offset": N, "limit": N}` | `{"content": "..."}` |
| `glob` | `{"pattern": "...", "root": "..."}` | `{"files": ["..."]}` |
| `grep` | `{"pattern": "...", "path": "...", "include": "..."}` | `{"matches": [...]}` |

### Shell tools

| Tool ID | Args | Result |
|---------|------|--------|
| `bash_exec` | `{"command": "...", "workdir": "...", "timeout_ms": N}` | `{"stdout": "...", "stderr": "...", "exit_code": N}` |

### Web tools

| Tool ID | Args | Result |
|---------|------|--------|
| `web_search` | `{"query": "...", "num": N}` | `{"results": [...]}` |
| `web_fetch` | `{"url": "..."}` | `{"content": "...", "content_type": "..."}` |

### Agent tools

| Tool ID | Args | Result |
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

## Agent Modules

### Librarian — External Research Agent

Performs multi-source research by searching the web, fetching documentation, and finding code examples. Returns structured reports with attributed findings.

```rust
// Research a topic across web, docs, or code
pub async fn research(query: &str, scope: ResearchScope, max_results: usize) -> ToolResult<ResearchReport>;

// Fetch and parse documentation from a URL
pub async fn fetch_documentation(url: &str) -> ToolResult<DocPage>;

// Find code examples for a library or API
pub async fn find_examples(query: &str, language: &str) -> ToolResult<Vec<CodeExample>>;

// Synthesise a list of findings into a coherent summary
pub fn synthesise(findings: &[ResearchFinding], max_points: usize) -> String;
```

### Metis — Pre-Planning Analysis Agent

Analyses natural language requests to uncover ambiguities and implicit requirements before implementation begins.

```rust
pub fn analyse(request: &str) -> MetisAnalysis;
```

`MetisAnalysis` contains a `summary`, a list of `ambiguities` (each with severity, aspect, options, and recommendation), and a list of `implicit_requirements` (with confidence score and rationale).

### Momus — Review Agent

Reviews plans, code, and architecture for completeness, consistency, and adherence to best practices.

```rust
pub fn review_plan(plan_text: &str, plan_name: &str) -> MomusReview;
pub fn review_code(path: &Path, content: &str) -> ToolResult<MomusReview>;
pub fn review_code_batch(files: &[(&Path, &str)]) -> ToolResult<Vec<MomusReview>>;
pub fn review_architecture(arch_text: &str) -> MomusReview;
```

## ConfigStore

Loads and watches a TOML configuration file for changes. Returns a shared `Arc<RwLock<Config>>` that can be hot-reloaded at runtime.

```rust
// Load from file and start polling for changes
pub fn load(path: &str) -> Result<(Self, Arc<RwLock<Config>>), String>;

// Create store from existing config (no file watching)
pub fn with_config(config: Arc<RwLock<Config>>) -> Self;

// Background loop that checks for file changes
pub async fn start_watching(&self);
```

## MCPToolExecutor

Connects to external MCP (Model Context Protocol) servers via stdio or HTTP. Each MCP server is registered under a prefix in the `ExecutorRouter`.
