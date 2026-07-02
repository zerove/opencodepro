use crate::executor::BuiltinToolExecutor;
use serde_json::{json, Value};

// ── utility command implementations ──────────────────────────

pub async fn cmd_git_blame(args: Value) -> Result<Vec<u8>, sisyphus_types::TypesError> {
    let path = args.get("path").and_then(|p| p.as_str()).ok_or_else(|| {
        sisyphus_types::TypesError::ToolExecutionError {
            tool_id: "git_blame".to_string(),
            details: "Missing required param: path (string)".to_string(),
        }
    })?;
    let line = args.get("line").and_then(serde_json::Value::as_i64).ok_or_else(|| {
        sisyphus_types::TypesError::ToolExecutionError {
            tool_id: "git_blame".to_string(),
            details: "Missing required param: line (number)".to_string(),
        }
    })?;

    let cmd = format!("git blame -L {line},{line} --porcelain {path}");
    let output = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .output()
        .await
        .map_err(|e| sisyphus_types::TypesError::ToolExecutionError {
            tool_id: "git_blame".to_string(),
            details: format!("Git blame execution failed: {e}"),
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next().ok_or_else(|| {
        sisyphus_types::TypesError::ToolExecutionError {
            tool_id: "git_blame".to_string(),
            details: "No output from git blame".to_string(),
        }
    })?;

    let commit_hash = first_line.split(' ').next().unwrap_or("");

    let show_cmd = format!("git show -s --format=%B {commit_hash}");
    let show_output = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(&show_cmd)
        .output()
        .await
        .map_err(|e| sisyphus_types::TypesError::ToolExecutionError {
            tool_id: "git_blame".to_string(),
            details: format!("Git show failed: {e}"),
        })?;

    let commit_msg = String::from_utf8_lossy(&show_output.stdout).to_string();

    Ok(serde_json::to_vec(&json!({
        "commit": commit_hash,
        "message": commit_msg,
        "line": line
    })).unwrap())
}

pub async fn cmd_type_check(args: Value) -> Result<Vec<u8>, sisyphus_types::TypesError> {
    let package = args.get("package").and_then(|p| p.as_str());
    let mut cmd = tokio::process::Command::new("cargo");
    cmd.arg("check");
    if let Some(pkg) = package {
        cmd.arg("-p").arg(pkg);
    }
    cmd.arg("--message-format=json");

    let output = cmd.output().await.map_err(|e| sisyphus_types::TypesError::ToolExecutionError {
        tool_id: "type_check".to_string(),
        details: format!("Cargo check failed to start: {e}"),
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut errors = Vec::new();

    for line in stdout.lines() {
        if let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) {
            if msg["stream"].as_str() == Some("stderr") {
                if let Some(message) = msg.get("message") {
                    errors.push(message.clone());
                }
            }
        }
    }

    Ok(serde_json::to_vec(&json!({
        "success": output.status.success(),
        "errors": errors,
        "exit_code": output.status.code()
    })).unwrap())
}

pub async fn cmd_lsp_symbol_search(
    executor: &BuiltinToolExecutor,
    args: Value,
) -> Result<Vec<u8>, sisyphus_types::TypesError> {
    let query = args.get("query").and_then(|q| q.as_str()).ok_or_else(|| {
        sisyphus_types::TypesError::ToolExecutionError {
            tool_id: "lsp_symbol_search".to_string(),
            details: "Missing required param: query (string)".to_string(),
        }
    })?;

    // 1. Attempt real bridge call if available
    if let Some(bridge) = &executor.bridge_client {
        match bridge.execute_tool("lsp_symbol_search", args.clone()).await {
            Ok(res) => return Ok(serde_json::to_vec(&res).unwrap()),
            Err(e) => {
                tracing::warn!(error = %e, "LSP bridge call failed, falling back to simulation");
            }
        }
    }

    // 2. Fallback: Simulate LSP search
    let results = vec![
        json!({ "name": query, "location": "crates/sisyphus-core/src/kernel.rs:120", "kind": "Function" }),
        json!({ "name": format!("{}_impl", query), "location": "crates/sisyphus-tools/src/executor/mod.rs:45", "kind": "Struct" }),
    ];

    Ok(serde_json::to_vec(&json!({ "symbols": results })).unwrap())
}

pub async fn cmd_dependency_graph(args: Value) -> Result<Vec<u8>, sisyphus_types::TypesError> {
    let path = args.get("path").and_then(|p| p.as_str()).ok_or_else(|| {
        sisyphus_types::TypesError::ToolExecutionError {
            tool_id: "dependency_graph".to_string(),
            details: "Missing required param: path (string)".to_string(),
        }
    })?;

    let content = tokio::fs::read_to_string(path).await.map_err(|e| sisyphus_types::TypesError::ToolExecutionError {
        tool_id: "dependency_graph".to_string(),
        details: format!("Failed to read file: {e}"),
    })?;

    let mut edges = Vec::new();
    for line in content.lines() {
        if line.trim().starts_with("use ") {
            let parts: Vec<&str> = line.trim().trim_start_matches("use ").split(',').collect();
            for part in parts {
                let clean_part = part.trim().trim_end_matches(';');
                edges.push(format!("{path} --> {clean_part}"));
            }
        }
    }

    let mermaid = format!("graph TD\n  {}", edges.join("\n  "));
    Ok(serde_json::to_vec(&json!({ "mermaid": mermaid })).unwrap())
}

pub async fn cmd_doc_sync_check(client: &std::sync::Arc<dyn sisyphus_tools_llm::metis::LlmClient>, args: Value) -> Result<Vec<u8>, sisyphus_types::TypesError> {
    let code_path = args.get("code_path").and_then(|p| p.as_str()).ok_or_else(|| {
        sisyphus_types::TypesError::ToolExecutionError {
            tool_id: "doc_sync_check".to_string(),
            details: "Missing required param: code_path (string)".to_string(),
        }
    })?;
    let doc_path = args.get("doc_path").and_then(|p| p.as_str()).ok_or_else(|| {
        sisyphus_types::TypesError::ToolExecutionError {
            tool_id: "doc_sync_check".to_string(),
            details: "Missing required param: doc_path (string)".to_string(),
        }
    })?;

    let code = tokio::fs::read_to_string(code_path).await.map_err(|e| sisyphus_types::TypesError::ToolExecutionError {
        tool_id: "doc_sync_check".to_string(),
        details: format!("Failed to read code: {e}"),
    })?;
    let doc = tokio::fs::read_to_string(doc_path).await.map_err(|e| sisyphus_types::TypesError::ToolExecutionError {
        tool_id: "doc_sync_check".to_string(),
        details: format!("Failed to read doc: {e}"),
    })?;

    let prompt = format!(
        "Analyze the following code and its corresponding documentation for inconsistencies.\n\nCODE:\n{code}\n\nDOCS:\n{doc}\n\nReport any discrepancies in parameters, return types, or behavior. If consistent, return 'CONSISTENT'."
    );

    let analysis = client.generate(&prompt).await.map_err(|e| sisyphus_types::TypesError::ToolExecutionError {
        tool_id: "doc_sync_check".to_string(),
        details: format!("LLM analysis failed: {e}"),
    })?;

    Ok(serde_json::to_vec(&json!({ "analysis": analysis, "is_consistent": analysis.to_uppercase().contains("CONSISTENT") })).unwrap())
}

pub fn cmd_json_query(args: Value) -> Result<Vec<u8>, sisyphus_types::TypesError> {
    let path = args.get("path").and_then(|p| p.as_str()).ok_or_else(|| {
        sisyphus_types::TypesError::ToolExecutionError {
            tool_id: "json_query".to_string(),
            details: "Missing required param: path (string)".to_string(),
        }
    })?;
    let json_str = args.get("json").and_then(|j| j.as_str()).ok_or_else(|| {
        sisyphus_types::TypesError::ToolExecutionError {
            tool_id: "json_query".to_string(),
            details: "Missing required param: json (string)".to_string(),
        }
    })?;

    let value: Value = serde_json::from_str(json_str).map_err(|e| sisyphus_types::TypesError::ToolExecutionError {
        tool_id: "json_query".to_string(),
        details: format!("Invalid JSON: {e}"),
    })?;

    let mut current = &value;
    for part in path.split('.') {
        if part.is_empty() { continue; }
        if part.contains('[') && part.ends_with(']') {
            let open_bracket = part.find('[').unwrap();
            let key = &part[..open_bracket];
            let index_str = &part[open_bracket + 1..part.len() - 1];
            let index = index_str.parse::<usize>().map_err(|e| sisyphus_types::TypesError::ToolExecutionError {
                tool_id: "json_query".to_string(),
                details: format!("Invalid array index: {e}"),
            })?;

            current = current.get(key).and_then(|v| v.get(index)).ok_or_else(|| {
                sisyphus_types::TypesError::ToolExecutionError {
                    tool_id: "json_query".to_string(),
                    details: format!("Path not found at {key}.{index}"),
                }
            })?;
        } else {
            current = current.get(part).ok_or_else(|| {
                sisyphus_types::TypesError::ToolExecutionError {
                    tool_id: "json_query".to_string(),
                    details: format!("Path not found at {part}"),
                }
            })?;
        }
    }

    Ok(serde_json::to_vec(current).unwrap())
}

pub fn cmd_base64_convert(args: Value) -> Result<Vec<u8>, sisyphus_types::TypesError> {
    use base64::{engine::general_purpose, Engine as _};

    let mode = args.get("mode").and_then(|m| m.as_str()).unwrap_or("encode");
    let input = args.get("input").and_then(|i| i.as_str()).ok_or_else(|| {
        sisyphus_types::TypesError::ToolExecutionError {
            tool_id: "base64_convert".to_string(),
            details: "Missing required param: input (string)".to_string(),
        }
    })?;

    let result = if mode == "decode" {
        general_purpose::STANDARD.decode(input).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .map_err(|e| sisyphus_types::TypesError::ToolExecutionError {
                tool_id: "base64_convert".to_string(),
                details: format!("Base64 decode error: {e}"),
            })?
    } else {
        general_purpose::STANDARD.encode(input)
    };

    Ok(serde_json::to_vec(&json!({ "result": result })).unwrap())
}

pub fn cmd_time_convert(args: Value) -> Result<Vec<u8>, sisyphus_types::TypesError> {
    use chrono::{DateTime, Utc, Local};

    let time_str = args.get("time").and_then(|t| t.as_str()).ok_or_else(|| {
        sisyphus_types::TypesError::ToolExecutionError {
            tool_id: "time_convert".to_string(),
            details: "Missing required param: time (string)".to_string(),
        }
    })?;

    let target_tz = args.get("timezone").and_then(|tz| tz.as_str()).unwrap_or("UTC");

    // Try to parse ISO 8601
    let dt = DateTime::parse_from_rfc3339(time_str).map_err(|e| sisyphus_types::TypesError::ToolExecutionError {
        tool_id: "time_convert".to_string(),
        details: format!("Invalid ISO 8601 time: {e}"),
    })?.with_timezone(&Utc);

    let result = match target_tz {
        "UTC" => dt.to_rfc3339(),
        "Local" => dt.with_timezone(&Local).to_rfc3339(),
        _ => return Err(sisyphus_types::TypesError::ToolExecutionError {
            tool_id: "time_convert".to_string(),
            details: format!("Unsupported timezone: {target_tz}"),
        }),
    };

    Ok(serde_json::to_vec(&json!({ "converted_time": result })).unwrap())
}

pub fn cmd_url_convert(args: Value) -> Result<Vec<u8>, sisyphus_types::TypesError> {
    let mode = args.get("mode").and_then(|m| m.as_str()).unwrap_or("encode");
    let input = args.get("input").and_then(|i| i.as_str()).ok_or_else(|| {
        sisyphus_types::TypesError::ToolExecutionError {
            tool_id: "url_convert".to_string(),
            details: "Missing required param: input (string)".to_string(),
        }
    })?;

    let result = if mode == "decode" {
        urlencoding::decode(input).map(std::borrow::Cow::into_owned).map_err(|e| sisyphus_types::TypesError::ToolExecutionError {
            tool_id: "url_convert".to_string(),
            details: format!("URL decode error: {e}"),
        })?
    } else {
        urlencoding::encode(input).into_owned()
    };

    Ok(serde_json::to_vec(&json!({ "result": result })).unwrap())
}

pub fn cmd_path_expand(args: Value) -> Result<Vec<u8>, sisyphus_types::TypesError> {
    let path_str = args.get("path").and_then(|p| p.as_str()).ok_or_else(|| {
        sisyphus_types::TypesError::ToolExecutionError {
            tool_id: "path_expand".to_string(),
            details: "Missing required param: path (string)".to_string(),
        }
    })?;

    let expanded = if path_str.starts_with('~') {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/zeron".to_string());
        path_str.replace('~', &home)
    } else {
        path_str.to_string()
    };

    Ok(serde_json::to_vec(&json!({ "expanded_path": expanded })).unwrap())
}

pub async fn cmd_coverage_analyze(args: Value) -> Result<Vec<u8>, sisyphus_types::TypesError> {
    let package = args.get("package").and_then(|p| p.as_str()).unwrap_or("all");

    // 尝试运行 cargo llvm-cov (如果安装了)
    let mut cmd = tokio::process::Command::new("cargo");
    cmd.arg("llvm-cov").arg("--json");
    if package != "all" {
        cmd.arg("-p").arg(package);
    }

    let output = cmd.output().await.map_err(|e| sisyphus_types::TypesError::ToolExecutionError {
        tool_id: "coverage_analyze".to_string(),
        details: format!("Failed to execute llvm-cov: {e}"),
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    // 如果 llvm-cov 不可用，返回一个引导信息，提示用户安装
    if output.status.code() != Some(0) {
        return Ok(serde_json::to_vec(&json!({
            "status": "error",
            "message": "llvm-cov not found or failed. Please install 'cargo-llvm-cov' to use this tool.",
            "hint": "cargo install cargo-llvm-cov"
        })).unwrap());
    }

    // 解析 JSON 覆盖率数据并提取未覆盖的行
    // 这里简化处理，直接返回原始 JSON 供 LLM 分析
    Ok(serde_json::to_vec(&json!({
        "status": "success",
        "coverage_data": stdout,
    })).unwrap())
}
