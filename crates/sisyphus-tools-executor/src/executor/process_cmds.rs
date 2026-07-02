use crate::executor::args::{ok_json, parse_args, str_arg, u64_arg};
use serde_json::json;
use std::path::Path;

// ── bash & web command implementations ────────────────────────

pub async fn cmd_bash_exec(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let command = str_arg(&v, "command")?;
    let workdir = crate::executor::args::opt_str_arg(&v, "workdir");
    let timeout_ms = u64_arg(&v, "timeout_ms").unwrap_or(30000);
    let workdir_ref = workdir.as_ref().map(|s| Path::new(s.as_str()));
    let output = sisyphus_tools_system::bash::execute(&command, workdir_ref, timeout_ms)
        .await
        .map_err(|e| e.to_string())?;
    ok_json(json!({
        "stdout": output.stdout,
        "stderr": output.stderr,
        "exit_code": output.exit_code,
        "duration_ms": output.duration.as_millis(),
    }))
}

pub async fn cmd_web_search(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let query = str_arg(&v, "query")?;
    let num = crate::executor::args::usize_arg(&v, "num").unwrap_or(5);
    let results = sisyphus_tools_system::web::websearch(&query, num)
        .await
        .map_err(|e| e.to_string())?;
    let list: Vec<serde_json::Value> = results
        .iter()
        .map(|r| {
            json!({
                "title": r.title,
                "url": r.url,
                "snippet": r.snippet,
            })
        })
        .collect();
    ok_json(json!({"results": list}))
}

pub async fn cmd_web_fetch(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let url = str_arg(&v, "url")?;
    let result = sisyphus_tools_system::web::webfetch(&url)
        .await
        .map_err(|e| e.to_string())?;
    ok_json(json!({
        "content": result.content,
        "content_type": result.content_type,
    }))
}
