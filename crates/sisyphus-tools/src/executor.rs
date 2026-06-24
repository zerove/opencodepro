use std::path::Path;
use serde_json::{json, Value};

/// A `ToolExecutor` that dispatches tool calls to sisyphus-tools built-in functions.
///
/// Accepts `tool_id` like `"file_read"`, `"bash_exec"`, `"web_search"`, etc.
/// Arguments are JSON-encoded `Vec<u8>` and results are JSON-encoded `Vec<u8>`.
///
/// ## Supported Tool IDs & Schemas
///
/// | Tool ID | Args | Result |
/// |---|---|---|
/// | `file_read` | `{"path": "..."}` | `{"content": "..."}` |
/// | `file_write` | `{"path": "...", "content": "..."}` | `{"bytes_written": N}` |
/// | `file_edit` | `{"path": "...", "old": "...", "new": "..."}` | `{"replaced": true}` |
/// | `file_edit_all` | `{"path": "...", "old": "...", "new": "..."}` | `{"count": N}` |
/// | `file_read_lines` | `{"path": "...", "offset": N, "limit": N}` | `{"content": "..."}` |
/// | `glob` | `{"pattern": "...", "root": "..."}` | `{"files": ["..."]}` |
/// | `grep` | `{"pattern": "...", "path": "...", "include": "..."}` | `{"matches": [...]}` |
/// | `bash_exec` | `{"command": "...", "workdir": "...", "timeout_ms": N}` | `{"stdout": "...", "stderr": "...", "exit_code": N, "duration_ms": N}` |
/// | `web_search` | `{"query": "...", "num": N}` | `{"results": [{"title": "...", "url": "...", "snippet": "..."}]}` |
/// | `web_fetch` | `{"url": "..."}` | `{"content": "...", "content_type": "..."}` |
/// | `librarian/research` | `{"query": "...", "scope": "web|docs|code|all", "num": N}` | Full `ResearchReport` JSON |
/// | `librarian/fetch_docs` | `{"url": "..."}` | Full `DocPage` JSON |
/// | `librarian/examples` | `{"query": "...", "language": "..."}` | `[CodeExample]` JSON array |
/// | `librarian/synthesise` | `{"findings": [...], "max_points": N}` | `{"synthesis": "..."}` |
/// | `metis/analyse` | `{"request": "..."}` | Full `MetisAnalysis` JSON |
/// | `momus/review_plan` | `{"plan_text": "...", "plan_name": "..."}` | Full `MomusReview` JSON |
/// | `momus/review_code` | `{"path": "...", "content": "..."}` | Full `MomusReview` JSON |
/// | `momus/review_code_batch` | `{"files": [{"path": "...", "content": "..."}]}` | `[MomusReview]` JSON array |
/// | `momus/review_arch` | `{"arch_text": "..."}` | Full `MomusReview` JSON |
pub struct BuiltinToolExecutor;

#[async_trait::async_trait]
impl sisyphus_types::ToolExecutor for BuiltinToolExecutor {
    async fn execute(
        &self,
        tool_id: &str,
        args: Vec<u8>,
        _fuel_limit: u64,
    ) -> Result<Vec<u8>, String> {
        match tool_id {
            "file_read" => cmd_file_read(args),
            "file_write" => cmd_file_write(args),
            "file_edit" => cmd_file_edit(args),
            "file_edit_all" => cmd_file_edit_all(args),
            "file_read_lines" => cmd_file_read_lines(args),
            "glob" => cmd_glob(args),
            "grep" => cmd_grep(args),
            "bash_exec" => cmd_bash_exec(args).await,
            "web_search" => cmd_web_search(args).await,
            "web_fetch" => cmd_web_fetch(args).await,
            // ── librarian ──────────────────────────────
            "librarian/research" => cmd_librarian_research(args).await,
            "librarian/fetch_docs" => cmd_librarian_fetch_docs(args).await,
            "librarian/examples" => cmd_librarian_examples(args).await,
            "librarian/synthesise" => cmd_librarian_synthesise(args),
            // ── metis ──────────────────────────────────
            "metis/analyse" => cmd_metis_analyse(args),
            // ── momus ──────────────────────────────────
            "momus/review_plan" => cmd_momus_review_plan(args),
            "momus/review_code" => cmd_momus_review_code(args),
            "momus/review_code_batch" => cmd_momus_review_code_batch(args),
            "momus/review_arch" => cmd_momus_review_arch(args),
            other => Err(format!("unknown builtin tool: {other}")),
        }
    }
}

// ── argument parsing helpers ─────────────────────────────────

fn parse_args(args: &[u8]) -> Result<Value, String> {
    if args.is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_slice(args).map_err(|e| format!("invalid JSON args: {e}"))
}

fn path_arg(v: &Value, name: &str) -> Result<String, String> {
    v.get(name)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("missing or non-string arg '{name}'"))
}

fn str_arg(v: &Value, name: &str) -> Result<String, String> {
    v.get(name)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("missing or non-string arg '{name}'"))
}

fn u64_arg(v: &Value, name: &str) -> Result<u64, String> {
    v.get(name)
        .and_then(|v| v.as_u64())
        .ok_or_else(|| format!("missing or non-integer arg '{name}'"))
}

fn usize_arg(v: &Value, name: &str) -> Result<usize, String> {
    u64_arg(v, name).map(|n| n as usize)
}

fn opt_str_arg(v: &Value, name: &str) -> Option<String> {
    v.get(name).and_then(|v| v.as_str()).map(|s| s.to_string())
}

fn ok_json(val: Value) -> Result<Vec<u8>, String> {
    serde_json::to_vec(&val).map_err(|e| format!("serialize result: {e}"))
}

// ── command implementations ──────────────────────────────────

fn cmd_file_read(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let path = path_arg(&v, "path")?;
    let content = crate::file::read(Path::new(&path)).map_err(|e| e.to_string())?;
    ok_json(json!({"content": content}))
}

fn cmd_file_write(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let path = path_arg(&v, "path")?;
    let content = str_arg(&v, "content")?;
    crate::file::write(Path::new(&path), &content).map_err(|e| e.to_string())?;
    ok_json(json!({"bytes_written": content.len()}))
}

fn cmd_file_edit(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let path = path_arg(&v, "path")?;
    let old = str_arg(&v, "old")?;
    let new = str_arg(&v, "new")?;
    crate::file::edit(Path::new(&path), &old, &new).map_err(|e| e.to_string())?;
    ok_json(json!({"replaced": true}))
}

fn cmd_file_edit_all(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let path = path_arg(&v, "path")?;
    let old = str_arg(&v, "old")?;
    let new = str_arg(&v, "new")?;
    let count = crate::file::edit_all(Path::new(&path), &old, &new).map_err(|e| e.to_string())?;
    ok_json(json!({"count": count}))
}

fn cmd_file_read_lines(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let path = path_arg(&v, "path")?;
    let offset = usize_arg(&v, "offset").unwrap_or(0);
    let limit = usize_arg(&v, "limit").unwrap_or(2000);
    let content = crate::file::read_lines(Path::new(&path), offset, limit).map_err(|e| e.to_string())?;
    ok_json(json!({"content": content}))
}

fn cmd_glob(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let pattern = str_arg(&v, "pattern")?;
    let root = path_arg(&v, "root")?;
    let files = crate::file::glob(&pattern, Path::new(&root)).map_err(|e| e.to_string())?;
    let paths: Vec<String> = files.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    ok_json(json!({"files": paths}))
}

fn cmd_grep(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let pattern = str_arg(&v, "pattern")?;
    let path = path_arg(&v, "path")?;
    let include = opt_str_arg(&v, "include");
    let matches = crate::file::grep(&pattern, Path::new(&path), include.as_deref()).map_err(|e| e.to_string())?;
    let list: Vec<Value> = matches.iter().map(|m| json!({
        "file": m.file.to_string_lossy(),
        "line": m.line,
        "content": m.content,
    })).collect();
    ok_json(json!({"matches": list}))
}

async fn cmd_bash_exec(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let command = str_arg(&v, "command")?;
    let workdir = opt_str_arg(&v, "workdir");
    let timeout_ms = u64_arg(&v, "timeout_ms").unwrap_or(30000);
    let workdir_ref = workdir.as_ref().map(|s| Path::new(s.as_str()));
    let output = crate::bash::execute(&command, workdir_ref, timeout_ms).await.map_err(|e| e.to_string())?;
    ok_json(json!({
        "stdout": output.stdout,
        "stderr": output.stderr,
        "exit_code": output.exit_code,
        "duration_ms": output.duration.as_millis(),
    }))
}

async fn cmd_web_search(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let query = str_arg(&v, "query")?;
    let num = usize_arg(&v, "num").unwrap_or(5);
    let results = crate::web::websearch(&query, num).await.map_err(|e| e.to_string())?;
    let list: Vec<Value> = results.iter().map(|r| json!({
        "title": r.title,
        "url": r.url,
        "snippet": r.snippet,
    })).collect();
    ok_json(json!({"results": list}))
}

async fn cmd_web_fetch(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let url = str_arg(&v, "url")?;
    let result = crate::web::webfetch(&url).await.map_err(|e| e.to_string())?;
    ok_json(json!({
        "content": result.content,
        "content_type": result.content_type,
    }))
}

// ── librarian commands ──────────────────────────────────────

async fn cmd_librarian_research(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let query = str_arg(&v, "query")?;
    let scope_str = str_arg(&v, "scope").unwrap_or_else(|_| "web".into());
    let scope: crate::ResearchScope = scope_str
        .parse()
        .map_err(|e: String| format!("invalid scope: {e}"))?;
    let num = usize_arg(&v, "num").unwrap_or(5);
    let report = crate::research(&query, scope, num).await.map_err(|e| e.to_string())?;
    ok_json(serde_json::to_value(&report).map_err(|e| format!("serialize: {e}"))?)
}

async fn cmd_librarian_fetch_docs(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let url = str_arg(&v, "url")?;
    let page = crate::fetch_documentation(&url).await.map_err(|e| e.to_string())?;
    ok_json(serde_json::to_value(&page).map_err(|e| format!("serialize: {e}"))?)
}

async fn cmd_librarian_examples(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let query = str_arg(&v, "query")?;
    let language = str_arg(&v, "language").unwrap_or_else(|_| "rust".into());
    let examples = crate::find_examples(&query, &language).await.map_err(|e| e.to_string())?;
    ok_json(serde_json::to_value(&examples).map_err(|e| format!("serialize: {e}"))?)
}

fn cmd_librarian_synthesise(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let findings_val = v.get("findings")
        .ok_or_else(|| "missing 'findings' arg".to_string())?;
    let findings: Vec<crate::ResearchFinding> = serde_json::from_value(findings_val.clone())
        .map_err(|e| format!("invalid findings JSON: {e}"))?;
    let max_points = usize_arg(&v, "max_points").unwrap_or(10);
    let synthesis = crate::synthesise(&findings, max_points);
    ok_json(json!({"synthesis": synthesis}))
}

// ── metis commands ──────────────────────────────────────────

fn cmd_metis_analyse(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let request = str_arg(&v, "request")?;
    let analysis = crate::analyse(&request);
    ok_json(serde_json::to_value(&analysis).map_err(|e| format!("serialize: {e}"))?)
}

// ── momus commands ──────────────────────────────────────────

fn cmd_momus_review_plan(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let plan_text = str_arg(&v, "plan_text")?;
    let plan_name = str_arg(&v, "plan_name").unwrap_or_else(|_| "unnamed".into());
    let review = crate::review_plan(&plan_text, &plan_name);
    ok_json(serde_json::to_value(&review).map_err(|e| format!("serialize: {e}"))?)
}

fn cmd_momus_review_code(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let path_str = str_arg(&v, "path")?;
    let content = str_arg(&v, "content")?;
    let path = std::path::Path::new(&path_str);
    let review = crate::review_code(path, &content).map_err(|e| e.to_string())?;
    ok_json(serde_json::to_value(&review).map_err(|e| format!("serialize: {e}"))?)
}

fn cmd_momus_review_code_batch(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let files_val = v.get("files")
        .ok_or_else(|| "missing 'files' arg".to_string())?;
    let files: Vec<serde_json::Value> = serde_json::from_value(files_val.clone())
        .map_err(|e| format!("invalid files JSON: {e}"))?;
    let mut pairs = Vec::new();
    for f in &files {
        let path_str = f.get("path").and_then(|v| v.as_str())
            .ok_or_else(|| "each file entry must have a 'path' string".to_string())?;
        let content = f.get("content").and_then(|v| v.as_str())
            .ok_or_else(|| "each file entry must have a 'content' string".to_string())?;
        pairs.push((std::path::Path::new(path_str), content));
    }
    let reviews = crate::review_code_batch(&pairs).map_err(|e| e.to_string())?;
    ok_json(serde_json::to_value(&reviews).map_err(|e| format!("serialize: {e}"))?)
}

fn cmd_momus_review_arch(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let arch_text = str_arg(&v, "arch_text")?;
    let review = crate::review_architecture(&arch_text);
    ok_json(serde_json::to_value(&review).map_err(|e| format!("serialize: {e}"))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sisyphus_types::ToolExecutor;
    use serde_json::Value;

    fn mkargs(val: Value) -> Vec<u8> {
        serde_json::to_vec(&val).unwrap()
    }

    fn assert_ok(result: Result<Vec<u8>, String>) -> Value {
        let bytes = result.unwrap_or_else(|e| panic!("expected ok: {e}"));
        serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("invalid JSON result: {e}"))
    }

    #[tokio::test]
    async fn test_executor_unknown_tool() {
        let ex = BuiltinToolExecutor;
        let result = ex.execute("no_such_tool", vec![], 0).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown builtin tool"));
    }

    #[tokio::test]
    async fn test_file_read_write_roundtrip() {
        let ex = BuiltinToolExecutor;
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("test.txt");

        // write
        let args = mkargs(json!({"path": path.to_string_lossy(), "content": "hello world"}));
        let result = assert_ok(ex.execute("file_write", args, 0).await);
        assert_eq!(result["bytes_written"], 11);

        // read
        let args = mkargs(json!({"path": path.to_string_lossy()}));
        let result = assert_ok(ex.execute("file_read", args, 0).await);
        assert_eq!(result["content"], "hello world");
    }

    #[tokio::test]
    async fn test_bash_exec() {
        let ex = BuiltinToolExecutor;
        let args = mkargs(json!({"command": "echo hello", "timeout_ms": 5000}));
        let result = assert_ok(ex.execute("bash_exec", args, 0).await);
        assert_eq!(result["stdout"].as_str().unwrap().trim(), "hello");
        assert_eq!(result["exit_code"], 0);
    }

    #[tokio::test]
    async fn test_web_search_executor() {
        let ex = BuiltinToolExecutor;
        let args = mkargs(json!({"query": "test", "num": 1}));
        // Web search may fail in offline environments — that's acceptable
        let result = ex.execute("web_search", args, 0).await;
        if let Ok(bytes) = result {
            let v: Value = serde_json::from_slice(&bytes).unwrap();
            assert!(v["results"].is_array());
        }
        // If it fails, that's also fine (offline test env)
    }

    #[tokio::test]
    async fn test_file_edit() {
        let ex = BuiltinToolExecutor;
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("edit.txt");

        // write initial
        let args = mkargs(json!({"path": path.to_string_lossy(), "content": "foo bar baz"}));
        assert_ok(ex.execute("file_write", args, 0).await);

        // edit
        let args = mkargs(json!({"path": path.to_string_lossy(), "old": "bar", "new": "XXX"}));
        let result = assert_ok(ex.execute("file_edit", args, 0).await);
        assert_eq!(result["replaced"], true);

        // verify
        let args = mkargs(json!({"path": path.to_string_lossy()}));
        let result = assert_ok(ex.execute("file_read", args, 0).await);
        assert_eq!(result["content"], "foo XXX baz");
    }

    #[tokio::test]
    async fn test_file_edit_all() {
        let ex = BuiltinToolExecutor;
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("edit_all.txt");

        let args = mkargs(json!({"path": path.to_string_lossy(), "content": "a b a b a"}));
        assert_ok(ex.execute("file_write", args, 0).await);

        let args = mkargs(json!({"path": path.to_string_lossy(), "old": "a", "new": "X"}));
        let result = assert_ok(ex.execute("file_edit_all", args, 0).await);
        assert_eq!(result["count"], 3);

        let args = mkargs(json!({"path": path.to_string_lossy()}));
        let result = assert_ok(ex.execute("file_read", args, 0).await);
        assert_eq!(result["content"], "X b X b X");
    }

    #[tokio::test]
    async fn test_glob_tool() {
        let ex = BuiltinToolExecutor;
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.rs"), "").unwrap();
        std::fs::write(dir.path().join("b.rs"), "").unwrap();
        std::fs::write(dir.path().join("c.txt"), "").unwrap();

        let args = mkargs(json!({"pattern": "*.rs", "root": dir.path().to_string_lossy()}));
        let result = assert_ok(ex.execute("glob", args, 0).await);
        let files: Vec<&str> = result["files"].as_array().unwrap()
            .iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|f| f.ends_with("a.rs")));
        assert!(files.iter().any(|f| f.ends_with("b.rs")));
    }

    #[tokio::test]
    async fn test_bash_exec_failure() {
        let ex = BuiltinToolExecutor;
        let args = mkargs(json!({"command": "exit 42", "timeout_ms": 5000}));
        let result = ex.execute("bash_exec", args, 0).await;
        // bash::execute returns Err for non-zero exit codes
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("exit 42"));
    }

    #[tokio::test]
    async fn test_invalid_args() {
        let ex = BuiltinToolExecutor;
        let result = ex.execute("file_read", b"not json".to_vec(), 0).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid JSON"));
    }

    #[tokio::test]
    async fn test_missing_required_arg() {
        let ex = BuiltinToolExecutor;
        let args = mkargs(json!({}));
        let result = ex.execute("file_read", args, 0).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing"));
    }

    // ── agent tool tests ────────────────────────────────────

    #[tokio::test]
    async fn test_executor_unknown_agent_tool() {
        let ex = BuiltinToolExecutor;
        let result = ex.execute("librarian/nonexistent", vec![], 0).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown builtin tool"));

        let result = ex.execute("metis/nonexistent", vec![], 0).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_executor_metis_analyse() {
        let ex = BuiltinToolExecutor;
        let args = mkargs(json!({"request": "Refactor the login module to use OAuth2"}));
        let result = assert_ok(ex.execute("metis/analyse", args, 0).await);
        assert!(result["summary"].as_str().unwrap_or("").len() > 0);
        assert!(result["ambiguities"].is_array());
        assert!(result["implicit_requirements"].is_array());
    }

    #[tokio::test]
    async fn test_executor_momus_review_plan() {
        let ex = BuiltinToolExecutor;
        let plan_text = "- Step 1: Write failing test\n- Step 2: Implement minimum code\n- Step 3: Refactor";
        let args = mkargs(json!({"plan_text": plan_text, "plan_name": "TDD workflow"}));
        let result = assert_ok(ex.execute("momus/review_plan", args, 0).await);
        assert!(result["summary"].as_str().unwrap_or("").len() > 0);
        assert!(result["comments"].is_array());
    }

    #[tokio::test]
    async fn test_executor_momus_review_code() {
        let ex = BuiltinToolExecutor;
        let code = "fn add(a: i32, b: i32) -> i32 { a + b }";
        let args = mkargs(json!({"path": "/fake/lib.rs", "content": code}));
        let result = assert_ok(ex.execute("momus/review_code", args, 0).await);
        assert!(result.get("summary").is_some());
    }

    #[tokio::test]
    async fn test_executor_momus_review_arch() {
        let ex = BuiltinToolExecutor;
        let arch = "The system uses a microservices architecture with message queues.";
        let args = mkargs(json!({"arch_text": arch}));
        let result = assert_ok(ex.execute("momus/review_arch", args, 0).await);
        assert!(result["summary"].as_str().unwrap_or("").len() > 0);
    }

    #[tokio::test]
    async fn test_executor_librarian_synthesise() {
        let ex = BuiltinToolExecutor;
        let findings = json!([
            {"claim": "Rust is memory-safe", "source_url": "https://example.com/rust", "source_title": "Rust docs", "relevance": 0.9, "retrieved_at": "2025-01-01"},
            {"claim": "Rust has zero-cost abstractions", "source_url": "https://example.com/zero", "source_title": "Rust perf", "relevance": 0.8, "retrieved_at": "2025-01-01"},
        ]);
        let args = mkargs(json!({"findings": findings, "max_points": 5}));
        let result = assert_ok(ex.execute("librarian/synthesise", args, 0).await);
        assert!(result["synthesis"].as_str().unwrap_or("").len() > 0);
    }

    #[tokio::test]
    async fn test_executor_librarian_research() {
        let ex = BuiltinToolExecutor;
        let args = mkargs(json!({"query": "Rust async/await", "scope": "web", "num": 1}));
        // May fail in offline env — acceptable
        let result = ex.execute("librarian/research", args, 0).await;
        if let Ok(bytes) = result {
            let v: Value = serde_json::from_slice(&bytes).unwrap();
            assert!(v["query"].as_str().unwrap_or("").len() > 0);
        }
    }

    #[tokio::test]
    async fn test_executor_momus_review_code_batch() {
        let ex = BuiltinToolExecutor;
        let files = json!([
            {"path": "/fake/main.rs", "content": "fn main() { println!(\"hello\"); }"},
            {"path": "/fake/lib.rs", "content": "pub fn add(a: i32, b: i32) -> i32 { a + b }"},
        ]);
        let args = mkargs(json!({"files": files}));
        let result = assert_ok(ex.execute("momus/review_code_batch", args, 0).await);
        assert!(result.as_array().unwrap().len() == 2);
    }

    #[tokio::test]
    async fn test_executor_agent_tool_missing_args() {
        let ex = BuiltinToolExecutor;
        let result = ex.execute("metis/analyse", mkargs(json!({})), 0).await;
        assert!(result.is_err());

        let result = ex.execute("momus/review_plan", mkargs(json!({})), 0).await;
        assert!(result.is_err());

        let result = ex.execute("librarian/research", mkargs(json!({})), 0).await;
        assert!(result.is_err());
    }
}
