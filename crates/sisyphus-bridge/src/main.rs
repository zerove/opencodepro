use tonic::{transport::Server, Request, Response, Status};
use sisyphus_bridge::sisyphus_bridge_server::{SisyphusBridgeServer, SisyphusBridge};
use sisyphus_bridge::{ExecuteSopRequest, SopResponse};
use sisyphus_core::{SOPGraph, SisyphusKernel, WasmSOPNode};
use sisyphus_sandbox::WasmToolExecutor;
use sisyphus_mem::MfsStore;
use sisyphus_intel::{SisyphusIntel, StandardReflector};
use std::sync::Arc;

pub mod sisyphus_bridge {
    tonic::include_proto!("sisyphus.bridge");
}

pub struct MySisyphusBridge {
    kernel: Arc<SisyphusKernel>,
}

impl MySisyphusBridge {
    pub async fn new() -> Self {
        let state_store = Arc::new(MfsStore::new("sqlite::memory:").await.expect("Failed to init MFS Store"));
        let tool_executor = Arc::new(WasmToolExecutor::new());
        
        let simulator = Arc::new(sisyphus_intel::ExperienceSimulator::new(state_store.clone()));
        let reflector = Arc::new(StandardReflector);
        let evolver = Arc::new(sisyphus_intel::StandardEvolver::new());
        let intel = Arc::new(SisyphusIntel::new(simulator, reflector, evolver));
        
        let synthesizer = Arc::new(sisyphus_intel::TemplateSynthesizer::new(tool_executor.clone()));
        
        let kernel = Arc::new(SisyphusKernel::new(state_store, tool_executor, intel, synthesizer));
        
        Self { kernel }
    }

}

#[tonic::async_trait]
impl SisyphusBridge for MySisyphusBridge {
    async fn execute_sop(
        &self,
        request: Request<ExecuteSopRequest>,
    ) -> Result<Response<SopResponse>, Status> {
        let req = request.into_inner();
        println!("Cognitive Execution: sop_id={}", req.sop_id);

        if req.sop_id.starts_with("goal:") {
            let goal = &req.sop_id[5..];
            let sop_id = format!("synth_{}", goal);
            match self.kernel.execute_goal(goal, &sop_id).await {
                Ok((result, report)) => {
                    Ok(Response::new(SopResponse {
                        status: "SUCCESS".into(),
                        result: format!("Result: {} | Cognition: {}", result, report),
                    }))
                }
                Err(e) => {
                    println!("[ERROR] Goal execution failed for {}: {}", goal, e);
                    Err(Status::internal(e))
                }
            }
        } else {
            let graph = match self.kernel.load_sop(&req.sop_id).await {
                Ok(g) => g,
                Err(_) => {
                    let mut g = SOPGraph::new();
                    let tool_id = req.sop_id.clone();
                    let node = Arc::new(WasmSOPNode {
                        name: format!("node_{}", tool_id),
                        tool_id,
                        executor: self.kernel.tool_executor.clone(),
                        fuel_limit: 1_000_000,
                    });
                    g.add_node(node);
                    g
                }
            };
            
            let start_node = graph.graph.node_indices().next().ok_or_else(|| Status::internal("Graph empty"))?;
            
            match self.kernel.execute_with_cognition(&graph, start_node, &req.sop_id).await {
                Ok((result, report, evolved_graph)) => {
                    if evolved_graph.is_some() {
                        println!("SOP evolved for sop_id={}", req.sop_id);
                    }
                    Ok(Response::new(SopResponse {
                        status: "SUCCESS".into(),
                        result: format!("Result: {} | Cognition: {}", result, report),
                    }))
                }
                Err(e) => {
                    println!("[ERROR] Execution failed for sop_id={}: {}", req.sop_id, e);
                    Err(Status::internal(e))
                }
            }
        }
    }

}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "[::1]:50051".parse()?;
    let bridge = MySisyphusBridge::new().await;

    println!("Sisyphus High-Performance Cognitive Bridge listening on {}", addr);

    Server::builder()
        .add_service(SisyphusBridgeServer::new(bridge))
        .serve(addr)
        .await?;

    Ok(())
}
