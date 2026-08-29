use sisyphus_bridge_api::sisyphus_bridge_server::SisyphusBridgeServer;
use sisyphus_metrics::init_metrics;
use sisyphus_tools_system::config_store::ConfigStore;
use sisyphus_types::Config;
use std::sync::{Arc, RwLock};
use tokio::signal;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;
use tracing::{error, info, warn};

// OpenTelemetry
use opentelemetry::global;
use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Registry};

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
            let mut cfg = Config::default();
            // Merge environment overrides (SISYPHUS_*) so CI/dev setups that
            // run without a config file still honor env vars such as the
            // chaos-injection switches.
            cfg.apply_env_overrides();
            let cfg = Arc::new(RwLock::new(cfg));
            // Create a no-op store that never reloads
            let store = ConfigStore::with_config(cfg.clone());
            (store, cfg)
        }
    };

    // ── OpenTelemetry initialization ─────────────────────────────────────
    let otel_endpoint = shared_config
        .read()
        .map(|c| c.metrics.opentelemetry.endpoint.clone())
        .unwrap_or_else(|_| "http://localhost:4317".to_string());
    let service_name = shared_config
        .read()
        .map(|c| c.metrics.opentelemetry.service_name.clone())
        .unwrap_or_else(|_| "sisyphus-bridge".to_string());
    let otel_enabled = shared_config
        .read()
        .map(|c| c.metrics.opentelemetry.enabled)
        .unwrap_or(false);

    if otel_enabled {
        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_tonic()
            .with_endpoint(otel_endpoint.clone())
            .build()?;
        let resource = Resource::builder()
            .with_attributes([
                KeyValue::new("service.name", service_name.clone()),
                KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
            ])
            .build();
        let provider = SdkTracerProvider::builder()
            .with_batch_exporter(exporter)
            .with_resource(resource)
            .build();
        global::set_tracer_provider(provider);

        info!("OpenTelemetry initialized: endpoint={}", otel_endpoint);
    } else {
        info!("OpenTelemetry disabled (enable via config metrics.opentelemetry.enabled=true)");
    }

    // Initialize tracing (with OpenTelemetry layer when enabled)
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let fmt_layer = tracing_subscriber::fmt::layer();
    let registry = Registry::default().with(filter).with(fmt_layer);
    if otel_enabled {
        let otel_layer = tracing_opentelemetry::layer();
        registry.with(otel_layer).try_init()?;
    } else {
        registry.try_init()?;
    }

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

    // Get JWT config from shared config
    let jwt_config = {
        let config = shared_config.read().map_err(|e| {
            Box::new(std::io::Error::other(format!("Config lock poisoned: {e}")))
                as Box<dyn std::error::Error>
        })?;
        config.jwt.clone()
    };

    // 桥接持有的 shared_http（来自 LLM client 注册），暴露给 /api/metrics
    let shared_http = bridge.shared_http();
    let rest_state = if !jwt_config.secret.is_empty() {
        crate::http::RestApiState::new_with_jwt(bridge.clone(), data_dir, jwt_config)
    } else {
        crate::http::RestApiState::new(bridge.clone(), data_dir)
    };
    let rest_state = match shared_http {
        Some(client) => rest_state.with_shared_http(client),
        None => rest_state,
    };

    let http_app = crate::http::build_router(rest_state)
        .layer(tower_http::cors::CorsLayer::permissive())
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let http_addr: std::net::SocketAddr = http_addr_str
        .strip_prefix("http://")
        .unwrap_or(&http_addr_str)
        .parse()
        .map_err(|e| {
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidInput, e))
                as Box<dyn std::error::Error>
        })?;
    // ── 协调关闭通道：收到 SIGINT/SIGTERM 后通知 HTTP/gRPC/LLM 顺序停机 ──
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let mut sig_rx = shutdown_rx.clone();

    tokio::spawn(async move {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sigterm) => {
                tokio::select! {
                    _ = signal::ctrl_c() => info!("Received SIGINT"),
                    _ = sigterm.recv() => info!("Received SIGTERM"),
                }
                let _ = shutdown_tx.send(true);
            }
            Err(e) => {
                // 无法安装 SIGTERM handler: 记录并继续 (SIGINT 仍可工作)
                error!(error = %e, "Failed to install SIGTERM handler; SIGINT only");
            }
        }
    });

    info!("Starting HTTP REST API server on {http_addr}");

    let mut http_shutdown_rx = sig_rx.clone();
    let _http_handle = tokio::spawn(async move {
        match tokio::net::TcpListener::bind(http_addr).await {
            Ok(listener) => {
                if let Err(e) = axum::serve(listener, http_app)
                    .with_graceful_shutdown(async move {
                        // 等待 shutdown 信号
                        let _ = http_shutdown_rx.changed().await;
                        info!("HTTP graceful shutdown initiated (waiting up to 5s for in-flight requests)");
                        // 留出 5s 给 in-flight HTTP 请求完成
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
    let mut grpc_shutdown_rx = sig_rx.clone();
    grpc_server
        .serve_with_incoming_shutdown(grpc_incoming, async move {
            let _ = grpc_shutdown_rx.changed().await;
            info!("gRPC graceful shutdown initiated (waiting up to 5s for in-flight RPCs)");
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        })
        .await?;

    // 等待 shutdown 信号
    let _ = sig_rx.changed().await;
    info!("Bridge shutdown signal received, calling bridge.shutdown(5s)");
    if let Err(e) = bridge.shutdown(std::time::Duration::from_secs(5)).await {
        warn!("Bridge shutdown error: {e}");
    } else {
        info!("Bridge shutdown complete");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use sisyphus_bridge_api::{
        sisyphus_bridge_server::SisyphusBridge, AddMessageRequest, AddMessageResponse,
        CallJcodeRequest, CallJcodeResponse, CaptureExperienceRequest, CaptureExperienceResponse,
        DecomposeGoalRequest, DecomposeGoalResponse, DelegateCognitiveTaskRequest,
        DeleteWorkspaceRequest, DeleteWorkspaceResponse, EventRequest, ExecuteSopRequest,
        ExecuteToolRequest, GetCurrentWorkspaceRequest, GetCurrentWorkspaceResponse,
        HealthCheckRequest, LearningInsightsRequest, LearningInsightsResponse, ListAgentsRequest,
        ListPluginsRequest, ListPluginsResponse, ListSoPsRequest, ListToolsRequest,
        ListToolsResponse, ListWorkspacesRequest, ListWorkspacesResponse, LoadPluginRequest,
        LoadPluginResponse, SubmitFeedbackRequest, SubmitFeedbackResponse, TaskProto,
        UnloadPluginRequest, UnloadPluginResponse,
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
            let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx).map(Ok);
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

        // 以下是缺失的 trait 方法实现
        async fn delegate_cognitive_task(
            &self,
            _request: Request<DelegateCognitiveTaskRequest>,
        ) -> Result<
            tonic::Response<sisyphus_bridge_api::DelegateCognitiveTaskResponse>,
            tonic::Status,
        > {
            Ok(tonic::Response::new(
                sisyphus_bridge_api::DelegateCognitiveTaskResponse {
                    success: true,
                    error: String::new(),
                    plan_json: "{}".into(),
                },
            ))
        }

        type StreamCognitiveTaskStream =
            tonic::codegen::BoxStream<sisyphus_bridge_api::CognitiveTaskEvent>;

        async fn stream_cognitive_task(
            &self,
            _request: Request<DelegateCognitiveTaskRequest>,
        ) -> Result<tonic::Response<Self::StreamCognitiveTaskStream>, tonic::Status> {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            tokio::spawn(async move {
                let _ = tx.send(Ok(sisyphus_bridge_api::CognitiveTaskEvent {
                    event_type: 4,
                    payload: "done".into(),
                    timestamp: 0,
                }));
            });
            let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
            Ok(tonic::Response::new(Box::pin(stream)))
        }

        async fn submit_feedback(
            &self,
            _request: Request<SubmitFeedbackRequest>,
        ) -> Result<tonic::Response<SubmitFeedbackResponse>, tonic::Status> {
            Ok(tonic::Response::new(SubmitFeedbackResponse {
                success: true,
                error: String::new(),
                evolution_json: "{}".into(),
            }))
        }

        async fn capture_experience(
            &self,
            _request: Request<CaptureExperienceRequest>,
        ) -> Result<tonic::Response<CaptureExperienceResponse>, tonic::Status> {
            Ok(tonic::Response::new(CaptureExperienceResponse {
                success: true,
                error: String::new(),
            }))
        }

        async fn create_workspace(
            &self,
            _request: Request<sisyphus_bridge_api::CreateWorkspaceRequest>,
        ) -> Result<tonic::Response<sisyphus_bridge_api::CreateWorkspaceResponse>, tonic::Status>
        {
            Ok(tonic::Response::new(
                sisyphus_bridge_api::CreateWorkspaceResponse {
                    success: true,
                    error: String::new(),
                    workspace_id: "mock-workspace".into(),
                },
            ))
        }

        async fn switch_workspace(
            &self,
            _request: Request<sisyphus_bridge_api::SwitchWorkspaceRequest>,
        ) -> Result<tonic::Response<sisyphus_bridge_api::SwitchWorkspaceResponse>, tonic::Status>
        {
            Ok(tonic::Response::new(
                sisyphus_bridge_api::SwitchWorkspaceResponse {
                    success: true,
                    error: String::new(),
                },
            ))
        }

        async fn list_workspaces(
            &self,
            _request: Request<ListWorkspacesRequest>,
        ) -> Result<tonic::Response<ListWorkspacesResponse>, tonic::Status> {
            Ok(tonic::Response::new(ListWorkspacesResponse {
                workspaces: vec![],
            }))
        }

        async fn get_current_workspace(
            &self,
            _request: Request<GetCurrentWorkspaceRequest>,
        ) -> Result<tonic::Response<GetCurrentWorkspaceResponse>, tonic::Status> {
            Ok(tonic::Response::new(GetCurrentWorkspaceResponse {
                found: false,
                workspace: None,
            }))
        }

        async fn delete_workspace(
            &self,
            _request: Request<DeleteWorkspaceRequest>,
        ) -> Result<tonic::Response<DeleteWorkspaceResponse>, tonic::Status> {
            Ok(tonic::Response::new(DeleteWorkspaceResponse {
                success: true,
                error: String::new(),
            }))
        }

        async fn call_jcode(
            &self,
            _request: Request<CallJcodeRequest>,
        ) -> Result<tonic::Response<CallJcodeResponse>, tonic::Status> {
            Ok(tonic::Response::new(CallJcodeResponse {
                success: true,
                result: "mock".into(),
                error: String::new(),
            }))
        }

        type StreamEventsStream = tonic::codegen::BoxStream<sisyphus_bridge_api::BridgeEvent>;

        async fn stream_events(
            &self,
            _request: Request<EventRequest>,
        ) -> Result<tonic::Response<Self::StreamEventsStream>, tonic::Status> {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            tokio::spawn(async move {
                let _ = tx.send(Ok(sisyphus_bridge_api::BridgeEvent {
                    event: Some(sisyphus_bridge_api::bridge_event::Event::LogUpdate(
                        sisyphus_bridge_api::LogUpdate {
                            message: "mock".into(),
                            level: "INFO".into(),
                        },
                    )),
                }));
            });
            let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
            Ok(tonic::Response::new(Box::pin(stream)))
        }

        // ── Chaos Engineering (mock stubs) ──────────────────────────

        async fn start_chaos(
            &self,
            _request: Request<sisyphus_bridge_api::ChaosStartRequest>,
        ) -> Result<tonic::Response<sisyphus_bridge_api::ChaosStartResponse>, tonic::Status>
        {
            Ok(tonic::Response::new(
                sisyphus_bridge_api::ChaosStartResponse {
                    success: true,
                    experiment_id: "mock-experiment".into(),
                    error: String::new(),
                },
            ))
        }

        async fn stop_chaos(
            &self,
            _request: Request<sisyphus_bridge_api::ChaosStopRequest>,
        ) -> Result<tonic::Response<sisyphus_bridge_api::ChaosStopResponse>, tonic::Status>
        {
            Ok(tonic::Response::new(
                sisyphus_bridge_api::ChaosStopResponse {
                    success: true,
                    result: Some(sisyphus_bridge_api::ChaosExperimentResult {
                        name: "mock".into(),
                        operations_attempted: 0,
                        operations_succeeded: 0,
                        operations_failed: 0,
                        faults_injected: 0,
                        success_rate: 1.0,
                    }),
                    error: String::new(),
                },
            ))
        }

        async fn get_chaos_status(
            &self,
            _request: Request<sisyphus_bridge_api::ChaosStatusRequest>,
        ) -> Result<tonic::Response<sisyphus_bridge_api::ChaosStatusResponse>, tonic::Status>
        {
            Ok(tonic::Response::new(
                sisyphus_bridge_api::ChaosStatusResponse {
                    running: false,
                    result: None,
                },
            ))
        }

        type StreamAnomalyReportStream =
            tonic::codegen::BoxStream<sisyphus_bridge_api::AnomalyReport>;

        async fn stream_anomaly_report(
            &self,
            _request: Request<sisyphus_bridge_api::AnomalyRequest>,
        ) -> Result<tonic::Response<Self::StreamAnomalyReportStream>, tonic::Status> {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            tokio::spawn(async move {
                let _ = tx.send(Ok(sisyphus_bridge_api::AnomalyReport {
                    node_name: "mock_node".into(),
                    severity: "Low".into(),
                    metric: "fuel".into(),
                    observed: 0.0,
                    expected: 0.0,
                    description: "no anomalies".into(),
                    timestamp: 0,
                }));
            });
            let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
            Ok(tonic::Response::new(Box::pin(stream)))
        }

        // ── Experience Sharing (mock stubs) ───────────────────────────

        async fn share_experience(
            &self,
            _request: Request<sisyphus_bridge_api::ShareExperienceRequest>,
        ) -> Result<tonic::Response<sisyphus_bridge_api::ShareExperienceResponse>, tonic::Status>
        {
            Ok(tonic::Response::new(
                sisyphus_bridge_api::ShareExperienceResponse {
                    success: true,
                    error: String::new(),
                },
            ))
        }

        async fn pull_experience(
            &self,
            _request: Request<sisyphus_bridge_api::PullExperienceRequest>,
        ) -> Result<tonic::Response<sisyphus_bridge_api::PullExperienceResponse>, tonic::Status>
        {
            Ok(tonic::Response::new(
                sisyphus_bridge_api::PullExperienceResponse {
                    success: true,
                    error: String::new(),
                    experiences: vec![],
                },
            ))
        }

        async fn list_shared_experiences(
            &self,
            _request: Request<sisyphus_bridge_api::ListSharedExperiencesRequest>,
        ) -> Result<
            tonic::Response<sisyphus_bridge_api::ListSharedExperiencesResponse>,
            tonic::Status,
        > {
            Ok(tonic::Response::new(
                sisyphus_bridge_api::ListSharedExperiencesResponse {
                    success: true,
                    error: String::new(),
                    experiences: vec![],
                },
            ))
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
