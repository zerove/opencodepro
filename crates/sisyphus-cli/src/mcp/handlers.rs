use std::path::Path;

use super::{tool_registry, LLM_CLIENT};

/// Dispatch a JSON-RPC request to the appropriate MCP method handler.
pub(super) async fn handle_request(
    req: crate::tools::JsonRpcRequest,
) -> crate::tools::JsonRpcResponse {
    match req.method.as_str() {
        // ── Lifecycle ──
        "initialize" => handle_initialize(&req),

        "notifications/initialized" | "notifications/cancelled" => crate::tools::JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: None,
            result: None,
            error: None,
        },

        // ── Tools ──
        "tools/list" => handle_tools_list(&req),

        "tools/call" => handle_tools_call(req).await,

        // ── Unknown ──
        _ => jsonrpc_error(
            req.id,
            crate::tools::MCP_ERROR_METHOD_NOT_FOUND,
            format!("Method not found: {}", req.method),
        ),
    }
}

fn handle_initialize(req: &crate::tools::JsonRpcRequest) -> crate::tools::JsonRpcResponse {
    let result = crate::tools::InitializeResult {
        protocol_version: "2024-11-05".into(),
        capabilities: crate::tools::ServerCapabilities {
            tools: Some(crate::tools::ToolsCapability { list_changed: None }),
            resources: None,
        },
        server_info: Some(crate::tools::ServerInfo {
            name: "sisyphus-tools".into(),
            version: "0.1.0".into(),
        }),
    };

    crate::tools::JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id: req.id,
        result: Some(serde_json::to_value(result).unwrap_or_default()),
        error: None,
    }
}

fn handle_tools_list(req: &crate::tools::JsonRpcRequest) -> crate::tools::JsonRpcResponse {
    let tools: Vec<crate::tools::McpTool> = tool_registry()
        .iter()
        .map(|t| crate::tools::McpTool {
            name: t.name.to_string(),
            description: Some(t.description.to_string()),
            input_schema: Some(t.input_schema.clone()),
        })
        .collect();

    crate::tools::JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id: req.id,
        result: Some(serde_json::json!({ "tools": tools })),
        error: None,
    }
}

async fn handle_tools_call(
    req: crate::tools::JsonRpcRequest,
) -> crate::tools::JsonRpcResponse {
    let params: crate::tools::CallToolParams = match req
        .params
        .as_ref()
        .map(|p| serde_json::from_value(p.clone()))
        .transpose()
    {
        Ok(Some(p)) => p,
        Ok(None) => {
            return jsonrpc_error(
                req.id,
                crate::tools::MCP_ERROR_INVALID_PARAMS,
                "Missing tool call parameters".into(),
            );
        }
        Err(e) => {
            return jsonrpc_error(
                req.id,
                crate::tools::MCP_ERROR_INVALID_PARAMS,
                format!("Invalid tool call params: {e}"),
            );
        }
    };

    let args = params
        .arguments
        .unwrap_or(serde_json::Value::Object(Default::default()));

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
                crate::tools::MCP_ERROR_TOOL_NOT_FOUND,
                format!("Tool not found: {}", params.name),
            );
        }
    };

    match result {
        Ok(output) => crate::tools::JsonRpcResponse {
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
        Err(err_msg) => crate::tools::JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: req.id,
            result: None,
            error: Some(crate::tools::JsonRpcError {
                code: crate::tools::MCP_ERROR_TOOL_EXECUTION,
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
    args.get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
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
    let client = LLM_CLIENT
        .get()
        .ok_or("LLM client not initialized")?
        .clone();
    let request = get_str(&args, "request")?;
    let analysis = crate::tools::metis::analyse(request, &*client)
        .await
        .map_err(|e| format!("Analysis error: {e}"))?;
    output_json(&analysis)
}

async fn execute_momus_review_plan(args: serde_json::Value) -> Result<String, String> {
    let client = LLM_CLIENT.get().ok_or("LLM client not initialized")?;
    let plan_text = get_str(&args, "plan_text")?;
    let plan_name = get_str(&args, "plan_name")?;
    let review = crate::tools::review_plan(&**client, plan_text, plan_name).await;
    output_json(&review)
}

async fn execute_momus_review_code(args: serde_json::Value) -> Result<String, String> {
    let client = LLM_CLIENT.get().ok_or("LLM client not initialized")?;
    let path = get_str(&args, "path")?;
    let content = get_str(&args, "content")?;
    let review = crate::tools::review_code(&**client, Path::new(path), content)
        .await
        .map_err(|e| format!("Code review error: {e}"))?;
    output_json(&review)
}

async fn execute_momus_review_arch(args: serde_json::Value) -> Result<String, String> {
    let client = LLM_CLIENT.get().ok_or("LLM client not initialized")?;
    let arch_text = get_str(&args, "arch_text")?;
    let review = crate::tools::review_architecture(&**client, arch_text).await;
    output_json(&review)
}

async fn execute_librarian_research(args: serde_json::Value) -> Result<String, String> {
    let query = get_str(&args, "query")?;
    let scope_str = get_str_opt(&args, "scope").unwrap_or("web");
    let max_results = get_usize(&args, "max_results", 5).clamp(1, 20);

    let scope = match scope_str {
        "docs" | "documentation" => crate::tools::ResearchScope::Documentation,
        "code" | "examples" => crate::tools::ResearchScope::CodeExamples,
        "all" => crate::tools::ResearchScope::All,
        _ => crate::tools::ResearchScope::Web,
    };

    let report = crate::tools::research(query, scope, max_results)
        .await
        .map_err(|e| format!("Research error: {e}"))?;
    output_json(&report)
}

async fn execute_librarian_fetch_docs(args: serde_json::Value) -> Result<String, String> {
    let url = get_str(&args, "url")?;
    let page = crate::tools::fetch_documentation(url)
        .await
        .map_err(|e| format!("Fetch error: {e}"))?;
    output_json(&page)
}

async fn execute_librarian_find_examples(args: serde_json::Value) -> Result<String, String> {
    let query = get_str(&args, "query")?;
    let language = get_str_opt(&args, "language").unwrap_or("rust");
    let examples = crate::tools::find_examples(query, language)
        .await
        .map_err(|e| format!("Find examples error: {e}"))?;
    output_json(&examples)
}

async fn execute_librarian_synthesise(args: serde_json::Value) -> Result<String, String> {
    let client = LLM_CLIENT.get().ok_or("LLM client not initialized")?;
    let findings_json = get_str(&args, "findings_json")?;

    let findings: Vec<crate::tools::ResearchFinding> =
        serde_json::from_str(findings_json).map_err(|e| format!("Invalid findings JSON: {e}"))?;

    let synthesis = crate::tools::synthesise(&findings, &**client).await?;
    output_text(synthesis)
}

// ── Error Helper ──────────────────────────────────────────────

fn jsonrpc_error(id: Option<u64>, code: i64, message: String) -> crate::tools::JsonRpcResponse {
    crate::tools::JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id,
        result: None,
        error: Some(crate::tools::JsonRpcError {
            code,
            message,
            data: None,
        }),
    }
}
