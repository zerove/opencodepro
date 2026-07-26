//! Sisyphus Tauri Desktop Application
//!
//! Provides a native desktop GUI for the Sisyphus cognitive engine.
//! Uses the Sisyphus Rust SDK to communicate with the backend.

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|_app| {
            tracing::info!("Sisyphus Desktop starting");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            health_check,
            get_state,
            set_state,
            list_state,
            search_vectors,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// Tauri commands

#[tauri::command]
async fn health_check(server_url: String) -> Result<serde_json::Value, String> {
    let client = sisyphus_sdk::SisyphusClient::new(&server_url);
    let health = client.health().await.map_err(|e| e.to_string())?;
    serde_json::to_value(health).map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_state(server_url: String, key: String) -> Result<serde_json::Value, String> {
    let client = sisyphus_sdk::SisyphusClient::new(&server_url);
    match client.get_state(&key).await {
        Ok(state) => serde_json::to_value(state).map_err(|e| e.to_string()),
        Err(sisyphus_sdk::SdkError::NotFound(_)) => Ok(serde_json::Value::Null),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
async fn set_state(server_url: String, key: String, value: serde_json::Value) -> Result<(), String> {
    let client = sisyphus_sdk::SisyphusClient::new(&server_url);
    client.set_state(&key, value).await.map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn list_state(server_url: String) -> Result<serde_json::Value, String> {
    let client = sisyphus_sdk::SisyphusClient::new(&server_url);
    let response = client.list_state(None, None).await.map_err(|e| e.to_string())?;
    serde_json::to_value(response).map_err(|e| e.to_string())
}

#[tauri::command]
async fn search_vectors(server_url: String, query: Vec<f32>, top_k: usize) -> Result<serde_json::Value, String> {
    let client = sisyphus_sdk::SisyphusClient::new(&server_url);
    let response = client.search_vectors(&query, top_k).await.map_err(|e| e.to_string())?;
    serde_json::to_value(response).map_err(|e| e.to_string())
}