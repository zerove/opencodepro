use sisyphus_bridge_api::sisyphus_bridge_server::SisyphusBridgeServer;
use sisyphus_metrics::init_metrics;
use sisyphus_tools_system::config_store::ConfigStore;
use sisyphus_types::Config;
use std::sync::{Arc, RwLock};
use tokio::signal;
use tokio_stream::wrappers::TcpListenerStream;

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
    // ADR-019：依赖图可能同时启用 ring/aws-lc-rs；显式安装 rustls provider，
    // 否则启用 TLS 时握手会 panic（fail-closed 的前提下也不允许运行期崩溃）。
    sisyphus_bridge::peer::tls::ensure_rustls_crypto_provider();
    let config_path =
        std::env::var("SISYPHUS_CONFIG").unwrap_or_else(|_| "config.toml".to_string());

    let (_store, shared_config) = match ConfigStore::load(&config_path) {
        Ok(s) => {
            info!(path = %config_path, "Loaded configuration with hot-reload");
            s
        },
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
        },
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
    let otel_enabled =
        shared_config.read().map(|c| c.metrics.opentelemetry.enabled).unwrap_or(false);

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

    // ── Cluster peer protocol (TODO 8.3.1 Phase B) ───────────────────────
    // 必须在桥接构造之前完成：内核的经验库要拿到 peer 管理器才能把本地
    // 新增经验广播出去，而内核由 MySisyphusBridge::new 内部构造。
    let (cluster_runtime, cluster_server_tls): (
        Option<sisyphus_bridge::bridge_service::ClusterRuntime>,
        Option<tonic::transport::ServerTlsConfig>,
    ) = {
        use sisyphus_bridge::peer::PeerManager;

        let cfg = match shared_config.read() {
            Ok(c) => c.cluster.clone(),
            Err(_) => sisyphus_types::ClusterConfig::default(),
        };
        if !cfg.enabled {
            info!(
                static_peers = cfg.peers.len(),
                "Cluster peer protocol disabled (set [cluster].enabled = true to enable)"
            );
            (None, None)
        } else {
            let derived = std::env::var("SISYPHUS_CLUSTER_NODE_ID")
                .ok()
                .or_else(|| std::env::var("HOSTNAME").ok())
                .unwrap_or_else(|| "local-node".to_string());
            let self_node_id = if cfg.node_id.is_empty() {
                derived
            } else {
                cfg.node_id.clone()
            };
            let peer_ttl = std::time::Duration::from_secs(cfg.peer_ttl_secs);
            let manager = std::sync::Arc::new(PeerManager::new(self_node_id.clone(), peer_ttl));
            let tls_material = sisyphus_bridge::peer::load_control_plane_tls(&cfg.control_plane);
            let control_plane_tls_wired = tls_material.wired;
            if cfg.control_plane.tls_enabled && !tls_material.wired {
                tracing::error!(
                    note = %tls_material.note,
                    "cluster.control_plane.tls_enabled=true but TLS material not wired; refusing to arm control plane (fail-closed)"
                );
            }
            // ADR-019：SubmitControlLease/ProposeControl 鉴权策略（fail-closed）。
            let auth_mode_ok = match cfg.control_plane.submit_auth_mode.as_str() {
                "none" => true,
                "mtls_subject_allowlist" => {
                    if !cfg.control_plane.tls_enabled || !tls_material.wired {
                        if cfg.control_plane.enabled {
                            tracing::error!(
                                "mtls_subject_allowlist requires wired mTLS; refusing to arm control plane (fail-closed)"
                            );
                        }
                        false
                    } else if cfg.control_plane.allowed_submit_subjects.is_empty() {
                        if cfg.control_plane.enabled {
                            tracing::error!(
                                "mtls_subject_allowlist requires non-empty allowed_submit_subjects; refusing to arm control plane (fail-closed)"
                            );
                        }
                        false
                    } else {
                        true
                    }
                },
                other => {
                    if cfg.control_plane.enabled {
                        tracing::error!(mode = %other, "unknown cluster.control_plane.submit_auth_mode; refusing to arm control plane (fail-closed)");
                    }
                    false
                },
            };
            let tls_ok = !cfg.control_plane.tls_enabled || tls_material.wired;
            let control_plane_effectively_armed =
                cfg.control_plane.enabled && tls_ok && auth_mode_ok;
            let (control_plane_state, control_plane_coordinator) =
                if control_plane_effectively_armed {
                    use sisyphus_bridge::peer::control_plane::{
                        ControlPlaneCoordinator, ControlPlaneState,
                    };
                    if cfg.control_plane.tls_enabled && tls_material.wired {
                        tracing::warn!(
                            note = %tls_material.note,
                            "cluster.control_plane mTLS wired (transport). submit_auth_mode=none: no app-layer SubmitControlLease subject allowlist enforcement; controlled environments only, requires ADR-019 dedicated-runner evidence."
                        );
                    } else {
                        tracing::warn!(
                        "cluster.control_plane.enabled=true (no TLS/auth enforcement): ADR-019 gate on; production multi-node quorum requires full admission criteria (resolver/member-change/chaos/rollback/TLS/auth evidence)."
                    );
                    }
                    let state = std::sync::Arc::new(ControlPlaneState::new());
                    let coord =
                        std::sync::Arc::new(ControlPlaneCoordinator::with_timeouts_and_tls(
                            manager.clone(),
                            state.clone(),
                            std::time::Duration::from_millis(
                                cfg.control_plane.propose_timeout_ms.max(1),
                            ),
                            std::time::Duration::from_millis(
                                cfg.control_plane.connect_timeout_ms.max(1),
                            ),
                            tls_material.client.clone(),
                        ));
                    (Some(state), Some(coord))
                } else {
                    (None, None)
                };
            info!(
                node_id = %self_node_id,
                cluster = %cfg.cluster_name,
                peer_ttl_secs = cfg.peer_ttl_secs,
                static_peers = cfg.peers.len(),
                control_plane = cfg.control_plane.enabled,
                control_plane_tls_wired,
                control_plane_armed = control_plane_state.is_some(),
                "Cluster peer protocol enabled (peer manager created before kernel)"
            );
            let server_tls = tls_material.server.clone();
            (
                Some(sisyphus_bridge::bridge_service::ClusterRuntime {
                    peer_manager: manager,
                    node_id: self_node_id,
                    cluster_config: cfg,
                    control_plane_tls_wired,
                    control_plane_state,
                    control_plane_coordinator,
                }),
                server_tls,
            )
        }
    };

    // Initialize bridge with shared config for dynamic updates
    let bridge = Arc::new(
        MySisyphusBridge::new(shared_config.clone(), None, cluster_runtime.clone()).await?,
    );

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
    let http_stats_provider = bridge.http_stats_provider();
    let rest_state = if !jwt_config.secret.is_empty() {
        crate::http::RestApiState::new_with_jwt(bridge.clone(), data_dir, jwt_config)
    } else {
        crate::http::RestApiState::new(bridge.clone(), data_dir)
    };
    let rest_state = match http_stats_provider {
        Some(provider) => rest_state.with_http_stats(provider),
        None => rest_state,
    };

    let http_app = crate::http::build_router(rest_state)
        .layer(tower_http::cors::CorsLayer::permissive())
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let http_addr: std::net::SocketAddr =
        http_addr_str.strip_prefix("http://").unwrap_or(&http_addr_str).parse().map_err(|e| {
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
            },
            Err(e) => {
                // 无法安装 SIGTERM handler: 记录并继续 (SIGINT 仍可工作)
                error!(error = %e, "Failed to install SIGTERM handler; SIGINT only");
            },
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
            },
            Err(e) => {
                error!("Failed to bind HTTP listener on {http_addr}: {e}");
            },
        }
    });

    // ── Start gRPC server ───────────────────────────────────────────────
    let auth_interceptor = auth::AuthInterceptor::new(auth_token);
    let mut server_builder = tonic::transport::Server::builder();
    if let Some(tls) = cluster_server_tls {
        server_builder = server_builder.tls_config(tls).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("cluster control plane TLS server config failed (fail-closed): {e}"),
            )
        })?;
        info!("gRPC server TLS enabled (cluster control plane mTLS material; applies to shared gRPC port)");
    }
    let mut grpc_server = server_builder.add_service(SisyphusBridgeServer::with_interceptor(
        (*bridge).clone(),
        auth_interceptor,
    ));

    // Try IPv6 (::1) first; fall back to IPv4 (127.0.0.1) on systems without IPv6 loopback
    let grpc_listener = match tokio::net::TcpListener::bind(grpc_addr).await {
        Ok(l) => l,
        Err(_) => {
            let v4_addr: std::net::SocketAddr =
                format!("127.0.0.1:{}", grpc_addr.port()).parse().map_err(|e| {
                    Box::new(std::io::Error::new(std::io::ErrorKind::InvalidInput, e))
                })?;
            warn!("Failed to bind gRPC on {grpc_addr}, falling back to {v4_addr}");
            tokio::net::TcpListener::bind(v4_addr).await?
        },
    };
    let actual_grpc_addr = grpc_listener.local_addr().unwrap_or(grpc_addr);
    let grpc_incoming = TcpListenerStream::new(grpc_listener);

    // ── Cluster service registration (peer manager 已在桥接构造前创建) ──
    if let Some(runtime) = cluster_runtime.as_ref() {
        use sisyphus_bridge::peer::ExperienceSyncHandler;
        use sisyphus_bridge::peer::{
            spawn_static_peers_with_catchup, ClusterServiceImpl, StaticPeerConfig,
        };

        let cfg = match shared_config.read() {
            Ok(c) => c.cluster.clone(),
            Err(_) => sisyphus_types::ClusterConfig::default(),
        };
        // ADR-010：service 挂上经验库；ADR-019：复用 runtime 装配的控制面 state/coordinator（避免双份状态）。
        let experience_bank = bridge.experience_bank().clone();
        let control_plane_service_enabled = cfg.control_plane.enabled
            && runtime.control_plane_state.is_some()
            && runtime.control_plane_coordinator.is_some();
        let service = (if let (Some(cp), Some(coord)) = (
            runtime.control_plane_state.clone(),
            runtime.control_plane_coordinator.clone(),
        ) {
            ClusterServiceImpl::with_control_plane_coordinator_and_bounds(
                runtime.peer_manager.clone(),
                Some(experience_bank.clone()),
                cp,
                coord,
                control_plane_service_enabled,
                cfg.control_plane.lease_ttl_min_secs,
                cfg.control_plane.lease_ttl_max_secs,
            )
        } else if let Some(cp) = runtime.control_plane_state.clone() {
            ClusterServiceImpl::with_control_plane(
                runtime.peer_manager.clone(),
                Some(experience_bank.clone()),
                cp,
                control_plane_service_enabled,
            )
        } else {
            ClusterServiceImpl::with_experience_bank(
                runtime.peer_manager.clone(),
                experience_bank.clone(),
            )
        })
        .with_control_plane_auth(
            cfg.control_plane.submit_auth_mode.clone(),
            cfg.control_plane.allowed_submit_subjects.clone(),
        );
        grpc_server = grpc_server.add_service(
            sisyphus_bridge_api::cluster_proto::cluster_server::ClusterServer::new(service),
        );
        let self_grpc_addr = format!("http://{actual_grpc_addr}");
        let mut dial_cfg =
            StaticPeerConfig::from_types(&cfg, runtime.node_id.clone(), self_grpc_addr);
        // ADR-010：拨号器连接建立后据此发起反熵拉取（补齐断连期间的丢失）。
        dial_cfg.experience_bank = Some(experience_bank.clone());

        // ADR-011：入站通道 Lagged 时的补齐信号。
        // 通道容量 1 足够：信号语义是幂等的「再拉一次」，即使多次 Lagged
        // 合并成一次补齐，反熵也会按摘要算出完整差量（摘要记空洞，不记次数）。
        // 反过来不设时间节流：节流窗口里发生的丢失会被压制到下一次信号之后，
        // 而那次信号未必再来——Lagged 只在溢出那一刻发一次。
        let (catchup_tx, catchup_rx) = tokio::sync::broadcast::channel::<()>(1);
        let handles = spawn_static_peers_with_catchup(
            dial_cfg,
            runtime.peer_manager.clone(),
            Some(catchup_rx),
        );

        // 经验同步接收端：把 peer 广播来的经验按 CRDT 合并进本地库。
        // 处理器持有的是未装饰的 VectorExperienceBank，远程应用不会再次广播。
        let (sync_shutdown_tx, sync_shutdown_rx) = tokio::sync::broadcast::channel(1);
        // 联邦经验聚合（v0.4.0 Phase A）：默认关闭，开启后低质入站经验在
        // CRDT 合并前被丢弃。关闭时完全不接入，行为与 v0.3.0 一致。
        let sync_handler = ExperienceSyncHandler::new(
            experience_bank,
            runtime.peer_manager.clone(),
            runtime.node_id.clone(),
        )
        .with_catchup_signal(catchup_tx);
        let sync_handler = if cfg.federated.enabled {
            info!(
                min_quality_score = cfg.federated.min_quality_score,
                success_weight = cfg.federated.success_weight,
                deviation_weight = cfg.federated.deviation_weight,
                "Federated experience aggregation enabled"
            );
            sync_handler.with_federated_aggregation(cfg.federated.clone())
        } else {
            sync_handler
        };
        let sync_handle = sync_handler.spawn(sync_shutdown_rx);

        // 联邦策略权重同步（v0.4.0 Phase B，ADR-017）：周期性发布本节点策略快照，
        // 消费对端 StateProposal 并合并进共享视图。默认周期 60s；0 = 只收不发。
        // 关闭时与经验同步处理器一样跟随 sig_rx 通知退出。
        let strategy_weights = bridge.strategy_weights().clone();
        let strategy_sync_interval_secs = cfg.strategy_sync_interval_secs;
        let (strategy_shutdown_tx, strategy_shutdown_rx) = tokio::sync::broadcast::channel(1);
        let strategy_handler = if strategy_sync_interval_secs == 0 {
            None
        } else {
            Some(
                sisyphus_bridge::peer::StrategyWeightsHandler::new(
                    runtime.peer_manager.clone(),
                    runtime.node_id.clone(),
                    bridge.kernel().meta_learner.clone(),
                    strategy_weights,
                )
                .with_interval(std::time::Duration::from_secs(strategy_sync_interval_secs)),
            )
        };
        let strategy_handle = strategy_handler.map(|h| h.spawn(strategy_shutdown_rx));

        info!(
            node_id = %runtime.node_id,
            cluster = %cfg.cluster_name,
            static_peers = cfg.peers.len(),
            dialer_handles = handles.len(),
            "ClusterService registered, static dialers and experience sync started"
        );

        // 跟随既有关闭信号：先通知处理器停止，等待在途合并完成再返回，
        // 避免进程退出时截断正在写入的远程经验。
        let mut sync_sig_rx = sig_rx.clone();
        tokio::spawn(async move {
            let _ = sync_sig_rx.changed().await;
            let _ = sync_shutdown_tx.send(());
            let _ = tokio::time::timeout(std::time::Duration::from_secs(5), sync_handle).await;
            if let Some(handle) = strategy_handle {
                let _ = strategy_shutdown_tx.send(());
                let _ = tokio::time::timeout(std::time::Duration::from_secs(5), handle).await;
            }
        });
    }

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
        let response = bridge.health_check(Request::new(HealthCheckRequest {})).await.unwrap();
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
        let response = bridge.list_so_ps(Request::new(ListSoPsRequest {})).await.unwrap();
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
