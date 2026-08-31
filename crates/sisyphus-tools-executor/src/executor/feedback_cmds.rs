use crate::executor::args::{ok_json, parse_args, str_arg};
use serde_json::json;
use std::sync::Arc;

// ── feedback command implementations ──────────────────────────

/// Feedback command: persist a single feedback entry for later analysis.
///
/// Args (JSON): { `key`, `value`, `tags`: Vec<String>, `weight` (default 1.0) }
pub async fn cmd_feedback_save(
    store: Arc<sisyphus_tools_core::feedback::FeedbackStore>,
    args: Vec<u8>,
) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let agent_id = str_arg(&v, "agent_id")?;
    let request = str_arg(&v, "request")?;
    let output = str_arg(&v, "output")?;
    let corrected = str_arg(&v, "corrected")?;
    let tags = v
        .get("tags")
        .and_then(|t| t.as_array())
        .map(|t| {
            t.iter().filter_map(|v| v.as_str().map(std::string::ToString::to_string)).collect()
        })
        .unwrap_or_default();

    let feedback =
        sisyphus_tools_core::feedback::Feedback::new(agent_id, request, output, corrected, tags);

    store.save_feedback(feedback).await.map_err(|e| e.to_string())?;
    ok_json(json!({"status": "saved"}))
}

/// Feedback command: load a single feedback entry by id.
///
/// Args (JSON): { `id`: feedback entry id }
///
/// Returns JSON `{ entry: Option<Feedback> }`.
pub async fn cmd_feedback_get(
    store: Arc<sisyphus_tools_core::feedback::FeedbackStore>,
    args: Vec<u8>,
) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let agent_id = str_arg(&v, "agent_id")?;
    let limit = crate::executor::args::usize_arg(&v, "limit").unwrap_or(10);

    let feedbacks = store.get_recent_feedback(&agent_id, limit).await.map_err(|e| e.to_string())?;
    ok_json(serde_json::to_value(&feedbacks).map_err(|e| format!("serialize: {e}"))?)
}

/// Feedback command: query the feedback store by tags / time range / minimum weight.
///
/// Args (JSON):
/// - `tags` (optional): list of tags (AND match)
/// - `since` (optional): RFC3339 timestamp lower bound
/// - `min_weight` (default 0.0): minimum weight filter
/// - `limit` (default 50): max results
pub async fn cmd_feedback_query(
    store: Arc<sisyphus_tools_core::feedback::FeedbackStore>,
    args: Vec<u8>,
) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let query = str_arg(&v, "query")?;
    let tags = v.get("tags").and_then(|t| t.as_array()).map(|t| {
        t.iter().filter_map(|v| v.as_str().map(std::string::ToString::to_string)).collect()
    });

    let feedbacks = store.query_feedback(&query, tags).await.map_err(|e| e.to_string())?;
    ok_json(serde_json::to_value(&feedbacks).map_err(|e| format!("serialize: {e}"))?)
}
