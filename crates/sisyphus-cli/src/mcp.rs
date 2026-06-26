use anyhow::{Context, Result};
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use tokio::io::{AsyncBufReadExt, BufReader};

/// Global LLM client used by MCP tool handlers.
static LLM_CLIENT: OnceLock<Arc<dyn sisyphus_tools::metis::LlmClient>> = OnceLock::new();

// ── Tool Registry ─────────────────────────────────────────────

struct ToolDef {
    name: &'static str,
    description: &'static str,
    input_schema: serde_json::Value,
}

static TOOLS: OnceLock<Vec<ToolDef>> = OnceLock::new();

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

// ── MCP Server ───────────────────────────────────────────────

/// Start an MCP stdio server that exposes Sisyphus agent tools.
/// Runs until stdin closes.
pub async fn run_serve(client: Arc<dyn sisyphus_tools::metis::LlmClient>) -> Result<()> {
    LLM_CLIENT.set(client).map_err(|_| anyhow::anyhow!("LLM client already initialized"))?;

    let stdin = tokio::io::stdin();
    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();

    while let Some(line) = lines.next_line().await? {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let request: sisyphus_tools::JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                // Parse error — must respond even if we can't parse the id
                let resp = sisyphus_tools::JsonRpcResponse {
                    jsonrpc: "2.0".into(),
                    id: None,
                    result: None,
                    error: Some(sisyphus_tools::JsonRpcError {
                        code: sisyphus_tools::MCP_ERROR_PARSE,
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

        let response = handle_request(request).await;

        let out = serde_json::to_string(&response)?;
        println!("{out}");
        std::io::stdout().flush()?;
    }

    Ok(())
}

/// Dispatch a JSON-RPC request to the appropriate MCP method handler.
async fn handle_request(req: sisyphus_tools::JsonRpcRequest) -> sisyphus_tools::JsonRpcResponse {
    match req.method.as_str() {
        // ── Lifecycle ──
        "initialize" => handle_initialize(&req),

        "notifications/initialized" | "notifications/cancelled" => {
            // Notifications have no response — but MCP sends them as requests,
            // so we respond with an empty ack.
            sisyphus_tools::JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: None,
                result: None,
                error: None,
            }
        }

        // ── Tools ──
        "tools/list" => handle_tools_list(&req),

        "tools/call" => handle_tools_call(req).await,

        // ── Unknown ──
        _ => jsonrpc_error(
            req.id,
            sisyphus_tools::MCP_ERROR_METHOD_NOT_FOUND,
            format!("Method not found: {}", req.method),
        ),
    }
}

fn handle_initialize(req: &sisyphus_tools::JsonRpcRequest) -> sisyphus_tools::JsonRpcResponse {
    let result = sisyphus_tools::InitializeResult {
        protocol_version: "2024-11-05".into(),
        capabilities: sisyphus_tools::ServerCapabilities {
            tools: Some(sisyphus_tools::ToolsCapability {
                list_changed: None,
            }),
            resources: None,
        },
        server_info: Some(sisyphus_tools::ServerInfo {
            name: "sisyphus-tools".into(),
            version: "0.1.0".into(),
        }),
    };

    sisyphus_tools::JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id: req.id,
        result: Some(serde_json::to_value(result).unwrap_or_default()),
        error: None,
    }
}

fn handle_tools_list(req: &sisyphus_tools::JsonRpcRequest) -> sisyphus_tools::JsonRpcResponse {
    let tools: Vec<sisyphus_tools::McpTool> = tool_registry()
        .iter()
        .map(|t| sisyphus_tools::McpTool {
            name: t.name.to_string(),
            description: Some(t.description.to_string()),
            input_schema: Some(t.input_schema.clone()),
        })
        .collect();

    sisyphus_tools::JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id: req.id,
        result: Some(serde_json::json!({ "tools": tools })),
        error: None,
    }
}

async fn handle_tools_call(req: sisyphus_tools::JsonRpcRequest) -> sisyphus_tools::JsonRpcResponse {
    let params: sisyphus_tools::CallToolParams = match req
        .params
        .as_ref()
        .map(|p| serde_json::from_value(p.clone()))
        .transpose()
    {
        Ok(Some(p)) => p,
        Ok(None) => {
            return jsonrpc_error(
                req.id,
                sisyphus_tools::MCP_ERROR_INVALID_PARAMS,
                "Missing tool call parameters".into(),
            );
        }
        Err(e) => {
            return jsonrpc_error(
                req.id,
                sisyphus_tools::MCP_ERROR_INVALID_PARAMS,
                format!("Invalid tool call params: {e}"),
            );
        }
    };

    let args = params.arguments.unwrap_or(serde_json::Value::Object(Default::default()));

    let result = match params.name.as_str() {
        "metis_analyse" => execute_metis_analyse(args).await,
        "momus_review_plan" => execute_momus_review_plan(args).await,
        "momus_review_code" => execute_momus_review_code(args).await,
        "momus_review_architecture" => execute_momus_review_arch(args).await,
        "librarian_research" => execute_librarian_research(args).await,
        "librarian_fetch_docs" => execute_librarian_fetch_docs(args).await,
        "librarian_find_examples" => execute_librarian_find_examples(args).await,
        "librarian_synthesise" => execute_librarian_synthesise(args).await,
        _ => {
            return jsonrpc_error(
                req.id,
                sisyphus_tools::MCP_ERROR_TOOL_NOT_FOUND,
                format!("Tool not found: {}", params.name),
            );
        }
    };

    match result {
        Ok(output) => sisyphus_tools::JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: req.id,
            result: Some(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": output
                }],
                "isError": false
            })),
            error: None,
        },
        Err(err_msg) => sisyphus_tools::JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: req.id,
            result: None,
            error: Some(sisyphus_tools::JsonRpcError {
                code: sisyphus_tools::MCP_ERROR_TOOL_EXECUTION,
                message: err_msg,
                data: None,
            }),
        },
    }
}

// ── Tool Execution ────────────────────────────────────────────

fn get_str<'a>(args: &'a serde_json::Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("Missing required argument: '{key}'"))
}

fn get_str_opt<'a>(args: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|v| v.as_str()).filter(|s| !s.is_empty())
}

fn get_usize(args: &serde_json::Value, key: &str, default: usize) -> usize {
    args.get(key)
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(default)
}

fn output_text(text: String) -> Result<String, String> {
    Ok(text)
}

fn output_json<T: serde::Serialize>(value: T) -> Result<String, String> {
    serde_json::to_string_pretty(&value).map_err(|e| format!("Serialization error: {e}"))
}

async fn execute_metis_analyse(args: serde_json::Value) -> Result<String, String> {
    let client = LLM_CLIENT.get().ok_or("LLM client not initialized")?.clone();
    let request = get_str(&args, "request")?;
    let analysis = sisyphus_tools::metis::analyse(request, &*client)
        .await
        .map_err(|e| format!("Analysis error: {e}"))?;
    output_json(&analysis)
}

async fn execute_momus_review_plan(args: serde_json::Value) -> Result<String, String> {
    let client = LLM_CLIENT.get().ok_or("LLM client not initialized")?;
    let plan_text = get_str(&args, "plan_text")?;
    let plan_name = get_str(&args, "plan_name")?;
    let review = sisyphus_tools::review_plan(&**client, plan_text, plan_name).await;
    output_json(&review)
}

async fn execute_momus_review_code(args: serde_json::Value) -> Result<String, String> {
    let client = LLM_CLIENT.get().ok_or("LLM client not initialized")?;
    let path = get_str(&args, "path")?;
    let content = get_str(&args, "content")?;
    let review = sisyphus_tools::review_code(&**client, Path::new(path), content)
        .await
        .map_err(|e| format!("Code review error: {e}"))?;
    output_json(&review)
}

async fn execute_momus_review_arch(args: serde_json::Value) -> Result<String, String> {
    let client = LLM_CLIENT.get().ok_or("LLM client not initialized")?;
    let arch_text = get_str(&args, "arch_text")?;
    let review = sisyphus_tools::review_architecture(&**client, arch_text).await;
    output_json(&review)
}

async fn execute_librarian_research(args: serde_json::Value) -> Result<String, String> {
    let query = get_str(&args, "query")?;
    let scope_str = get_str_opt(&args, "scope").unwrap_or("web");
    let max_results = get_usize(&args, "max_results", 5).clamp(1, 20);

    let scope = match scope_str {
        "docs" | "documentation" => sisyphus_tools::ResearchScope::Documentation,
        "code" | "examples" => sisyphus_tools::ResearchScope::CodeExamples,
        "all" => sisyphus_tools::ResearchScope::All,
        _ => sisyphus_tools::ResearchScope::Web,
    };

    let report = sisyphus_tools::research(query, scope, max_results)
        .await
        .map_err(|e| format!("Research error: {e}"))?;
    output_json(&report)
}

async fn execute_librarian_fetch_docs(args: serde_json::Value) -> Result<String, String> {
    let url = get_str(&args, "url")?;
    let page = sisyphus_tools::fetch_documentation(url)
        .await
        .map_err(|e| format!("Fetch error: {e}"))?;
    output_json(&page)
}

async fn execute_librarian_find_examples(args: serde_json::Value) -> Result<String, String> {
    let query = get_str(&args, "query")?;
    let language = get_str_opt(&args, "language").unwrap_or("rust");
    let examples = sisyphus_tools::find_examples(query, language)
        .await
        .map_err(|e| format!("Find examples error: {e}"))?;
    output_json(&examples)
}

async fn execute_librarian_synthesise(args: serde_json::Value) -> Result<String, String> {
    let client = LLM_CLIENT.get().ok_or("LLM client not initialized")?;
    let findings_json = get_str(&args, "findings_json")?;

    let findings: Vec<sisyphus_tools::ResearchFinding> =
        serde_json::from_str(findings_json).map_err(|e| format!("Invalid findings JSON: {e}"))?;

    let synthesis = sisyphus_tools::synthesise(&findings, &**client).await?;
    output_text(synthesis)
}

// ── Error Helper ──────────────────────────────────────────────

fn jsonrpc_error(
    id: Option<u64>,
    code: i64,
    message: String,
) -> sisyphus_tools::JsonRpcResponse {
    sisyphus_tools::JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id,
        result: None,
        error: Some(sisyphus_tools::JsonRpcError {
            code,
            message,
            data: None,
        }),
    }
}

// ── MCP Client (existing) ─────────────────────────────────────

/// Helper: connect via stdio transport.
async fn connect_stdio(command: &str, args: &[String]) -> Result<sisyphus_tools::McpClient> {
    let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let transport = sisyphus_tools::StdioTransport::spawn(command, &args_refs)
        .await
        .context("Failed to spawn MCP server process")?;
    let mut client = sisyphus_tools::McpClient::new(Box::new(transport));
    client
        .initialize("sisyphus-cli", "0.1.0")
        .await
        .context("MCP initialize failed")?;
    Ok(client)
}

/// Helper: connect via HTTP transport.
async fn connect_http(url: &str) -> Result<sisyphus_tools::McpClient> {
    let transport = sisyphus_tools::HttpTransport::new(url);
    let mut client = sisyphus_tools::McpClient::new(Box::new(transport));
    client
        .initialize("sisyphus-cli", "0.1.0")
        .await
        .context("MCP initialize failed")?;
    Ok(client)
}

/// Resolve connection args → client.
/// If first arg looks like `http://` or `https://`, uses HTTP transport.
/// Otherwise treats args as `<command> [args...]` for stdio transport.
async fn resolve_client(args: &[String]) -> Result<sisyphus_tools::McpClient> {
    if args.is_empty() {
        anyhow::bail!("Provide either <url> for HTTP or <command> [args...] for stdio");
    }
    let first = &args[0];
    if first.starts_with("http://") || first.starts_with("https://") {
        connect_http(first).await
    } else {
        connect_stdio(first, &args[1..]).await
    }
}

/// `sisyphus mcp info <url-or-command...>` — show server info + tools.
pub async fn run_info(connect_args: &[String]) -> Result<()> {
    let mut client = resolve_client(connect_args).await?;

    let info = client
        .server_info()
        .cloned()
        .unwrap_or(sisyphus_tools::ServerInfo {
            name: "unknown".into(),
            version: "unknown".into(),
        });
    println!("Connected to MCP server: {} v{}", info.name, info.version);

    let tools = client.list_tools().await?;
    println!("Available tools ({}):", tools.len());
    for tool in &tools {
        let schema = tool
            .input_schema
            .as_ref()
            .and_then(|v| serde_json::to_string(v).ok())
            .unwrap_or_default();
        println!("  {} — {}", tool.name, tool.description.as_deref().unwrap_or(""));
        if !schema.is_empty() {
            println!("    schema: {schema}");
        }
    }
    Ok(())
}

/// `sisyphus mcp call <tool-name> [args-json] -- <url-or-command...>` — call a tool.
pub async fn run_call(
    tool_name: &str,
    args_json: Option<&str>,
    connect_args: &[String],
) -> Result<()> {
    let mut client = resolve_client(connect_args).await?;

    let arguments = args_json
        .map(serde_json::from_str)
        .transpose()
        .context("Invalid JSON arguments — must be a valid JSON object")?;

    let result = client.call_tool(tool_name, arguments).await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
