use serde_json::Value;
use sisyphus_bridge_api::sisyphus_bridge_client::SisyphusBridgeClient;
use sisyphus_bridge_api::ExecuteToolRequest;
use std::sync::Arc;
use tonic::{transport::Channel, Request};

#[derive(Clone)]
pub struct BridgeClient {
    client: Arc<tokio::sync::RwLock<SisyphusBridgeClient<Channel>>>,
}

impl BridgeClient {
    pub async fn new(endpoint: String) -> Result<Self, String> {
        let channel = Channel::from_shared(endpoint)
            .map_err(|e| format!("Invalid bridge endpoint: {e}"))?
            .connect()
            .await
            .map_err(|e| format!("Failed to connect to bridge: {e}"))?;

        Ok(Self {
            client: Arc::new(tokio::sync::RwLock::new(SisyphusBridgeClient::new(channel))),
        })
    }

    pub async fn execute_tool(&self, tool_id: &str, args: Value) -> Result<Value, String> {
        let args_json =
            serde_json::to_string(&args).map_err(|e| format!("Args serialization failed: {e}"))?;

        let request = Request::new(ExecuteToolRequest {
            tool_id: tool_id.to_string(),
            args_json,
            fuel_limit: 0,
        });

        let mut client = self.client.write().await;
        let response = client
            .execute_tool(request)
            .await
            .map_err(|e| format!("Bridge call failed: {e}"))?;

        let inner = response.into_inner();
        if inner.status == "SUCCESS" {
            serde_json::from_str(&inner.result_json)
                .map_err(|e| format!("Result deserialization failed: {e}"))
        } else {
            Err(inner.status)
        }
    }
}
