use sisyphus_bridge::sisyphus_bridge_server::{SisyphusBridge, SisyphusBridgeServer};
use sisyphus_bridge::{ExecuteSopRequest, SopResponse};
use sisyphus_core::{SOPGraph, SisyphusKernel, WasmSOPNode};
use sisyphus_intel::{SisyphusIntel, StandardReflector};
use sisyphus_mem::MfsStore;
use sisyphus_metrics::{init_metrics, record_bridge_request, record_sop_execution};
use sisyphus_sandbox::WasmToolExecutor;
use sisyphus_types::Config;
use std::sync::Arc;
use std::time::Instant;
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

        let tool_executor =
            Arc::new(WasmToolExecutor::new().expect("Failed to create WasmToolExecutor"));
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

        let start = Instant::now();
        info!("Cognitive Execution started");

        let result = if let Some(goal) = sop_id.strip_prefix("goal:") {
            let synthesized_sop_id = format!("synth_{}", goal);
            match self.kernel.execute_goal(goal, &synthesized_sop_id).await {
                Ok((result, report)) => {
                    let duration = start.elapsed();
                    record_sop_execution(&synthesized_sop_id, true, duration.as_secs_f64());
                    info!(
                        duration_ms = duration.as_millis(),
                        "Goal execution succeeded"
                    );
                    Ok(Response::new(SopResponse {
                        status: "SUCCESS".into(),
                        result: format!("Result: {} | Cognition: {}", result, report),
                    }))
                }
                Err(e) => {
                    let duration = start.elapsed();
                    record_sop_execution(&synthesized_sop_id, false, duration.as_secs_f64());
                    warn!(error = %e, duration_ms = duration.as_millis(), "Goal execution failed");
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
                .execute_with_cognition(&graph, start_node, &sop_id, None)
                .await
            {
                Ok((result, report, evolved_graph)) => {
                    let duration = start.elapsed();
                    record_sop_execution(&sop_id, true, duration.as_secs_f64());
                    if evolved_graph.is_some() {
                        info!("SOP evolved for sop_id={}", sop_id);
                    }
                    info!(
                        duration_ms = duration.as_millis(),
                        "SOP execution succeeded"
                    );
                    Ok(Response::new(SopResponse {
                        status: "SUCCESS".into(),
                        result: format!("Result: {} | Cognition: {}", result, report),
                    }))
                }
                Err(e) => {
                    let duration = start.elapsed();
                    record_sop_execution(&sop_id, false, duration.as_secs_f64());
                    warn!(error = %e, duration_ms = duration.as_millis(), "SOP execution failed");
                    Err(Status::internal(e))
                }
            }
        };

        let duration = start.elapsed();
        span.record("duration_ms", duration.as_millis() as u64);
        let success = result.is_ok();
        record_bridge_request(success, duration.as_secs_f64());
        result
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ── Load configuration ───────────────────────────────────────────
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

    // ── Initialize metrics ───────────────────────────────────────────
    init_metrics()?;

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
