use std::io::Write;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};

mod client;
mod handlers;

pub use client::{run_call, run_info};

/// MCP 工具处理器使用的全局 LLM 客户端。
static LLM_CLIENT: std::sync::OnceLock<Arc<dyn crate::tools::metis::LlmClient>> =
    std::sync::OnceLock::new();

// ── 工具注册表 ─────────────────────────────────────────────

struct ToolDef {
    name: &'static str,
    description: &'static str,
    input_schema: serde_json::Value,
}

static TOOLS: std::sync::OnceLock<Vec<ToolDef>> = std::sync::OnceLock::new();

fn tool_registry() -> &'static [ToolDef] {
    TOOLS.get_or_init(|| {
        vec![
            ToolDef {
                name: "metis_analyse",
                description: "Analyse a request for ambiguities, implicit requirements, and missing context. Use this BEFORE implementing any ambiguous task.",
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "request": {
                            "type": "string",
                            "description": "The request text to analyse for ambiguities"
                        }
                    },
                    "required": ["request"]
                }),
            },
            ToolDef {
                name: "momus_review_plan",
                description: "Review a plan for completeness, consistency, and verifiability. Catches missing steps, ambiguous outcomes, and unverifiable claims.",
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "plan_text": {
                            "type": "string",
                            "description": "The full plan text with steps, dependencies, and expected outcomes"
                        },
                        "plan_name": {
                            "type": "string",
                            "description": "A short descriptive name for the plan"
                        }
                    },
                    "required": ["plan_text", "plan_name"]
                }),
            },
            ToolDef {
                name: "momus_review_code",
                description: "Review source code for quality, correctness, and adherence to best practices. Checks error handling, performance, and type safety.",
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "File path to the code being reviewed"
                        },
                        "content": {
                            "type": "string",
                            "description": "The full source code content to review"
                        }
                    },
                    "required": ["path", "content"]
                }),
            },
            ToolDef {
                name: "momus_review_architecture",
                description: "Review an architecture description for consistency, modularity, dependency management, and scalability concerns.",
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "arch_text": {
                            "type": "string",
                            "description": "The architecture description or ADR text to review"
                        }
                    },
                    "required": ["arch_text"]
                }),
            },
            ToolDef {
                name: "librarian_research",
                description: "Perform multi-source web research on a topic. Searches the web, fetches top results, and returns a structured report with findings and synthesis.",
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "The research question or topic to search for"
                        },
                        "scope": {
                            "type": "string",
                            "description": "Search scope: 'web' (default), 'docs', 'code', or 'all'",
                            "enum": ["web", "docs", "code", "all"]
                        },
                        "max_results": {
                            "type": "integer",
                            "description": "Maximum results to fetch (default: 5, max: 20)",
                            "minimum": 1,
                            "maximum": 20
                        }
                    },
                    "required": ["query"]
                }),
            },
            ToolDef {
                name: "librarian_fetch_docs",
                description: "Fetch a specific URL and return structured documentation with a content preview.",
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "The URL to fetch documentation from"
                        }
                    },
                    "required": ["url"]
                }),
            },
            ToolDef {
                name: "librarian_find_examples",
                description: "Search for code examples of a specific library or API pattern. Returns code snippets with descriptions.",
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "What to search for, e.g. 'react useState hook' or 'rust async await'"
                        },
                        "language": {
                            "type": "string",
                            "description": "Programming language (e.g. 'rust', 'typescript', 'python')",
                            "default": "rust"
                        }
                    },
                    "required": ["query"]
                }),
            },
            ToolDef {
                name: "librarian_synthesise",
                description: "Synthesise multiple research findings into a concise ranked summary.",
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "findings_json": {
                            "type": "string",
                            "description": "JSON array of ResearchFinding objects, each with 'claim', 'source_url', 'source_title', 'relevance' (0-1)"
                        },
                        "max_points": {
                            "type": "integer",
                            "description": "Maximum number of points to include in the synthesis (default: 5)",
                            "minimum": 1,
                            "maximum": 20
                        }
                    },
                    "required": ["findings_json"]
                }),
            },
        ]
    })
}

// ── MCP 服务器 ───────────────────────────────────────────────

/// 启动暴露 Sisyphus 代理工具的 MCP stdio 服务器。
/// 运行直到 stdin 关闭。
pub async fn run_serve(client: Arc<dyn crate::tools::metis::LlmClient>) -> anyhow::Result<()> {
    LLM_CLIENT
        .set(client)
        .map_err(|_| anyhow::anyhow!("LLM client already initialized"))?;

    let stdin = tokio::io::stdin();
    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();

    while let Some(line) = lines.next_line().await? {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let request: crate::tools::JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let resp = crate::tools::JsonRpcResponse {
                    jsonrpc: "2.0".into(),
                    id: None,
                    result: None,
                    error: Some(crate::tools::JsonRpcError {
                        code: crate::tools::MCP_ERROR_PARSE,
                        message: format!("JSON parse error: {e}"),
                        data: None,
                    }),
                };
                let out = serde_json::to_string(&resp)?;
                println!("{out}");
                std::io::stdout().flush()?;
                continue;
            }
        };

        let response = handlers::handle_request(request).await;

        let out = serde_json::to_string(&response)?;
        println!("{out}");
        std::io::stdout().flush()?;
    }

    Ok(())
}
