use sisyphus_bridge::sisyphus_bridge_server::{SisyphusBridge, SisyphusBridgeServer};
use sisyphus_bridge::{
    ExecuteSopRequest, HealthCheckRequest, HealthCheckResponse, ListSopsRequest, ListSopsResponse,
    SopInfo, SopResponse, ExecutionEvent,
};
use sisyphus_core::{SOPGraph, SisyphusKernel, WasmSOPNode};
use sisyphus_intel::{SisyphusIntel, StandardReflector};
use sisyphus_mem::MfsStore;
use sisyphus_metrics::{init_metrics, record_bridge_request, record_sop_execution};
use sisyphus_sandbox::WasmToolExecutor;
use sisyphus_types::Config;
use std::path::PathBuf;
use std::sync::Arc;
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

        let tool_executor = Arc::new(
            WasmToolExecutor::with_config(
                Some(PathBuf::from(&config.sandbox.wasm_tools_dir)),
                Some(config.sandbox.max_memory_pages),
            )
            .expect("Failed to create WasmToolExecutor"),
        );
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

    async fn list_sops(
        &self,
        _request: Request<ListSopsRequest>,
    ) -> Result<Response<ListSopsResponse>, Status> {
        Ok(Response::new(ListSopsResponse {
            sops: vec![],
        }))
    }

    type StreamExecutionStream = tonic::codegen::BoxStream<Result<ExecutionEvent, Status>>;

    async fn stream_execution(
        &self,
        request: Request<ExecuteSopRequest>,
    ) -> Result<Response<Self::StreamExecutionStream>, Status> {
        let req = request.into_inner();
        let sop_id = req.sop_id.clone();
        let (tx, rx) = mpsc::unbounded_channel();
        let tx_wrapped = Some(tx);

        let kernel = self.kernel.clone();
        let default_fuel = self.default_fuel;

        tokio::spawn(async move {
            let result = if let Some(goal) = sop_id.strip_prefix("goal:") {
                let synthesized_sop_id = format!("synth_{}", goal);
                match kernel.execute_goal(goal, &synthesized_sop_id, tx_wrapped.clone()).await {
                    Ok((result, report)) => Ok((format!("Result: {} | Cognition: {}", result, report), synthesized_sop_id)),
                    Err(e) => Err(Status::internal(e)),
                }
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
                let start_node = graph.graph.node_indices().next().ok_or_else(|| Status::internal("Graph empty"))?;
                match kernel.execute_with_cognition(&graph, start_node, &sop_id, None, tx_wrapped.clone()).await {
                    Ok((result, report, _)) => Ok((format!("Result: {} | Cognition: {}", result, report), sop_id)),
                    Err(e) => Err(Status::internal(e)),
                }
            };

            if let Some(ref tx) = tx_wrapped {
                match result {
                    Ok((res, _)) => {
                        let _ = tx.send(Ok(ExecutionEvent {
                            event_type: 3,
                            node_name: "".into(),
                            status: "SUCCESS".into(),
                            message: res,
                            timestamp: 0,
                        }));
                    }
                    Err(e) => {
                        let _ = tx.send(Ok(ExecutionEvent {
                            event_type: 2,
                            node_name: "".into(),
                            status: "FAILED".into(),
                            message: e.to_string(),
                            timestamp: 0,
                        }));
                    }
                }
            }
        });

        let output_stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx)
            .map(|event| {
                match event {
                    sisyphus_types::ExecutionEvent::NodeStarted { node_name } => Ok(ExecutionEvent {
                        event_type: 0,
                        node_name,
                        status: "STARTED".into(),
                        message: "".into(),
                        timestamp: 0,
                    }),
                    sisyphus_types::ExecutionEvent::NodeCompleted { node_name, output } => Ok(ExecutionEvent {
                        event_type: 1,
                        node_name,
                        status: "COMPLETED".into(),
                        message: output.to_string(),
                        timestamp: 0,
                    }),
                    sisyphus_types::ExecutionEvent::NodeFailed { node_name, error } => Ok(ExecutionEvent {
                        event_type: 2,
                        node_name,
                        status: "FAILED".into(),
                        message: error,
                        timestamp: 0,
                    }),
                    sisyphus_types::ExecutionEvent::ExecutionComplete { result } => Ok(ExecutionEvent {
                        event_type: 3,
                        node_name: "".into(),
                        status: "COMPLETE".into(),
                        message: result.to_string(),
                        timestamp: 0,
                    }),
                }
            });

        Ok(Response::new(Box::pin(output_stream)))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config_path = std::env::var("SISYPHUS_CONFIG")
        .unwrap_or_else(|_| "config.toml".to_string());
    let mut config = match Config::from_file(&config_path) {
        Ok(cfg) => {
            info!(path = %config_path, "Loaded configuration");
            cfg
        }
        Err(e) => {
            warn!(path = %config_path, error = %e, "Config file not found, using defaults");
            Config::default()
        }
    };
    config.apply_env_overrides();

    init_metrics(config.metrics.prometheus_port)?;

    let addr = "[::1]:50051".parse()?;
    let bridge = MySisyphusBridge::new(&config).await;

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
