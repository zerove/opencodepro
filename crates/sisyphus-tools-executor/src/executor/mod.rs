use sisyphus_tools_core::bridge::BridgeClient;
use sisyphus_tools_llm::metis::LlmClient;
use std::sync::Arc;

pub mod agent_cmds;
pub mod args;
pub mod feedback_cmds;
pub mod file_cmds;
pub mod process_cmds;
pub mod util_cmds;

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
/// | `feedback/save` | `{"agent_id": "...", "request": "...", "output": "...", "corrected": "...", "tags": [...]}` | `{"status": "saved"}` |
/// | `feedback/get` | `{"agent_id": "...", "limit": N}` | `[Feedback]` JSON array |
/// | `librarian/research` | `{"query": "...", "scope": "web|docs|code|all", "num": N}` | Full `ResearchReport` JSON |
/// | `librarian/fetch_docs` | `{"url": "..."}` | Full `DocPage` JSON |
/// | `librarian/examples` | `{"query": "...", "language": "..."}` | `[CodeExample]` JSON array |
/// | `librarian/synthesise` | `{"findings": [...], "max_points": N}` | `{"synthesis": "..."}` |
/// | `metis/analyse` | `{"request": "..."}` | Full `MetisAnalysis` JSON |
/// | `momus/review_plan` | `{"plan_text": "...", "plan_name": "..."}` | Full `MomusReview` JSON |
/// | `momus/review_code` | `{"path": "...", "content": "..."}` | Full `MomusReview` JSON |
/// | `momus/review_code_batch` | `{"files": [{"path": "...", "content": "..."}]}` | `[MomusReview]` JSON array |
/// | `momus/review_arch` | `{"arch_text": "..."}` | Full `MomusReview` JSON |
/// | `security/audit_code` | `{"path": "...", "content": "..."}` | Full `SecurityAuditReport` JSON |
/// | `security/threat_model` | `{"arch_text": "..."}` | Full `ThreatModelReport` JSON |
#[derive(Clone)]
pub struct BuiltinToolExecutor {
    pub registry: sisyphus_tools_core::hook::HookRegistry,
    pub llm_client: Arc<dyn LlmClient>,
    pub feedback_store: Arc<sisyphus_tools_core::feedback::FeedbackStore>,
    pub bridge_client: Option<BridgeClient>,
}

#[async_trait::async_trait]
impl sisyphus_types::ToolExecutor for BuiltinToolExecutor {
    async fn execute(
        &self,
        tool_id: &str,
        args: Vec<u8>,
        _fuel_limit: u64,
    ) -> Result<Vec<u8>, sisyphus_types::TypesError> {
        let llm_client = self.llm_client.clone();
        let feedback_store = self.feedback_store.clone();
        let self_ref = Arc::new(self.clone());
            sisyphus_tools_core::hook::execute_with_hooks(tool_id, args, &self.registry, move |enriched_args| {
            let client = llm_client.clone();
            let _executor_ref = Arc::clone(&self_ref);
            async move {
                match tool_id {
                    // ── file commands ──────────────────────────────
                    "file_read" => file_cmds::cmd_file_read(enriched_args),
                    "file_write" => file_cmds::cmd_file_write(enriched_args),
                    "file_edit" => file_cmds::cmd_file_edit(enriched_args),
                    "file_edit_all" => file_cmds::cmd_file_edit_all(enriched_args),
                    "file_read_lines" => file_cmds::cmd_file_read_lines(enriched_args),
                    "glob" => file_cmds::cmd_glob(enriched_args),
                    "grep" => file_cmds::cmd_grep(enriched_args),

                    // ── process commands ──────────────────────────
                    "bash_exec" => process_cmds::cmd_bash_exec(enriched_args).await,
                    "web_search" => process_cmds::cmd_web_search(enriched_args).await,
                    "web_fetch" => process_cmds::cmd_web_fetch(enriched_args).await,

                    // ── utility commands ──────────────────────────
                    "json_query" => {
                        let v = serde_json::from_slice(&enriched_args).map_err(|e| e.to_string())?;
                        util_cmds::cmd_json_query(v).map_err(|e| e.to_string())
                    },
                    "base64_convert" => {
                        let v = serde_json::from_slice(&enriched_args).map_err(|e| e.to_string())?;
                        util_cmds::cmd_base64_convert(v).map_err(|e| e.to_string())
                    },
                    "time_convert" => {
                        let v = serde_json::from_slice(&enriched_args).map_err(|e| e.to_string())?;
                        util_cmds::cmd_time_convert(v).map_err(|e| e.to_string())
                    },
                    "url_convert" => {
                        let v = serde_json::from_slice(&enriched_args).map_err(|e| e.to_string())?;
                        util_cmds::cmd_url_convert(v).map_err(|e| e.to_string())
                    },
                    "path_expand" => {
                        let v = serde_json::from_slice(&enriched_args).map_err(|e| e.to_string())?;
                        util_cmds::cmd_path_expand(v).map_err(|e| e.to_string())
                    },
                    "lsp_symbol_search" => {
                        let v = serde_json::from_slice(&enriched_args).map_err(|e| e.to_string())?;
                        util_cmds::cmd_lsp_symbol_search(&self_ref, v).await.map_err(|e| e.to_string())
                    },
                    "dependency_graph" => {
                        let v = serde_json::from_slice(&enriched_args).map_err(|e| e.to_string())?;
                        util_cmds::cmd_dependency_graph(v).await.map_err(|e| e.to_string())
                    },
                    "doc_sync_check" => {
                        let v = serde_json::from_slice(&enriched_args).map_err(|e| e.to_string())?;
                        util_cmds::cmd_doc_sync_check(&client, v).await.map_err(|e| e.to_string())
                    },
                    "coverage_analyze" => {
                        let v = serde_json::from_slice(&enriched_args).map_err(|e| e.to_string())?;
                        util_cmds::cmd_coverage_analyze(v).await.map_err(|e| e.to_string())
                    },
                    "git_blame" => {
                        let v = serde_json::from_slice(&enriched_args).map_err(|e| e.to_string())?;
                        util_cmds::cmd_git_blame(v).await.map_err(|e| e.to_string())
                    },
                    "type_check" => {
                        let v = serde_json::from_slice(&enriched_args).map_err(|e| e.to_string())?;
                        util_cmds::cmd_type_check(v).await.map_err(|e| e.to_string())
                    },

                    // ── feedback commands ─────────────────────────
                    "feedback/save" => {
                        feedback_cmds::cmd_feedback_save(feedback_store.clone(), enriched_args).await
                    }
                    "feedback/get" => feedback_cmds::cmd_feedback_get(feedback_store.clone(), enriched_args).await,
                    "feedback/query" => {
                        feedback_cmds::cmd_feedback_query(feedback_store.clone(), enriched_args).await
                    }

                    // ── librarian commands ─────────────────────────
                    "librarian/research" => agent_cmds::cmd_librarian_research(enriched_args).await,
                    "librarian/fetch_docs" => agent_cmds::cmd_librarian_fetch_docs(enriched_args).await,
                    "librarian/examples" => agent_cmds::cmd_librarian_examples(enriched_args).await,
                    "librarian/synthesise" => {
                        agent_cmds::cmd_librarian_synthesise(client.as_ref(), enriched_args).await
                    }

                    // ── metis commands ────────────────────────────
                    "metis/analyse" => agent_cmds::cmd_metis_analyse(client.as_ref(), enriched_args).await,

                    // ── momus commands ────────────────────────────
                    "momus/review_plan" => {
                        agent_cmds::cmd_momus_review_plan(client.as_ref(), enriched_args).await
                    }
                    "momus/review_code" => {
                        agent_cmds::cmd_momus_review_code(client.as_ref(), enriched_args).await
                    }
                    "momus/review_code_batch" => {
                        agent_cmds::cmd_momus_review_code_batch(client.as_ref(), enriched_args).await
                    }
                    "momus/review_arch" => {
                        agent_cmds::cmd_momus_review_arch(client.as_ref(), enriched_args).await
                    }

                    // ── scribe commands ────────────────────────────
                    "scribe/generate_adr" => {
                        agent_cmds::cmd_scribe_generate_adr(client.as_ref(), enriched_args).await
                    }
                    "scribe/update_docs" => {
                        agent_cmds::cmd_scribe_update_docs(client.as_ref(), enriched_args).await
                    }
                    "scribe/apply_update" => agent_cmds::cmd_scribe_apply_update(enriched_args),

                    // ── security commands ─────────────────────────
                    "security/audit_code" => {
                        agent_cmds::cmd_security_audit_code(client.as_ref(), enriched_args).await
                    }
                    "security/threat_model" => {
                        agent_cmds::cmd_security_threat_model(client.as_ref(), enriched_args).await
                    }

                    other => Err(format!("unknown builtin tool: {other}")),
                }
            }
        })
        .await
        .map_err(|e| sisyphus_types::TypesError::ToolExecutionError {
            tool_id: tool_id.to_string(),
            details: e,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sisyphus_tools_llm::metis::LlmClient;
    use serde_json::{json, Value};
    use sisyphus_types::ToolExecutor;

    struct MockLlmClient;
    #[async_trait::async_trait]
    impl LlmClient for MockLlmClient {
        async fn generate(&self, prompt: &str) -> Result<String, String> {
            self.chat("", prompt, false).await
        }

        async fn chat(
            &self,
            _system: &str,
            user: &str,
            _json_mode: bool,
        ) -> Result<String, String> {
            Ok(format!(
                r#"{{"ambiguities": [], "implicit_requirements": [], "summary": "Mock analysis for: {}", "actionable": true}}"#,
                user
            ))
        }
    }

    fn mkargs(val: Value) -> Vec<u8> {
        serde_json::to_vec(&val).unwrap()
    }

    fn assert_ok(result: Result<Vec<u8>, sisyphus_types::TypesError>) -> Value {
        let bytes = result.unwrap_or_else(|e| panic!("expected ok: {e}"));
        serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("invalid JSON result: {e}"))
    }

    fn make_executor() -> BuiltinToolExecutor {
        BuiltinToolExecutor {
            registry: sisyphus_tools_core::hook::HookRegistry::new(),
            llm_client: Arc::new(MockLlmClient),
            feedback_store: Arc::new(sisyphus_tools_core::feedback::FeedbackStore::default_dir()),
            bridge_client: None,
        }
    }

    #[tokio::test]
    async fn test_executor_unknown_tool() {
        let ex = make_executor();
        let result = ex.execute("no_such_tool", vec![], 0).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown builtin tool"));
    }

    #[tokio::test]
    async fn test_file_read_write_roundtrip() {
        let ex = make_executor();
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
        let ex = make_executor();
        let args = mkargs(json!({"command": "echo hello", "timeout_ms": 5000}));
        let result = assert_ok(ex.execute("bash_exec", args, 0).await);
        assert_eq!(result["stdout"].as_str().unwrap().trim(), "hello");
        assert_eq!(result["exit_code"], 0);
    }

    #[tokio::test]
    async fn test_web_search_executor() {
        let ex = make_executor();
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
        let ex = make_executor();
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
        let ex = make_executor();
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
        let ex = make_executor();
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.rs"), "").unwrap();
        std::fs::write(dir.path().join("b.rs"), "").unwrap();
        std::fs::write(dir.path().join("c.txt"), "").unwrap();

        let args = mkargs(json!({"pattern": "*.rs", "root": dir.path().to_string_lossy()}));
        let result = assert_ok(ex.execute("glob", args, 0).await);
        let files: Vec<&str> = result["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|f| f.ends_with("a.rs")));
        assert!(files.iter().any(|f| f.ends_with("b.rs")));
    }

    #[tokio::test]
    async fn test_bash_exec_failure() {
        let ex = make_executor();
        let args = mkargs(json!({"command": "exit 42", "timeout_ms": 5000}));
        let result = ex.execute("bash_exec", args, 0).await;
        // bash::execute returns Err for non-zero exit codes
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("exit 42"));
    }

    #[tokio::test]
    async fn test_invalid_args() {
        let ex = make_executor();
        let result = ex.execute("file_read", b"not json".to_vec(), 0).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("invalid JSON"));
    }

    #[tokio::test]
    async fn test_missing_required_arg() {
        let ex = make_executor();
        let args = mkargs(json!({}));
        let result = ex.execute("file_read", args, 0).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing"));
    }

    // ── agent tool tests ────────────────────────────────────

    #[tokio::test]
    async fn test_executor_unknown_agent_tool() {
        let ex = make_executor();
        let result = ex.execute("librarian/nonexistent", vec![], 0).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown builtin tool"));

        let result = ex.execute("metis/nonexistent", vec![], 0).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_executor_metis_analyse() {
        let ex = make_executor();
        let args = mkargs(json!({"request": "Refactor the login module to use OAuth2"}));
        let result = assert_ok(ex.execute("metis/analyse", args, 0).await);
        assert!(!result["summary"].as_str().unwrap_or("").is_empty());
        assert!(result["ambiguities"].is_array());
        assert!(result["implicit_requirements"].is_array());
    }

    #[tokio::test]
    async fn test_executor_momus_review_plan() {
        let ex = make_executor();
        let plan_text =
            "- Step 1: Write failing test\n- Step 2: Implement minimum code\n- Step 3: Refactor";
        let args = mkargs(json!({"plan_text": plan_text, "plan_name": "TDD workflow"}));
        let result = assert_ok(ex.execute("momus/review_plan", args, 0).await);
        assert!(!result["summary"].as_str().unwrap_or("").is_empty());
        assert!(result["comments"].is_array());
    }

    #[tokio::test]
    async fn test_executor_momus_review_code() {
        let ex = make_executor();
        let code = "fn add(a: i32, b: i32) -> i32 { a + b }";
        let args = mkargs(json!({"path": "/fake/lib.rs", "content": code}));
        let result = assert_ok(ex.execute("momus/review_code", args, 0).await);
        assert!(result.get("summary").is_some());
    }

    #[tokio::test]
    async fn test_executor_momus_review_arch() {
        let ex = make_executor();
        let arch = "The system uses a microservices architecture with message queues.";
        let args = mkargs(json!({"arch_text": arch}));
        let result = assert_ok(ex.execute("momus/review_arch", args, 0).await);
        assert!(!result["summary"].as_str().unwrap_or("").is_empty());
    }

    #[tokio::test]
    async fn test_executor_librarian_synthesise() {
        let ex = make_executor();
        let findings = json!([
            {"claim": "Rust is memory-safe", "source_url": "https://example.com/rust", "source_title": "Rust docs", "relevance": 0.9, "retrieved_at": "2025-01-01"},
            {"claim": "Rust has zero-cost abstractions", "source_url": "https://example.com/zero", "source_title": "Rust perf", "relevance": 0.8, "retrieved_at": "2025-01-01"},
        ]);
        let args = mkargs(json!({"findings": findings, "max_points": 5}));
        let result = assert_ok(ex.execute("librarian/synthesise", args, 0).await);
        assert!(!result["synthesis"].as_str().unwrap_or("").is_empty());
    }

    #[tokio::test]
    async fn test_executor_librarian_research() {
        let ex = make_executor();
        let args = mkargs(json!({"query": "Rust async/await", "scope": "web", "num": 1}));
        // May fail in offline env — acceptable
        let result = ex.execute("librarian/research", args, 0).await;
        if let Ok(bytes) = result {
            let v: Value = serde_json::from_slice(&bytes).unwrap();
            assert!(!v["query"].as_str().unwrap_or("").is_empty());
        }
    }

    #[tokio::test]
    async fn test_executor_momus_review_code_batch() {
        let ex = make_executor();
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
        let ex = make_executor();
        let result = ex.execute("metis/analyse", mkargs(json!({})), 0).await;
        assert!(result.is_err());

        let result = ex.execute("momus/review_plan", mkargs(json!({})), 0).await;
        assert!(result.is_err());

        let result = ex.execute("librarian/research", mkargs(json!({})), 0).await;
        assert!(result.is_err());
    }
}
