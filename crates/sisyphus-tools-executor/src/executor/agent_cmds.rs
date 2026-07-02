use crate::executor::args::{flexible_str_arg, flexible_str_arg_multi, ok_json, parse_args, path_arg, str_arg};
use sisyphus_tools_llm::metis::LlmClient;
use serde_json::json;
use std::path::Path;

// ── librarian commands ──────────────────────────────────────

pub async fn cmd_librarian_research(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let query = flexible_str_arg(&v, "query", "content")?;
    let scope_str = str_arg(&v, "scope").unwrap_or_else(|_| "web".into());
    let scope: sisyphus_tools_llm_tools::librarian::ResearchScope = scope_str
        .parse()
        .map_err(|e: String| format!("invalid scope: {e}"))?;
    let num = crate::executor::args::usize_arg(&v, "num").unwrap_or(5);
    let report = sisyphus_tools_llm_tools::librarian::research(&query, scope, num)
        .await
        .map_err(|e| e.to_string())?;
    ok_json(serde_json::to_value(&report).map_err(|e| format!("serialize: {e}"))?)
}

pub async fn cmd_librarian_fetch_docs(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let url = str_arg(&v, "url")?;
    let page = sisyphus_tools_llm_tools::librarian::fetch_documentation(&url)
        .await
        .map_err(|e| e.to_string())?;
    ok_json(serde_json::to_value(&page).map_err(|e| format!("serialize: {e}"))?)
}

pub async fn cmd_librarian_examples(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let query = flexible_str_arg(&v, "query", "content")?;
    let language = str_arg(&v, "language").unwrap_or_else(|_| "rust".into());
    let examples = sisyphus_tools_llm_tools::librarian::find_examples(&query, &language)
        .await
        .map_err(|e| e.to_string())?;
    ok_json(serde_json::to_value(&examples).map_err(|e| format!("serialize: {e}"))?)
}

pub async fn cmd_librarian_synthesise(
    client: &dyn LlmClient,
    args: Vec<u8>,
) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let findings_val = v
        .get("findings")
        .ok_or_else(|| "missing 'findings' arg".to_string())?;
    let findings: Vec<sisyphus_tools_llm_tools::librarian::ResearchFinding> = serde_json::from_value(findings_val.clone())
        .map_err(|e| format!("invalid findings JSON: {e}"))?;

    let synthesis = sisyphus_tools_llm_tools::librarian::synthesise(&findings, client)
        .await
        .map_err(|e| format!("synthesis failed: {e}"))?;

    ok_json(json!({"synthesis": synthesis}))
}

// ── metis commands ──────────────────────────────────────────

pub async fn cmd_metis_analyse(client: &dyn LlmClient, args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let request = flexible_str_arg(&v, "request", "content")?;
    let analysis = sisyphus_tools_llm::metis::analyse(&request, client)
        .await
        .map_err(|e| e.to_string())?;
    ok_json(serde_json::to_value(&analysis).map_err(|e| format!("serialize: {e}"))?)
}

// ── momus commands ──────────────────────────────────────────

pub async fn cmd_momus_review_plan(client: &dyn LlmClient, args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let plan_text = flexible_str_arg_multi(&v, &["plan_text", "artifact", "content"])?;
    let plan_name = str_arg(&v, "plan_name").unwrap_or_else(|_| "unnamed".into());
    let review = sisyphus_tools_llm_tools::momus::review_plan(client, &plan_text, &plan_name).await;
    ok_json(serde_json::to_value(&review).map_err(|e| format!("serialize: {e}"))?)
}

pub async fn cmd_momus_review_code(client: &dyn LlmClient, args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let path_str = flexible_str_arg_multi(&v, &["path", "file", "content"])?;
    let content = flexible_str_arg_multi(&v, &["content", "artifact", "code", "path"])?;
    let path = Path::new(&path_str);
    let review = sisyphus_tools_llm_tools::momus::review_code(client, path, &content)
        .await
        .map_err(|e| e.to_string())?;
    ok_json(serde_json::to_value(&review).map_err(|e| format!("serialize: {e}"))?)
}

pub async fn cmd_momus_review_code_batch(
    client: &dyn LlmClient,
    args: Vec<u8>,
) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let files_val = v
        .get("files")
        .ok_or_else(|| "missing 'files' arg".to_string())?;
    let files: Vec<serde_json::Value> = serde_json::from_value(files_val.clone())
        .map_err(|e| format!("invalid files JSON: {e}"))?;
    let mut pairs = Vec::new();
    for f in &files {
        let path_str = f
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "each file entry must have a 'path' string".to_string())?;
        let content = f
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "each file entry must have a 'content' string".to_string())?;
        pairs.push((Path::new(path_str), content));
    }
    let reviews = sisyphus_tools_llm_tools::momus::review_code_batch(client, &pairs)
        .await
        .map_err(|e| e.to_string())?;
    ok_json(serde_json::to_value(&reviews).map_err(|e| format!("serialize: {e}"))?)
}

pub async fn cmd_momus_review_arch(client: &dyn LlmClient, args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let arch_text = flexible_str_arg_multi(&v, &["arch_text", "artifact", "content"])?;
    let review = sisyphus_tools_llm_tools::momus::review_architecture(client, &arch_text).await;
    ok_json(serde_json::to_value(&review).map_err(|e| format!("serialize: {e}"))?)
}

// ── scribe commands ──────────────────────────────────────────

pub async fn cmd_scribe_generate_adr(client: &dyn LlmClient, args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let context = flexible_str_arg(&v, "context", "content")?;
    let adr = sisyphus_tools_llm_tools::scribe::generate_adr(client, &context).await?;
    ok_json(json!({"adr": adr}))
}

pub async fn cmd_scribe_update_docs(client: &dyn LlmClient, args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let doc_content = flexible_str_arg(&v, "doc_content", "content")?;
    let changes = str_arg(&v, "changes")?;
    let suggestion = sisyphus_tools_llm_tools::scribe::update_docs(client, &doc_content, &changes).await?;
    ok_json(json!({"suggestion": suggestion}))
}

pub fn cmd_scribe_apply_update(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let path_str = path_arg(&v, "path")?;
    let content = str_arg(&v, "content")?;
    let path = Path::new(&path_str);

    let forbidden = ["Cargo.toml", "Cargo.lock", ".env", ".git/"];
    for f in forbidden {
        if path_str.contains(f) {
            return Err(format!(
                "writing to {f} is forbidden for scribe/apply_update"
            ));
        }
    }

    if content.contains("<<<< SEARCH") {
        let mut current_content = content.as_str();
        let mut applied_count = 0;

        while let Some(start_idx) = current_content.find("<<<< SEARCH") {
            let search_start = start_idx + "<<<< SEARCH".len();
            let replace_idx = current_content[search_start..]
                .find("==== REPLACE")
                .map(|i| i + search_start)
                .ok_or_else(|| "missing ==== REPLACE marker".to_string())?;

            let search_text = current_content[search_start..replace_idx]
                .trim_start_matches('\n')
                .trim_end_matches('\n');

            let end_idx = current_content[replace_idx..]
                .find(">>>> END")
                .map(|i| i + replace_idx)
                .ok_or_else(|| "missing >>>> END marker".to_string())?;

            let replace_start = replace_idx + "==== REPLACE".len();
            let replace_text = current_content[replace_start..end_idx]
                .trim_start_matches('\n')
                .trim_end_matches('\n');

            sisyphus_tools_system::file::edit(path, search_text, replace_text).map_err(|e| e.to_string())?;

            applied_count += 1;
            current_content = &current_content[end_idx + ">>>> END".len()..];
        }
        ok_json(json!({"applied_edits": applied_count}))
    } else {
        sisyphus_tools_system::file::write(path, &content).map_err(|e| e.to_string())?;
        ok_json(json!({"bytes_written": content.len()}))
    }
}

// ── security commands ───────────────────────────────────────

pub async fn cmd_security_audit_code(client: &dyn LlmClient, args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let path_str =
        flexible_str_arg_multi(&v, &["path", "file"]).unwrap_or_else(|_| "snippet.rs".to_string());
    let content = flexible_str_arg_multi(&v, &["content", "artifact", "code", "path"])?;
    let path = Path::new(&path_str);
    let report = sisyphus_tools_llm_tools::security::audit_code(client, path, &content)
        .await
        .map_err(|e| e.to_string())?;
    ok_json(serde_json::to_value(&report).map_err(|e| format!("serialize: {e}"))?)
}

pub async fn cmd_security_threat_model(
    client: &dyn LlmClient,
    args: Vec<u8>,
) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let arch_text = flexible_str_arg_multi(&v, &["arch_text", "artifact", "content"])?;
    let report = sisyphus_tools_llm_tools::security::threat_model(client, &arch_text)
        .await
        .map_err(|e| e.to_string())?;
    ok_json(serde_json::to_value(&report).map_err(|e| format!("serialize: {e}"))?)
}
