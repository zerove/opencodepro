use serde::{Deserialize, Serialize};
use tauri::command;

const DEFAULT_BASE_URL: &str = "http://localhost:8080";

#[derive(Debug, Serialize, Deserialize)]
struct HealthResponse {
    status: String,
    version: String,
    uptime_secs: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct Session {
    id: String,
    name: String,
    message_count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct SearchResult {
    id: String,
    score: f64,
    content: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ExecutionResult {
    sop_id: String,
    status: String,
    iterations: u32,
    deviation: f64,
}

fn get_base_url() -> String {
    dirs::home_dir()
        .and_then(|h| std::fs::read_to_string(h.join(".sisyphus/config.json")).ok())
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v["bridge_address"].as_str().map(String::from))
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
}

#[command]
async fn get_health() -> Result<HealthResponse, String> {
    let url = format!("{}/health", get_base_url());
    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("Connection failed: {e}"))?;

    resp.json::<HealthResponse>()
        .await
        .map_err(|e| format!("Parse error: {e}"))
}

#[command]
async fn list_sessions() -> Result<Vec<Session>, String> {
    let url = format!("{}/api/v1/sessions", get_base_url());
    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("Connection failed: {e}"))?;

    resp.json::<Vec<Session>>()
        .await
        .map_err(|e| format!("Parse error: {e}"))
}

#[command]
async fn search_memory(query: String, top_k: usize) -> Result<Vec<SearchResult>, String> {
    let url = format!("{}/api/v1/vector/search", get_base_url());
    let body = serde_json::json!({ "query": query, "top_k": top_k });

    let resp = reqwest::Client::new()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Connection failed: {e}"))?;

    resp.json::<Vec<SearchResult>>()
        .await
        .map_err(|e| format!("Parse error: {e}"))
}

#[command]
async fn execute_sop(sop_id: String) -> Result<ExecutionResult, String> {
    let url = format!("{}/api/v1/execute", get_base_url());
    let body = serde_json::json!({ "sop_id": sop_id, "context": {} });

    let resp = reqwest::Client::new()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Connection failed: {e}"))?;

    resp.json::<ExecutionResult>()
        .await
        .map_err(|e| format!("Parse error: {e}"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_health,
            list_sessions,
            search_memory,
            execute_sop,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
