use sisyphus_bridge::sisyphus_bridge_server::{SisyphusBridge, SisyphusBridgeServer};
use sisyphus_bridge::{
    ExecuteSopRequest, ExecuteToolRequest, ExecuteToolResponse,
    HealthCheckRequest, HealthCheckResponse, ListSoPsRequest, ListSoPsResponse,
    SopResponse, ExecutionEvent,
};
use sisyphus_core::{SOPGraph, SisyphusKernel, WasmSOPNode};
use sisyphus_intel::{SisyphusIntel, StandardReflector};
use sisyphus_mem::MfsStore;
use sisyphus_metrics::{init_metrics, record_bridge_request, record_sop_execution};
use sisyphus_sandbox::WasmToolExecutor;
use sisyphus_tools::{BuiltinToolExecutor, ConfigStore, MCPToolExecutor};
use sisyphus_types::{Config, ExecutorRouter, ToolExecutor};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Instant;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tonic::{transport::Server, Request, Response, Status};
use tracing::{info, info_span, warn};

pub mod sisyphus_bridge {
    tonic::include_proto!("sisyphus.bridge");
}

pub struct MySisyphusBridge {
    kernel: Arc<SisyphusKernel>,
    default_fuel: u64,
}

impl MySisyphusBridge {
    pub async fn new(config: &Config) -> Self {
        let state_store = Arc::new(
            MfsStore::new(&config.cache.sqlite_path)
                .await
                .expect("Failed to init MFS Store"),
        );

        let sim_iterations = config.executor.max_concurrent_tasks.max(1) * 64;
        let simulator = Arc::new(sisyphus_intel::MonteCarloSimulator::with_state_store(
            sim_iterations,
            state_store.clone(),
        ));

        // Primary executor: WASM (backward-compatible default)
        let wasm_executor = Arc::new(
            WasmToolExecutor::with_config(
                Some(PathBuf::from(&config.sandbox.wasm_tools_dir)),
                Some(config.sandbox.max_memory_pages),
            )
            .expect("Failed to create WasmToolExecutor"),
        );

        // Secondary executor: built-in tools
        let builtin_executor: Arc<dyn ToolExecutor> = Arc::new(BuiltinToolExecutor);

        // Build router: WASM = default (backward compat), builtin = "builtin:" prefix
        let mut router = ExecutorRouter::new(wasm_executor.clone());
        router.register("builtin", builtin_executor);

        // Optional MCP executors
        for (prefix, command_line) in &config.mcp.stdio_servers {
            let parts: Vec<&str> = command_line.split_whitespace().collect();
            if parts.is_empty() {
                warn!("Empty MCP stdio command for prefix '{prefix}', skipping");
                continue;
            }
            let cmd = parts[0];
            let args: Vec<&str> = parts[1..].to_vec();
            match MCPToolExecutor::from_stdio(cmd, &args).await {
                Ok(mcp) => {
                    info!("Connected MCP server '{prefix}' via stdio: {command_line}");
                    router.register(prefix, Arc::new(mcp));
                }
                Err(e) => {
                    warn!("Failed to start MCP server for prefix '{prefix}': {e}");
                }
            }
        }
        for (prefix, url) in &config.mcp.http_servers {
            match MCPToolExecutor::from_url(url).await {
                Ok(mcp) => {
                    info!("Connected MCP server '{prefix}' via HTTP: {url}");
                    router.register(prefix, Arc::new(mcp));
                }
                Err(e) => {
                    warn!("Failed to connect MCP server for prefix '{prefix}' at {url}: {e}");
                }
            }
        }

        let tool_executor: Arc<dyn ToolExecutor> = Arc::new(router);
        let reflector = Arc::new(StandardReflector);
        let evolver = Arc::new(sisyphus_intel::StandardEvolver::new());
        let intel = Arc::new(SisyphusIntel::new(simulator, reflector, evolver));

        let synthesizer = Arc::new(sisyphus_intel::GoalDrivenSynthesizer::new(
            tool_executor.clone(),
        ));

        let kernel = Arc::new(SisyphusKernel::new(
            state_store,
            tool_executor,
            intel,
            synthesizer,
        ));

        Self {
            kernel,
            default_fuel: config.executor.default_fuel,
        }
    }

    async fn run_execution(
        &self,
        sop_id: String,
        event_tx: Option<mpsc::UnboundedSender<sisyphus_types::ExecutionEvent>>,
    ) -> Result<(String, String), Status> {
        let start = Instant::now();
        let result = if let Some(goal) = sop_id.strip_prefix("goal:") {
            let synthesized_sop_id = format!("synth_{}", goal);
            match self.kernel.execute_goal(goal, &synthesized_sop_id, event_tx).await {
                Ok((result, report)) => {
                    let duration = start.elapsed();
                    record_sop_execution(&synthesized_sop_id, true, duration.as_secs_f64());
                    Ok((format!("Result: {} | Cognition: {}", result, report), synthesized_sop_id))
                }
                Err(e) => {
                    let duration = start.elapsed();
                    record_sop_execution(&synthesized_sop_id, false, duration.as_secs_f64());
                    Err(Status::internal(e))
                }
            }
        } else {
            let graph = match self.kernel.load_sop(&sop_id).await {
                Ok(g) => g,
                Err(_) => {
                    let mut g = SOPGraph::new();
                    let tool_id = sop_id.clone();
                    let fuel = self.default_fuel;
                    let node = Arc::new(WasmSOPNode {
                        name: format!("node_{}", tool_id),
                        tool_id,
                        executor: self.kernel.tool_executor.clone(),
                        fuel_limit: fuel,
                    });
                    g.add_node(node);
                    g
                }
            };

            let start_node = graph
                .graph
                .node_indices()
                .next()
                .ok_or_else(|| Status::internal("Graph empty"))?;

            match self
                .kernel
                .execute_with_cognition(&graph, start_node, &sop_id, None, event_tx)
                .await
            {
                Ok((result, report, evolved_graph)) => {
                    let duration = start.elapsed();
                    record_sop_execution(&sop_id, true, duration.as_secs_f64());
                    if evolved_graph.is_some() {
                        info!("SOP evolved for sop_id={}", sop_id);
                    }
                    Ok((format!("Result: {} | Cognition: {}", result, report), sop_id))
                }
                Err(e) => {
                    let duration = start.elapsed();
                    record_sop_execution(&sop_id, false, duration.as_secs_f64());
                    Err(Status::internal(e))
                }
            }
        };
        result
    }
}

#[tonic::async_trait]
impl SisyphusBridge for MySisyphusBridge {
    async fn execute_sop(
        &self,
        request: Request<ExecuteSopRequest>,
    ) -> Result<Response<SopResponse>, Status> {
        let req = request.into_inner();
        let sop_id = req.sop_id.clone();

        let span = info_span!("bridge.execute_sop", sop_id = %sop_id);
        let _enter = span.enter();

        let (result_str, _final_id) = self.run_execution(sop_id, None).await?;

        record_bridge_request(true, 0.0);

        Ok(Response::new(SopResponse {
            status: "SUCCESS".into(),
            result: result_str,
        }))
    }

    async fn health_check(
        &self,
        _request: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        Ok(Response::new(HealthCheckResponse {
            status: "SERVING".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }))
    }

    async fn list_so_ps(
        &self,
        _request: Request<ListSoPsRequest>,
    ) -> Result<Response<ListSoPsResponse>, Status> {
        Ok(Response::new(ListSoPsResponse {
            sops: vec![],
        }))
    }

    type StreamExecutionStream = tonic::codegen::BoxStream<ExecutionEvent>;

    async fn stream_execution(
        &self,
        request: Request<ExecuteSopRequest>,
    ) -> Result<Response<Self::StreamExecutionStream>, Status> {
        let req = request.into_inner();
        let sop_id = req.sop_id.clone();

        // Channel for domain events from kernel → stream
        let (tx, rx) = mpsc::unbounded_channel::<sisyphus_types::ExecutionEvent>();

        let kernel = self.kernel.clone();
        let default_fuel = self.default_fuel;

        tokio::spawn(async move {
            let _result: Result<(String, String), String> = if let Some(goal) = sop_id.strip_prefix("goal:") {
                let synthesized_sop_id = format!("synth_{}", goal);
                kernel.execute_goal(goal, &synthesized_sop_id, Some(tx.clone())).await
                    .map(|(r, report)| (format!("Result: {} | Cognition: {}", r, report), synthesized_sop_id))
            } else {
                let graph = match kernel.load_sop(&sop_id).await {
                    Ok(g) => g,
                    Err(_) => {
                        let mut g = SOPGraph::new();
                        let tool_id = sop_id.clone();
                        let node = Arc::new(WasmSOPNode {
                            name: format!("node_{}", tool_id),
                            tool_id,
                            executor: kernel.tool_executor.clone(),
                            fuel_limit: default_fuel,
                        });
                        g.add_node(node);
                        g
                    }
                };
                let start_node = match graph.graph.node_indices().next() {
                    Some(n) => n,
                    None => return,
                };
                kernel.execute_with_cognition(&graph, start_node, &sop_id, None, Some(tx)).await
                    .map(|(r, report, _)| (format!("Result: {} | Cognition: {}", r, report), sop_id))
            };
            // Kernel sends events through tx; no extra completion event needed.
        });

        let output_stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx)
            .map(|event| -> Result<ExecutionEvent, Status> {
                Ok(match event {
                    sisyphus_types::ExecutionEvent::NodeStarted { node_name } => ExecutionEvent {
                        event_type: 0,
                        node_name,
                        status: "STARTED".into(),
                        message: "".into(),
                        timestamp: 0,
                    },
                    sisyphus_types::ExecutionEvent::NodeCompleted { node_name, output } => ExecutionEvent {
                        event_type: 1,
                        node_name,
                        status: "COMPLETED".into(),
                        message: output.to_string(),
                        timestamp: 0,
                    },
                    sisyphus_types::ExecutionEvent::NodeFailed { node_name, error } => ExecutionEvent {
                        event_type: 2,
                        node_name,
                        status: "FAILED".into(),
                        message: error,
                        timestamp: 0,
                    },
                    sisyphus_types::ExecutionEvent::ExecutionComplete { result } => ExecutionEvent {
                        event_type: 3,
                        node_name: "".into(),
                        status: "COMPLETE".into(),
                        message: result.to_string(),
                        timestamp: 0,
                    },
                })
            });

        Ok(Response::new(Box::pin(output_stream)))
    }

    async fn execute_tool(
        &self,
        request: Request<ExecuteToolRequest>,
    ) -> Result<Response<ExecuteToolResponse>, Status> {
        let req = request.into_inner();
        let tool_id = req.tool_id.clone();
        let args_json = req.args_json;
        let fuel = if req.fuel_limit > 0 { req.fuel_limit } else { self.default_fuel };

        let span = info_span!("bridge.execute_tool", tool_id = %tool_id);
        let _enter = span.enter();

        let args_bytes = args_json.into_bytes();
        let result = self.kernel.tool_executor.execute(&tool_id, args_bytes, fuel).await;

        match result {
            Ok(bytes) => {
                let result_str = String::from_utf8_lossy(&bytes).to_string();
                record_bridge_request(true, 0.0);
                Ok(Response::new(ExecuteToolResponse {
                    status: "SUCCESS".into(),
                    result_json: result_str,
                }))
            }
            Err(e) => {
                record_bridge_request(false, 0.0);
                Ok(Response::new(ExecuteToolResponse {
                    status: format!("ERROR: {e}"),
                    result_json: "".into(),
                }))
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config_path = std::env::var("SISYPHUS_CONFIG")
        .unwrap_or_else(|_| "config.toml".to_string());

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

    {
        let config = shared_config.read().unwrap();
        init_metrics(config.metrics.prometheus_port)?;
    }

    let addr = "[::1]:50051".parse()?;

    // Initialize bridge with current config snapshot
    let bridge = {
        let config = shared_config.read().unwrap();
        MySisyphusBridge::new(&config).await
    };

    // Spawn config hot-reload in background
    tokio::spawn(async move {
        _store.start_watching().await;
    });

    info!(
        "Sisyphus High-Performance Cognitive Bridge listening on {}",
        addr
    );

    Server::builder()
        .add_service(SisyphusBridgeServer::new(bridge))
        .serve(addr)
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::sisyphus_bridge::{
        sisyphus_bridge_server::SisyphusBridge,
        ExecuteSopRequest, ExecuteToolRequest, HealthCheckRequest, ListSoPsRequest,
    };
    use tokio_stream::StreamExt;
    use tonic::Request;
    use std::collections::HashMap;

    struct TestBridge(&'static str);

    #[tonic::async_trait]
    impl SisyphusBridge for TestBridge {
        async fn execute_sop(
            &self,
            _request: Request<ExecuteSopRequest>,
        ) -> Result<tonic::Response<super::sisyphus_bridge::SopResponse>, tonic::Status> {
            Ok(tonic::Response::new(super::sisyphus_bridge::SopResponse {
                status: "SUCCESS".into(),
                result: "mock_output".into(),
            }))
        }

        async fn health_check(
            &self,
            _request: Request<HealthCheckRequest>,
        ) -> Result<tonic::Response<super::sisyphus_bridge::HealthCheckResponse>, tonic::Status> {
            Ok(tonic::Response::new(super::sisyphus_bridge::HealthCheckResponse {
                status: "SERVING".into(),
                version: self.0.to_string(),
            }))
        }

        async fn list_so_ps(
            &self,
            _request: Request<ListSoPsRequest>,
        ) -> Result<tonic::Response<super::sisyphus_bridge::ListSoPsResponse>, tonic::Status> {
            Ok(tonic::Response::new(super::sisyphus_bridge::ListSoPsResponse {
                sops: vec![],
            }))
        }

        type StreamExecutionStream = tonic::codegen::BoxStream<super::sisyphus_bridge::ExecutionEvent>;

        async fn stream_execution(
            &self,
            _request: Request<ExecuteSopRequest>,
        ) -> Result<tonic::Response<Self::StreamExecutionStream>, tonic::Status> {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            tokio::spawn(async move {
                let _ = tx.send(super::sisyphus_bridge::ExecutionEvent {
                    event_type: 3,
                    node_name: "".into(),
                    status: "COMPLETE".into(),
                    message: "done".into(),
                    timestamp: 0,
                });
            });
            let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx)
                .map(|e| Ok(e));
            Ok(tonic::Response::new(Box::pin(stream)))
        }

        async fn execute_tool(
            &self,
            request: Request<ExecuteToolRequest>,
        ) -> Result<tonic::Response<super::sisyphus_bridge::ExecuteToolResponse>, tonic::Status> {
            let req = request.into_inner();
            Ok(tonic::Response::new(super::sisyphus_bridge::ExecuteToolResponse {
                status: "SUCCESS".into(),
                result_json: format!("{{\"echo\": \"{}\"}}", req.tool_id),
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
