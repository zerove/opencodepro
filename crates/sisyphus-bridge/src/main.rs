use sisyphus_bridge_api::sisyphus_bridge_server::SisyphusBridgeServer;
use sisyphus_metrics::init_metrics;
use sisyphus_tools_system::config_store::ConfigStore;
use sisyphus_types::Config;
use std::sync::{Arc, RwLock};
use tokio::signal;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;
use tracing::{error, info, warn};

use sisyphus_bridge::{auth, bridge_service::MySisyphusBridge, http};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config_path =
        std::env::var("SISYPHUS_CONFIG").unwrap_or_else(|_| "config.toml".to_string());

    let (_store, shared_config) = match ConfigStore::load(&config_path) {
        Ok(s) => {
            info!(path = %config_path, "Loaded configuration with hot-reload");
            s
        }
        Err(e) => {
            warn!(path = %config_path, error = %e, "Config file not found, using defaults");
            let cfg = Arc::new(RwLock::new(Config::default()));
            // Create a no-op store that never reloads
            let store = ConfigStore::with_config(cfg.clone());
            (store, cfg)
        }
    };

    let auth_token = {
        let config = shared_config.read().map_err(|e| {
            Box::new(std::io::Error::other(format!("Config lock poisoned: {e}")))
                as Box<dyn std::error::Error>
        })?;
        let _ = init_metrics(config.metrics.prometheus_port);
        config.auth_token.clone()
    };

    let grpc_addr: std::net::SocketAddr = "[::1]:50051".parse()?;
    let http_addr_str = {
        let config = shared_config.read().map_err(|e| {
            Box::new(std::io::Error::other(format!("Config lock poisoned: {e}")))
                as Box<dyn std::error::Error>
        })?;
        config.http_address.clone()
    };

    // Initialize bridge with shared config for dynamic updates
    let bridge = Arc::new(MySisyphusBridge::new(shared_config.clone(), None).await?);

    // Spawn config hot-reload in background
    tokio::spawn(async move {
        _store.start_watching().await;
    });

    // ── Start HTTP REST API server ──────────────────────────────────────
    let data_dir = std::path::PathBuf::from("data");
    let rest_state = crate::http::RestApiState::new(bridge.clone(), data_dir);
    let http_app = crate::http::build_router(rest_state)
        .layer(tower_http::cors::CorsLayer::permissive())
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let http_addr: std::net::SocketAddr = http_addr_str
        .strip_prefix("http://")
        .unwrap_or(&http_addr_str)
        .parse()
        .unwrap_or(std::net::SocketAddr::from(([127, 0, 0, 1], 8080)));
    info!("Starting HTTP REST API server on {http_addr}");

    let _http_handle = tokio::spawn(async move {
        match tokio::net::TcpListener::bind(http_addr).await {
            Ok(listener) => {
                if let Err(e) = axum::serve(listener, http_app)
                    .with_graceful_shutdown(async {
                        signal::ctrl_c().await.ok();
                        info!("Received SIGINT, starting HTTP graceful shutdown...");
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    })
                    .await
                {
                    error!("HTTP server error: {e}");
                }
            }
            Err(e) => {
                error!("Failed to bind HTTP listener on {http_addr}: {e}");
            }
        }
    });

    // ── Start gRPC server ───────────────────────────────────────────────
    let auth_interceptor = auth::AuthInterceptor::new(auth_token);
    let grpc_server = Server::builder().add_service(SisyphusBridgeServer::with_interceptor(
        (*bridge).clone(),
        auth_interceptor,
    ));

    // Try IPv6 (::1) first; fall back to IPv4 (127.0.0.1) on systems without IPv6 loopback
    let grpc_listener = match tokio::net::TcpListener::bind(grpc_addr).await {
        Ok(l) => l,
        Err(_) => {
            let v4_addr: std::net::SocketAddr = format!("127.0.0.1:{}", grpc_addr.port())
                .parse()
                .map_err(|e| {
                Box::new(std::io::Error::new(std::io::ErrorKind::InvalidInput, e))
            })?;
            warn!("Failed to bind gRPC on {grpc_addr}, falling back to {v4_addr}");
            tokio::net::TcpListener::bind(v4_addr).await?
        }
    };
    let actual_grpc_addr = grpc_listener.local_addr().unwrap_or(grpc_addr);
    let grpc_incoming = TcpListenerStream::new(grpc_listener);

    info!("Starting gRPC server on {actual_grpc_addr}");
    grpc_server
        .serve_with_incoming_shutdown(grpc_incoming, async {
            signal::ctrl_c().await.ok();
            info!("Received SIGINT, starting gRPC graceful shutdown...");
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        })
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use sisyphus_bridge_api::{
        sisyphus_bridge_server::SisyphusBridge, AddMessageRequest, AddMessageResponse,
        DecomposeGoalRequest, DecomposeGoalResponse, ExecuteSopRequest, ExecuteToolRequest,
        HealthCheckRequest, LearningInsightsRequest, LearningInsightsResponse, ListAgentsRequest,
        ListPluginsRequest, ListPluginsResponse, ListSoPsRequest, ListToolsRequest,
        ListToolsResponse, LoadPluginRequest, LoadPluginResponse, TaskProto, UnloadPluginRequest,
        UnloadPluginResponse,
    };
    use std::collections::HashMap;
    use tokio_stream::StreamExt;
    use tonic::{Request, Response, Status};

    struct TestBridge(&'static str);

    #[tonic::async_trait]
    impl SisyphusBridge for TestBridge {
        async fn list_plugins(
            &self,
            _request: Request<ListPluginsRequest>,
        ) -> Result<Response<ListPluginsResponse>, Status> {
            Ok(Response::new(ListPluginsResponse { plugins: vec![] }))
        }

        async fn load_plugin(
            &self,
            _request: Request<LoadPluginRequest>,
        ) -> Result<Response<LoadPluginResponse>, Status> {
            Ok(Response::new(LoadPluginResponse {
                success: true,
                error: String::new(),
            }))
        }

        async fn unload_plugin(
            &self,
            _request: Request<UnloadPluginRequest>,
        ) -> Result<Response<UnloadPluginResponse>, Status> {
            Ok(Response::new(UnloadPluginResponse {
                success: true,
                error: String::new(),
            }))
        }

        async fn execute_sop(
            &self,
            _request: Request<ExecuteSopRequest>,
        ) -> Result<tonic::Response<sisyphus_bridge_api::SopResponse>, tonic::Status> {
            Ok(tonic::Response::new(sisyphus_bridge_api::SopResponse {
                status: "SUCCESS".into(),
                result: "mock_output".into(),
            }))
        }

        async fn health_check(
            &self,
            _request: Request<HealthCheckRequest>,
        ) -> Result<tonic::Response<sisyphus_bridge_api::HealthCheckResponse>, tonic::Status>
        {
            Ok(tonic::Response::new(
                sisyphus_bridge_api::HealthCheckResponse {
                    status: "SERVING".into(),
                    version: self.0.to_string(),
                },
            ))
        }

        async fn list_agents(
            &self,
            _request: Request<ListAgentsRequest>,
        ) -> Result<tonic::Response<sisyphus_bridge_api::ListAgentsResponse>, tonic::Status>
        {
            Ok(tonic::Response::new(
                sisyphus_bridge_api::ListAgentsResponse { agents: vec![] },
            ))
        }

        async fn list_so_ps(
            &self,
            _request: Request<ListSoPsRequest>,
        ) -> Result<tonic::Response<sisyphus_bridge_api::ListSoPsResponse>, tonic::Status> {
            Ok(tonic::Response::new(
                sisyphus_bridge_api::ListSoPsResponse { sops: vec![] },
            ))
        }

        type StreamExecutionStream = tonic::codegen::BoxStream<sisyphus_bridge_api::ExecutionEvent>;

        async fn stream_execution(
            &self,
            _request: Request<ExecuteSopRequest>,
        ) -> Result<tonic::Response<Self::StreamExecutionStream>, tonic::Status> {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            tokio::spawn(async move {
                let _ = tx.send(sisyphus_bridge_api::ExecutionEvent {
                    event_type: 3,
                    node_name: "".into(),
                    status: "COMPLETE".into(),
                    message: "done".into(),
                    timestamp: 0,
                });
            });
            let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx).map(|e| Ok(e));
            Ok(tonic::Response::new(Box::pin(stream)))
        }

        async fn execute_tool(
            &self,
            request: Request<ExecuteToolRequest>,
        ) -> Result<tonic::Response<sisyphus_bridge_api::ExecuteToolResponse>, tonic::Status>
        {
            let req = request.into_inner();
            Ok(tonic::Response::new(
                sisyphus_bridge_api::ExecuteToolResponse {
                    status: "SUCCESS".into(),
                    result_json: format!("{{\"echo\": \"{}\"}}", req.tool_id),
                },
            ))
        }

        async fn decompose_goal(
            &self,
            _request: Request<DecomposeGoalRequest>,
        ) -> Result<tonic::Response<DecomposeGoalResponse>, tonic::Status> {
            Ok(tonic::Response::new(DecomposeGoalResponse {
                success: true,
                error: String::new(),
                tasks: vec![TaskProto {
                    id: "task-1".into(),
                    agent_id: "cook".into(),
                    tool_id: "bash_exec".into(),
                    args_json: "{}".into(),
                    depends_on: vec![],
                }],
            }))
        }

        async fn list_skills(
            &self,
            _request: Request<sisyphus_bridge_api::ListSkillsRequest>,
        ) -> Result<tonic::Response<sisyphus_bridge_api::ListSkillsResponse>, tonic::Status>
        {
            Ok(tonic::Response::new(
                sisyphus_bridge_api::ListSkillsResponse { skills: vec![] },
            ))
        }

        async fn show_skill(
            &self,
            _request: Request<sisyphus_bridge_api::ShowSkillRequest>,
        ) -> Result<tonic::Response<sisyphus_bridge_api::ShowSkillResponse>, tonic::Status>
        {
            Ok(tonic::Response::new(
                sisyphus_bridge_api::ShowSkillResponse {
                    found: false,
                    skill: None,
                },
            ))
        }

        async fn install_skill(
            &self,
            _request: Request<sisyphus_bridge_api::InstallSkillRequest>,
        ) -> Result<tonic::Response<sisyphus_bridge_api::InstallSkillResponse>, tonic::Status>
        {
            Ok(tonic::Response::new(
                sisyphus_bridge_api::InstallSkillResponse {
                    success: true,
                    error: String::new(),
                    name: "mock-skill".into(),
                },
            ))
        }

        async fn remove_skill(
            &self,
            _request: Request<sisyphus_bridge_api::RemoveSkillRequest>,
        ) -> Result<tonic::Response<sisyphus_bridge_api::RemoveSkillResponse>, tonic::Status>
        {
            Ok(tonic::Response::new(
                sisyphus_bridge_api::RemoveSkillResponse {
                    success: true,
                    error: String::new(),
                },
            ))
        }

        async fn list_tools(
            &self,
            _request: Request<ListToolsRequest>,
        ) -> Result<tonic::Response<ListToolsResponse>, tonic::Status> {
            Ok(tonic::Response::new(ListToolsResponse { tools: vec![] }))
        }

        async fn get_learning_insights(
            &self,
            _request: Request<LearningInsightsRequest>,
        ) -> Result<Response<LearningInsightsResponse>, Status> {
            Ok(Response::new(LearningInsightsResponse {
                total_samples: 0,
                overall_success_rate: 0.0,
                strategies: vec![],
            }))
        }

        async fn list_benchmarks(
            &self,
            _request: Request<sisyphus_bridge_api::ListBenchmarksRequest>,
        ) -> Result<tonic::Response<sisyphus_bridge_api::ListBenchmarksResponse>, tonic::Status>
        {
            Ok(tonic::Response::new(
                sisyphus_bridge_api::ListBenchmarksResponse { benchmarks: vec![] },
            ))
        }

        async fn get_breaker_metrics(
            &self,
            _request: Request<sisyphus_bridge_api::BreakerMetricsRequest>,
        ) -> Result<tonic::Response<sisyphus_bridge_api::BreakerMetricsResponse>, tonic::Status>
        {
            Ok(tonic::Response::new(
                sisyphus_bridge_api::BreakerMetricsResponse { breakers: vec![] },
            ))
        }

        async fn add_message(
            &self,
            _request: Request<AddMessageRequest>,
        ) -> Result<Response<AddMessageResponse>, Status> {
            Ok(Response::new(AddMessageResponse {
                success: true,
                error: String::new(),
                response_content: "mock response".into(),
            }))
        }
    }

    #[tokio::test]
    async fn test_health_check() {
        let bridge = TestBridge("0.1.0-test");
        let response = bridge
            .health_check(Request::new(HealthCheckRequest {}))
            .await
            .unwrap();
        let inner = response.into_inner();
        assert_eq!(inner.status, "SERVING");
        assert_eq!(inner.version, "0.1.0-test");
    }

    #[tokio::test]
    async fn test_execute_sop() {
        let bridge = TestBridge("test");
        let response = bridge
            .execute_sop(Request::new(ExecuteSopRequest {
                sop_id: "test".to_string(),
                args: HashMap::new(),
            }))
            .await
            .unwrap();
        let inner = response.into_inner();
        assert_eq!(inner.status, "SUCCESS");
        assert_eq!(inner.result, "mock_output");
    }

    #[tokio::test]
    async fn test_list_sops() {
        let bridge = TestBridge("test");
        let response = bridge
            .list_so_ps(Request::new(ListSoPsRequest {}))
            .await
            .unwrap();
        assert!(response.into_inner().sops.is_empty());
    }

    #[tokio::test]
    async fn test_stream_execution() {
        let bridge = TestBridge("test");
        let mut stream = bridge
            .stream_execution(Request::new(ExecuteSopRequest {
                sop_id: "test".to_string(),
                args: HashMap::new(),
            }))
            .await
            .unwrap()
            .into_inner();
        let event = stream.next().await.unwrap().unwrap();
        assert_eq!(event.status, "COMPLETE");
    }

    #[tokio::test]
    async fn test_execute_tool() {
        let bridge = TestBridge("test");
        let response = bridge
            .execute_tool(Request::new(ExecuteToolRequest {
                tool_id: "metis/analyse".to_string(),
                args_json: r#"{"request": "test"}"#.to_string(),
                fuel_limit: 0,
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(response.status, "SUCCESS");
        assert!(response.result_json.contains("metis/analyse"));
    }
}
