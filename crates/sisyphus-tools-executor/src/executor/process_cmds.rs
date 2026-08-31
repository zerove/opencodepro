use crate::executor::args::{ok_json, parse_args, str_arg, u64_arg};
use metrics::counter;
use serde_json::json;
use std::path::Path;

// ── bash & web command implementations ────────────────────────

/// System command: execute a shell command in an optional working directory.
///
/// Args (JSON):
/// - `command` (required): shell command string
/// - `workdir` (optional): working directory
/// - `timeout_ms` (default 30000): execution timeout in milliseconds
///
/// Returns JSON `{ stdout, stderr, exit_code, duration_ms }`.
///
/// **Security**: command is run via `bash -c`. Callers must validate
/// `workdir` to be inside the project root, otherwise arbitrary command
/// execution is possible.
pub async fn cmd_bash_exec(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let command = str_arg(&v, "command")?;
    let workdir = crate::executor::args::opt_str_arg(&v, "workdir");
    let timeout_ms = u64_arg(&v, "timeout_ms").unwrap_or(30000);
    let workdir_ref = workdir.as_ref().map(|s| Path::new(s.as_str()));
    let output = sisyphus_tools_system::bash::execute(&command, workdir_ref, timeout_ms)
        .await
        .map_err(|e| e.to_string())?;
    counter!(
        "sisyphus_executor_bash_total",
        "exit_code" => output.exit_code.to_string(),
    )
    .increment(1);
    ok_json(json!({
        "stdout": output.stdout,
        "stderr": output.stderr,
        "exit_code": output.exit_code,
        "duration_ms": output.duration.as_millis(),
    }))
}

/// System command: search the web via the configured search backend.
///
/// Args (JSON):
/// - `query` (required): search query string
/// - `num` (default 5): maximum number of results
///
/// Returns JSON `{ results: [{ title, url, snippet }, ...] }`.
pub async fn cmd_web_search(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let query = str_arg(&v, "query")?;
    let num = crate::executor::args::usize_arg(&v, "num").unwrap_or(5);
    let results =
        sisyphus_tools_system::web::websearch(&query, num).await.map_err(|e| e.to_string())?;
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

/// System command: fetch a single URL and return its body as text.
///
/// Args (JSON):
/// - `url` (required): target URL
///
/// Returns JSON `{ content, content_type }`. Large responses may be
/// truncated by the underlying HTTP client.
pub async fn cmd_web_fetch(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let url = str_arg(&v, "url")?;
    let result = sisyphus_tools_system::web::webfetch(&url).await.map_err(|e| e.to_string())?;
    ok_json(json!({
        "content": result.content,
        "content_type": result.content_type,
    }))
}
