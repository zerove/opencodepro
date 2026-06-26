use sisyphus_bridge::sisyphus_bridge_server::{SisyphusBridge, SisyphusBridgeServer};
use sisyphus_bridge::{
    AgentMetadata, DecomposeGoalRequest, DecomposeGoalResponse, ExecuteSopRequest,
    ExecuteToolRequest, ExecuteToolResponse, HealthCheckRequest, HealthCheckResponse,
    ListAgentsRequest, ListAgentsResponse, ListSkillsRequest, ListSkillsResponse,
    ListToolsRequest, ListToolsResponse, ShowSkillRequest, ShowSkillResponse,
    InstallSkillRequest, InstallSkillResponse,
    RemoveSkillRequest, RemoveSkillResponse, SkillMetadata, ToolMetadata,
    ListSoPsRequest, ListSoPsResponse, SopResponse,
    TaskProto, ExecutionEvent,
    ListBenchmarksRequest, ListBenchmarksResponse, BenchmarkEntry,
    BreakerMetricsRequest, BreakerMetricsResponse, BreakerStatus,
};
use sisyphus_tools::LlmClient;
use sisyphus_core::{SOPGraph, SisyphusKernel, WasmSOPNode, Consensus};
use sisyphus_intel::{SisyphusIntel, StandardReflector};
use sisyphus_mem::MfsStore;
use std::num::NonZeroUsize;
use sisyphus_metrics::{init_metrics, record_bridge_request, record_sop_execution};
use sisyphus_sandbox::{WasmToolExecutor, PluginManager};
use sisyphus_tools::{
    AgentRegistry, BuiltinToolExecutor, ConfigStore, MCPToolExecutor, SkillLoader,
    TeamGoal, TeamOrchestrator, ReflectionReport, RoadmapManager,
};
use sisyphus_types::{Config, ExecutorRouter, ToolExecutor, MilestoneStatus, StateStore};
use std::path::PathBuf;
use std::sync::{Arc, RwLock, Weak};
use std::time::Instant;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio::signal;
use tonic::{transport::Server, Request, Response, Status};
use tracing::{info, info_span, warn};

mod auth;

/// No-op consensus implementation: always assumes this node is the leader.
pub struct NoOpConsensus;

#[async_trait::async_trait]
impl Consensus for NoOpConsensus {
    async fn is_leader(&self) -> bool {
        true
    }
    async fn get_leader(&self) -> String {
        "self".into()
    }
}

struct StrategicExecutor {
    kernel: Arc<RwLock<Option<Weak<SisyphusKernel>>>>,
    state_store: Arc<dyn StateStore>,
    llm_client: Arc<dyn sisyphus_tools::LlmClient>,
}

impl StrategicExecutor {
    fn new(state_store: Arc<dyn StateStore>, llm_client: Arc<dyn sisyphus_tools::LlmClient>) -> Self {
        Self {
            kernel: Arc::new(RwLock::new(None)),
            state_store,
            llm_client,
        }
    }

    fn set_kernel(&self, kernel: Weak<SisyphusKernel>) {
        *self.kernel.write().unwrap() = Some(kernel);
    }
}

#[tonic::async_trait]
impl ToolExecutor for StrategicExecutor {
    async fn execute(&self, tool_id: &str, args: Vec<u8>, _fuel: u64) -> Result<Vec<u8>, String> {
        let kernel_weak = self.kernel.read().unwrap().clone();
        let kernel = kernel_weak
            .and_then(|w| w.upgrade())
            .ok_or_else(|| "SisyphusKernel is not initialized or has been dropped".to_string())?;

        if tool_id == "next_goal" {
            let args_str = String::from_utf8_lossy(&args);
            let v: serde_json::Value = serde_json::from_str(&args_str)
                .map_err(|e| format!("Invalid JSON args: {e}"))?;
            
            let project_id = v["project_id"]
                .as_str()
                .ok_or_else(|| "Missing 'project_id' in arguments".to_string())?
                .to_string();

            info!("Strategic loop triggered for project: {}", project_id);

            let manager = RoadmapManager::new(self.state_store.clone(), project_id.clone());

            let goal_ref = manager.get_next_goal().await
                .map_err(|e| format!("Failed to get next goal: {e}"))?;

            let goal = match goal_ref {
                Some(g) => g,
                None => return Ok(b"No pending goals in the roadmap.".to_vec()),
            };

            info!("Strategic Loop: Executing next goal [{}] - {}", goal.goal_id, goal.description);

            let roadmap_context = manager.get_goal_context(&goal.goal_id).await
                .unwrap_or_else(|_| "Failed to load roadmap context.".to_string());
            let enriched_goal = format!(
                "{}\n\n--- Roadmap Context ---\n{}",
                goal.description,
                roadmap_context,
            );

            let synth_id = format!("strategic_{}", goal.goal_id);
            match kernel.execute_goal(&enriched_goal, &synth_id, None).await {
                Ok((result, report)) => {
                    manager.update_goal_status(&goal.goal_id, MilestoneStatus::Completed).await
                        .map_err(|e| format!("Goal executed but failed to update roadmap: {e}"))?;

                    let reflection = manager.reflect().await
                        .unwrap_or_else(|_| ReflectionReport {
                            project_id: project_id.clone(),
                            project_name: String::new(),
                            overall_progress_pct: 0.0,
                            milestones: vec![],
                            is_on_track: true,
                            stalled_goals: vec![],
                            completed_recently: vec![],
                            recommendations: vec![],
                        });

                    let output = format!(
                        "Strategic Goal [{}] Completed.\nResult: {}\nCognition: {}\n\n--- Reflection ---\nProgress: {:.0}% | On track: {}\nRecommendations:\n{}",
                        goal.goal_id,
                        result,
                        report,
                        reflection.overall_progress_pct,
                        if reflection.is_on_track { "yes" } else { "no" },
                        if reflection.recommendations.is_empty() {
                            "  None.".to_string()
                        } else {
                            reflection.recommendations.iter().map(|r| format!("  - {}", r)).collect::<Vec<_>>().join("\n")
                        },
                    );
                    Ok(output.into_bytes())
                }
                Err(e) => {
                    Err(format!("Strategic Goal [{}] execution failed: {e}", goal.goal_id))
                }
            }
        } else if tool_id == "progress" {
            let args_str = String::from_utf8_lossy(&args);
            let v: serde_json::Value = serde_json::from_str(&args_str)
                .map_err(|e| format!("Invalid JSON args: {e}"))?;
            
            let project_id = v["project_id"]
                .as_str()
                .ok_or_else(|| "Missing 'project_id' in arguments".to_string())?
                .to_string();

            let manager = RoadmapManager::new(self.state_store.clone(), project_id);
            let progress = manager.get_project_progress().await
                .map_err(|e| format!("Failed to get progress: {e}"))?;

            Ok(format!("Project overall progress: {:.2}%", progress * 100.0).into_bytes())
        } else if tool_id == "show_roadmap" {
            let args_str = String::from_utf8_lossy(&args);
            let v: serde_json::Value = serde_json::from_str(&args_str)
                .map_err(|e| format!("Invalid JSON args: {e}"))?;
            
            let project_id = v["project_id"]
                .as_str()
                .ok_or_else(|| "Missing 'project_id' in arguments".to_string())?
                .to_string();

            let manager = RoadmapManager::new(self.state_store.clone(), project_id);
            let project = manager.get_full_roadmap().await
                .map_err(|e| format!("Failed to load roadmap: {e}"))?;

            let mut output = format!("# Project: {}\n{}\n\n", project.name, project.description);
            for milestone in &project.roadmap.milestones {
                let progress = milestone.calculate_progress();
                output.push_str(&format!("## Milestone: {} [{:?}] ({:.1}%)\n", 
                    milestone.name, milestone.status, progress * 100.0));
                
                for phase in &milestone.phases {
                    output.push_str(&format!("### Phase: {}\n", phase.name));
                    for goal in &phase.goals {
                        let status_icon = match goal.status {
                            MilestoneStatus::Completed => "✅",
                            MilestoneStatus::InProgress => "🚧",
                            _ => "⬜",
                        };
                        output.push_str(&format!("- {} {}\n", status_icon, goal.description));
                    }
                }
                output.push('\n');
            }
            Ok(output.into_bytes())
        } else if tool_id == "replan" {
            let args_str = String::from_utf8_lossy(&args);
            let v: serde_json::Value = serde_json::from_str(&args_str)
                .map_err(|e| format!("Invalid JSON args: {e}"))?;
            
            let project_id = v["project_id"]
                .as_str()
                .ok_or_else(|| "Missing 'project_id' in arguments".to_string())?
                .to_string();

            let project_val = &v["project"];
            if project_val.is_null() {
                return Err("Missing 'project' data in arguments".to_string());
            }

            let project: sisyphus_types::Project = serde_json::from_value(project_val.clone())
                .map_err(|e| format!("Invalid project data: {e}"))?;

            let manager = RoadmapManager::new(self.state_store.clone(), project_id);
            manager.save_project(&project).await
                .map_err(|e| format!("Failed to replan roadmap: {e}"))?;

            Ok(b"Roadmap successfully replanned.".to_vec())
        } else if tool_id == "auto_plan" {
            let args_str = String::from_utf8_lossy(&args);
            let v: serde_json::Value = serde_json::from_str(&args_str)
                .map_err(|e| format!("Invalid JSON args: {e}"))?;
            
            let project_id = v["project_id"]
                .as_str()
                .ok_or_else(|| "Missing 'project_id' in arguments".to_string())?
                .to_string();
            let description = v["description"]
                .as_str()
                .ok_or_else(|| "Missing 'description' in arguments".to_string())?
                .to_string();

            let manager = RoadmapManager::new(self.state_store.clone(), project_id);
            let project = manager.auto_plan(self.llm_client.as_ref(), &description).await
                .map_err(|e| format!("Auto-plan failed: {e}"))?;

            let output = serde_json::to_string_pretty(&project)
                .map_err(|e| format!("Failed to serialize result: {e}"))?;
            Ok(output.into_bytes())
        } else if tool_id == "goal_context" {
            let args_str = String::from_utf8_lossy(&args);
            let v: serde_json::Value = serde_json::from_str(&args_str)
                .map_err(|e| format!("Invalid JSON args: {e}"))?;
            
            let project_id = v["project_id"]
                .as_str()
                .ok_or_else(|| "Missing 'project_id' in arguments".to_string())?
                .to_string();
            let goal_id = v["goal_id"]
                .as_str()
                .ok_or_else(|| "Missing 'goal_id' in arguments".to_string())?
                .to_string();

            let manager = RoadmapManager::new(self.state_store.clone(), project_id);
            let context = manager.get_goal_context(&goal_id).await
                .map_err(|e| format!("Failed to get goal context: {e}"))?;

            Ok(context.into_bytes())
        } else if tool_id == "reflect" {
            let args_str = String::from_utf8_lossy(&args);
            let v: serde_json::Value = serde_json::from_str(&args_str)
                .map_err(|e| format!("Invalid JSON args: {e}"))?;

            let project_id = v["project_id"]
                .as_str()
                .ok_or_else(|| "Missing 'project_id' in arguments".to_string())?
                .to_string();

            let manager = RoadmapManager::new(self.state_store.clone(), project_id);
            let report = manager.reflect().await
                .map_err(|e| format!("Reflection failed: {e}"))?;

            let output = serde_json::to_string_pretty(&report)
                .map_err(|e| format!("Failed to serialize reflection: {e}"))?;
            Ok(output.into_bytes())
        } else if tool_id == "predict" {
            let args_str = String::from_utf8_lossy(&args);
            let v: serde_json::Value = serde_json::from_str(&args_str)
                .map_err(|e| format!("Invalid JSON args: {e}"))?;

            let project_id = v["project_id"]
                .as_str()
                .ok_or_else(|| "Missing 'project_id' in arguments".to_string())?
                .to_string();

            let manager = RoadmapManager::new(self.state_store.clone(), project_id);
            let prediction = manager.predict_completion().await
                .map_err(|e| format!("Prediction failed: {e}"))?;

            let output = serde_json::to_string_pretty(&prediction)
                .map_err(|e| format!("Failed to serialize prediction: {e}"))?;
            Ok(output.into_bytes())
        } else {
            return Err(format!("Unknown strategic tool: {tool_id}"));
        }
    }
}

pub mod sisyphus_bridge {
    tonic::include_proto!("sisyphus.bridge");
}

pub struct MySisyphusBridge {
    kernel: Arc<SisyphusKernel>,
    consensus: Arc<dyn Consensus>,
    state_store: Arc<dyn sisyphus_types::StateStore>,
    default_fuel: u64,
    agent_registry: Arc<AgentRegistry>,
    tool_registry: Arc<sisyphus_tools::ToolRegistry>,
    team_orchestrator: Arc<TeamOrchestrator>,
    skill_loader: Arc<SkillLoader>,
    llm_client: Arc<sisyphus_tools::DynamicLlmClient>,
    plugin_manager: Arc<PluginManager>,
}

impl Clone for MySisyphusBridge {
    fn clone(&self) -> Self {
        Self {
            kernel: self.kernel.clone(),
            consensus: self.consensus.clone(),
            state_store: self.state_store.clone(),
            default_fuel: self.default_fuel,
            agent_registry: self.agent_registry.clone(),
            tool_registry: self.tool_registry.clone(),
            team_orchestrator: self.team_orchestrator.clone(),
            skill_loader: self.skill_loader.clone(),
            llm_client: self.llm_client.clone(),
            plugin_manager: self.plugin_manager.clone(),
        }
    }
}

impl MySisyphusBridge {
    pub async fn new(shared_config: Arc<RwLock<Config>>) -> Self {
        let config = shared_config.read().unwrap();
        let state_store = Arc::new(
            MfsStore::with_capacity(
                    &config.cache.sqlite_path,
                    NonZeroUsize::new(config.cache.l1_capacity)
                        .expect("l1_capacity must be > 0"),
                )
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

        // Initialize PluginManager with hot-reload support
        let wasm_executor_inner = wasm_executor.as_ref().clone();
        let plugin_manager = PluginManager::new(
            wasm_executor_inner,
            PathBuf::from(&config.sandbox.wasm_tools_dir),
        );

        // Secondary executor: built-in tools
        let mut registry = sisyphus_tools::HookRegistry::new();
        let loader = SkillLoader::new()
            .with_user_dir()
            .with_project_dir(std::path::Path::new("."));
        let skills = loader.load_all();
        for skill in skills {
            let hooks = sisyphus_tools::hook::hooks_from_skill(&skill.name, &skill.triggers, &skill.instructions);
            registry.register_all(hooks);
        }

        // Create shared DynamicLlmClient for all LLM-dependent components
        let dyn_llm_client = Arc::new(sisyphus_tools::DynamicLlmClient::new(shared_config.clone()));
        let llm_client_for_bridge = dyn_llm_client.clone();
        let builtin_executor: Arc<dyn ToolExecutor> = Arc::new(BuiltinToolExecutor {
            registry,
            llm_client: {
                let c: Arc<dyn LlmClient> = dyn_llm_client.clone();
                c
            },
            feedback_store: Arc::new(sisyphus_tools::feedback::FeedbackStore::new(".omo/feedback")),
        });

        // Build router: WASM = default (backward compat), builtin = "builtin:" prefix
        let mut router = ExecutorRouter::new(wasm_executor.clone());
        router.register("builtin", builtin_executor);
        
        let strategic_executor = Arc::new(StrategicExecutor::new(
            state_store.clone(),
            dyn_llm_client.clone(),
        ));
        router.register("strategic", strategic_executor.clone());

        // Collect MCP tool info for agent registration (registry created below)
        let mut mcp_servers: Vec<(String, String, Vec<sisyphus_tools::McpTool>)> = Vec::new();

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
                    // Discover tools before moving into Arc for agent + tool registration
                    let tools = mcp.list_tools().await.unwrap_or_default();
                    info!("Connected MCP server '{prefix}' via stdio: {command_line} ({} tools)", tools.len());
                    mcp_servers.push((prefix.clone(), command_line.clone(), tools));
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
                    let tools = mcp.list_tools().await.unwrap_or_default();
                    info!("Connected MCP server '{prefix}' via HTTP: {url} ({} tools)", tools.len());
                    mcp_servers.push((prefix.clone(), url.clone(), tools));
                    router.register(prefix, Arc::new(mcp));
                }
                Err(e) => {
                    warn!("Failed to connect MCP server for '{prefix}' at {url}: {e}");
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
            state_store.clone(),
            tool_executor,
            intel,
            synthesizer,
        ));

        strategic_executor.set_kernel(Arc::downgrade(&kernel));

        // Create and populate the agent registry
        let agent_registry = Arc::new(AgentRegistry::new(dyn_llm_client));
        agent_registry.register_builtin();

        // Create tool registry and populate from MCP servers
        let tool_registry = Arc::new(sisyphus_tools::ToolRegistry::new());
        for (prefix, _label, tools) in &mcp_servers {
            for tool in tools {
                let tool_id = format!("{}:{}", prefix, tool.name);
                tool_registry.register(
                    &tool_id,
                    sisyphus_tools::ToolInfo {
                        name: tool.name.clone(),
                        description: tool.description.clone().unwrap_or_default(),
                        parameters: tool.input_schema.clone(),
                        source: sisyphus_tools::ToolSource::Mcp {
                            server_prefix: prefix.clone(),
                        },
                    },
                );
            }
        }
        // Register built-in and agent tools from BuiltinToolExecutor
        register_builtin_tools(&tool_registry);
        // Register strategic tools from StrategicExecutor
        register_strategic_tools(&tool_registry);

        info!("Tool registry initialized with {} tools", tool_registry.len());

        // Register MCP server agents with their tool prefixes
        for (prefix, label, tools) in &mcp_servers {
            agent_registry.register_mcp_server(prefix, label, tools.len());
        }
        info!("Agent registry initialized with {} agents", agent_registry.list().len());

        let team_llm: Option<Arc<dyn LlmClient>> = None; // planner_llm is optional
        let team_orchestrator = TeamOrchestrator::new(
            agent_registry.clone(),
            kernel.tool_executor.clone(),
            team_llm,
        );

        Self {
            kernel,
            state_store,
            default_fuel: config.executor.default_fuel,
            agent_registry,
            tool_registry,
            team_orchestrator: Arc::new(team_orchestrator),
            skill_loader: Arc::new(loader),
            llm_client: llm_client_for_bridge,
            consensus: Arc::new(NoOpConsensus),
            plugin_manager,
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
        
        // 自动实施：Leader-only 路由校验
        // 假设 sop_id 需要 leader 权限
        // if !self.kernel.consensus.is_leader().await {
        //     return Err(Status::failed_precondition("FORWARD_REQUIRED"));
        // }

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

    async fn list_agents(
        &self,
        _request: Request<ListAgentsRequest>,
    ) -> Result<Response<ListAgentsResponse>, Status> {
        let agents = self.agent_registry.list();
        Ok(Response::new(ListAgentsResponse {
            agents: agents
                .into_iter()
                .map(|(id, info)| {
                    // Enrich agent descriptions with actual tool names from the registry
                    let description = if !info.tool_prefixes.is_empty() {
                        let tools = self
                            .tool_registry
                            .list_for_prefixes(&info.tool_prefixes);
                        if tools.is_empty() {
                            info.description
                        } else {
                            let tool_names: Vec<&str> =
                                tools.iter().map(|(_, t)| t.name.as_str()).collect();
                            format!(
                                "{} — Tools: {}",
                                info.description,
                                tool_names.join(", ")
                            )
                        }
                    } else {
                        info.description
                    };
                    AgentMetadata {
                        agent_id: id,
                        display_name: info.display_name,
                        description,
                        tool_prefixes: info.tool_prefixes,
                        max_concurrency: info.max_concurrency as u32,
                    }
                })
                .collect(),
        }))
    }

    async fn list_tools(
        &self,
        _request: Request<ListToolsRequest>,
    ) -> Result<Response<ListToolsResponse>, Status> {
        let tools = self.tool_registry.list();
        Ok(Response::new(ListToolsResponse {
            tools: tools
                .into_iter()
                .map(|(tool_id, info)| {
                    let source_str = match &info.source {
                        sisyphus_tools::ToolSource::Builtin => "builtin",
                        sisyphus_tools::ToolSource::Mcp { .. } => "mcp",
                        sisyphus_tools::ToolSource::Strategic => "strategic",
                        sisyphus_tools::ToolSource::Wasm => "wasm",
                    };
                    let params_json = info
                        .parameters
                        .map(|p| p.to_string())
                        .unwrap_or_default();
                    ToolMetadata {
                        tool_id,
                        name: info.name,
                        description: info.description,
                        source: source_str.into(),
                        parameters_json: params_json,
                    }
                })
                .collect(),
        }))
    }

    async fn decompose_goal(
        &self,
        request: Request<DecomposeGoalRequest>,
    ) -> Result<Response<DecomposeGoalResponse>, Status> {
        let req = request.into_inner();
        let goal = TeamGoal {
            id: req.goal_id,
            objective: req.objective,
            context: if req.context.is_empty() { None } else { Some(req.context) },
        };
        match self.team_orchestrator.decompose_goal(&goal).await {
            Ok(plan) => {
                let tasks: Vec<TaskProto> = plan.tasks.into_iter().map(|t| TaskProto {
                    id: t.id,
                    agent_id: t.agent_id,
                    tool_id: t.tool_id,
                    args_json: t.args.to_string(),
                    depends_on: t.depends_on,
                }).collect();
                Ok(Response::new(DecomposeGoalResponse {
                    success: true,
                    error: String::new(),
                    tasks,
                }))
            }
            Err(e) => Ok(Response::new(DecomposeGoalResponse {
                success: false,
                error: e.to_string(),
                tasks: vec![],
            })),
        }
    }

    async fn list_skills(
        &self,
        _request: Request<ListSkillsRequest>,
    ) -> Result<Response<ListSkillsResponse>, Status> {
        let skills = self.skill_loader.load_all();
        let proto_skills: Vec<SkillMetadata> = skills.into_iter().map(|s| {
            let tool_triggers: Vec<String> = s.triggers.iter()
                .flat_map(|t| t.tools.clone())
                .collect();
            let preview = if s.instructions.len() > 200 {
                format!("{}...", &s.instructions[..200])
            } else {
                s.instructions.clone()
            };
            SkillMetadata {
                name: s.name,
                description: s.description,
                version: s.version,
                author: s.author.unwrap_or_default(),
                tags: s.tags,
                tool_triggers,
                instructions_preview: preview,
                source: match s.source {
                    sisyphus_tools::skill::SkillSource::BuiltIn => "builtin".into(),
                    sisyphus_tools::skill::SkillSource::User => "user".into(),
                    sisyphus_tools::skill::SkillSource::Project => "project".into(),
                },
            }
        }).collect();
        Ok(Response::new(ListSkillsResponse { skills: proto_skills }))
    }

    async fn show_skill(
        &self,
        request: Request<ShowSkillRequest>,
    ) -> Result<Response<ShowSkillResponse>, Status> {
        let req = request.into_inner();
        let skills = self.skill_loader.load_all();
        if let Some(skill) = skills.into_iter().find(|s| s.name == req.name) {
            let tool_triggers: Vec<String> = skill.triggers.iter()
                .flat_map(|t| t.tools.clone())
                .collect();
            let preview = if skill.instructions.len() > 200 {
                format!("{}...", &skill.instructions[..200])
            } else {
                skill.instructions.clone()
            };
            Ok(Response::new(ShowSkillResponse {
                found: true,
                skill: Some(SkillMetadata {
                    name: skill.name,
                    description: skill.description,
                    version: skill.version,
                    author: skill.author.unwrap_or_default(),
                    tags: skill.tags,
                    tool_triggers,
                    instructions_preview: preview,
                    source: match skill.source {
                        sisyphus_tools::skill::SkillSource::BuiltIn => "builtin".into(),
                        sisyphus_tools::skill::SkillSource::User => "user".into(),
                        sisyphus_tools::skill::SkillSource::Project => "project".into(),
                    },
                }),
            }))
        } else {
            Ok(Response::new(ShowSkillResponse { found: false, skill: None }))
        }
    }

    async fn install_skill(
        &self,
        request: Request<InstallSkillRequest>,
    ) -> Result<Response<InstallSkillResponse>, Status> {
        let req = request.into_inner();
        let path = std::path::Path::new(&req.source_path);

        // Read and validate the skill file
        match sisyphus_tools::skill::install_skill(path) {
            Ok(name) => Ok(Response::new(InstallSkillResponse {
                success: true,
                error: String::new(),
                name,
            })),
            Err(e) => Ok(Response::new(InstallSkillResponse {
                success: false,
                error: e.to_string(),
                name: String::new(),
            })),
        }
    }

    async fn remove_skill(
        &self,
        request: Request<RemoveSkillRequest>,
    ) -> Result<Response<RemoveSkillResponse>, Status> {
        let req = request.into_inner();
        match sisyphus_tools::skill::remove_skill(&req.name) {
            Ok(()) => Ok(Response::new(RemoveSkillResponse {
                success: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(RemoveSkillResponse {
                success: false,
                error: e.to_string(),
            })),
        }
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

    async fn list_benchmarks(
        &self,
        _request: Request<ListBenchmarksRequest>,
    ) -> Result<Response<ListBenchmarksResponse>, Status> {
        let bm = self.kernel.benchmark.lock().map_err(|e| {
            Status::internal(format!("Benchmark lock poisoned: {e}"))
        })?;
        let names = bm.list_benchmarks();
        let mut benchmarks = Vec::new();
        for name in names {
            let trend = bm.get_trend(&name, 10);
            for run in trend {
                benchmarks.push(BenchmarkEntry {
                    name: run.name,
                    duration_secs: run.duration_secs,
                    timestamp: run.timestamp,
                    success: run.success,
                    metrics_json: run.metrics.to_string(),
                });
            }
        }
        Ok(Response::new(ListBenchmarksResponse { benchmarks }))
    }

    async fn get_breaker_metrics(
        &self,
        request: Request<BreakerMetricsRequest>,
    ) -> Result<Response<BreakerMetricsResponse>, Status> {
        let model_filter = request.into_inner().model_name;
        let breaker = self.llm_client.breaker();
        let models: Vec<String> = if model_filter.is_empty() {
            vec![]
        } else {
            vec![model_filter]
        };
        let mut breakers = Vec::new();
        for model in models {
            let metrics = breaker.metrics(&model);
            breakers.push(BreakerStatus {
                model_name: model,
                state: format!("{:?}", metrics.state),
                failure_count: metrics.failure_count,
                success_count: metrics.success_count,
            });
        }
        // Also always include a few key models if no filter
        if breakers.is_empty() {
            for model in &["gpt-4o", "gpt-4o-mini", "gemma-3"] {
                let metrics = breaker.metrics(model);
                breakers.push(BreakerStatus {
                    model_name: model.to_string(),
                    state: format!("{:?}", metrics.state),
                    failure_count: metrics.failure_count,
                    success_count: metrics.success_count,
                });
            }
        }
        Ok(Response::new(BreakerMetricsResponse { breakers }))
    }

    async fn get_learning_insights(
        &self,
        request: Request<sisyphus_bridge::LearningInsightsRequest>,
    ) -> Result<Response<sisyphus_bridge::LearningInsightsResponse>, Status> {
        let filter = request.into_inner().strategy_filter;
        let ml = self.kernel.meta_learner.lock().map_err(|e| Status::internal(e.to_string()))?;
        let insight = ml.learn();

        let strategies: Vec<sisyphus_bridge::StrategyInsight> = insight.strategies.iter()
            .filter(|s| filter.is_empty() || s.strategy == filter)
            .map(|s| sisyphus_bridge::StrategyInsight {
                strategy: s.strategy.clone(),
                total_uses: s.total_uses,
                successes: s.successes,
                failures: s.failures,
                avg_duration_secs: s.avg_duration_secs,
                success_rate: s.success_rate,
            })
            .collect();

        Ok(Response::new(sisyphus_bridge::LearningInsightsResponse {
            total_samples: insight.total_samples,
            overall_success_rate: insight.overall_success_rate,
            strategies,
        }))
    }

    async fn list_plugins(
        &self,
        _request: Request<sisyphus_bridge::ListPluginsRequest>,
    ) -> Result<Response<sisyphus_bridge::ListPluginsResponse>, Status> {
        let plugins = self.plugin_manager.list_plugins();
        let descriptors: Vec<sisyphus_bridge::PluginDescriptor> = plugins.into_iter().map(|p| sisyphus_bridge::PluginDescriptor {
            name: p.name,
            version: p.version,
            wasm_path: p.wasm_path.to_string_lossy().to_string(),
            tool_id: p.tool_id,
            fuel_limit: p.fuel_limit,
            checksum: p.checksum.unwrap_or_default(),
        }).collect();
        Ok(Response::new(sisyphus_bridge::ListPluginsResponse { plugins: descriptors }))
    }

    async fn load_plugin(
        &self,
        request: Request<sisyphus_bridge::LoadPluginRequest>,
    ) -> Result<Response<sisyphus_bridge::LoadPluginResponse>, Status> {
        let req = request.into_inner();
        let descriptor = sisyphus_types::PluginDescriptor {
            name: req.name,
            version: req.version,
            wasm_path: std::path::PathBuf::from(&req.wasm_path),
            tool_id: req.tool_id,
            fuel_limit: if req.fuel_limit > 0 { req.fuel_limit } else { self.default_fuel },
            checksum: None,
        };
        match self.plugin_manager.register_plugin(descriptor).await {
            Ok(()) => Ok(Response::new(sisyphus_bridge::LoadPluginResponse {
                success: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(sisyphus_bridge::LoadPluginResponse {
                success: false,
                error: e,
            })),
        }
    }

    async fn unload_plugin(
        &self,
        request: Request<sisyphus_bridge::UnloadPluginRequest>,
    ) -> Result<Response<sisyphus_bridge::UnloadPluginResponse>, Status> {
        let name = request.into_inner().name;
        match self.plugin_manager.unload_plugin(&name).await {
            Ok(()) => Ok(Response::new(sisyphus_bridge::UnloadPluginResponse {
                success: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(sisyphus_bridge::UnloadPluginResponse {
                success: false,
                error: e,
            })),
        }
    }
}

/// Register all built-in tools and agent tools from BuiltinToolExecutor.
fn register_builtin_tools(reg: &Arc<sisyphus_tools::ToolRegistry>) {
    use sisyphus_tools::{ToolInfo, ToolSource};
    let builtin = ToolSource::Builtin;
    // Agent tools use bare IDs (e.g. "librarian/research") matching agent_prefix "librarian/"
    // Core builtin tools use "builtin:" prefix matching router dispatch
    let tools: Vec<(&str, &str, &str)> = vec![
        // Core builtin tools (router prefix: builtin:)
        ("builtin:file_read", "file_read", "Read file content by path"),
        ("builtin:file_write", "file_write", "Write content to a file"),
        ("builtin:file_edit", "file_edit", "Replace first occurrence of old string with new string in a file"),
        ("builtin:file_edit_all", "file_edit_all", "Replace all occurrences of old string with new string in a file"),
        ("builtin:file_read_lines", "file_read_lines", "Read specific lines from a file with offset and limit"),
        ("builtin:glob", "glob", "Find files matching a glob pattern"),
        ("builtin:grep", "grep", "Search file contents with a regex pattern"),
        ("builtin:bash_exec", "bash_exec", "Execute a bash command and return output"),
        ("builtin:web_search", "web_search", "Search the web for a query"),
        ("builtin:web_fetch", "web_fetch", "Fetch content from a URL"),
        ("builtin:feedback/save", "feedback/save", "Save feedback for an agent response"),
        ("builtin:feedback/get", "feedback/get", "Retrieve feedback history for an agent"),
        ("builtin:feedback/query", "feedback/query", "Query feedback by criteria"),
        // Agent tools (bare IDs match agent_prefix like "librarian/")
        ("librarian/research", "librarian/research", "Comprehensive web research with synthesis"),
        ("librarian/fetch_docs", "librarian/fetch_docs", "Fetch documentation from a URL"),
        ("librarian/examples", "librarian/examples", "Search code examples from GitHub"),
        ("librarian/synthesise", "librarian/synthesise", "Synthesize research findings into concise report"),
        ("metis/analyse", "metis/analyse", "Analyse requirements for ambiguity and hidden intentions"),
        ("momus/review_plan", "momus/review_plan", "Review a plan for clarity and completeness"),
        ("momus/review_code", "momus/review_code", "Review code changes for quality and standards"),
        ("momus/review_code_batch", "momus/review_code_batch", "Review multiple files in batch"),
        ("momus/review_arch", "momus/review_arch", "Review architecture design for soundness"),
        ("scribe/generate_adr", "scribe/generate_adr", "Generate an Architecture Decision Record"),
        ("scribe/update_docs", "scribe/update_docs", "Update documentation based on code changes"),
        ("scribe/apply_update", "scribe/apply_update", "Apply a documentation edit"),
        ("security/audit_code", "security/audit_code", "Audit code for security vulnerabilities"),
        ("security/threat_model", "security/threat_model", "Model threats for an architecture"),
    ];
    for (tool_id, name, desc) in tools {
        reg.register(
            tool_id,
            ToolInfo {
                name: name.into(),
                description: desc.into(),
                parameters: None,
                source: builtin.clone(),
            },
        );
    }
}

/// Register strategic planning tools from StrategicExecutor.
fn register_strategic_tools(reg: &Arc<sisyphus_tools::ToolRegistry>) {
    use sisyphus_tools::{ToolInfo, ToolSource};
    let source = ToolSource::Strategic;
    // Tool_id format: "strategic:<name>" matches router dispatch (prefix "strategic:")
    let tools: Vec<(&str, &str, &str)> = vec![
        ("strategic:next_goal", "next_goal", "Execute the next pending goal in the roadmap"),
        ("strategic:progress", "progress", "Query overall roadmap progress and status"),
        ("strategic:show_roadmap", "show_roadmap", "Display the full roadmap with milestones and goals"),
        ("strategic:replan", "replan", "Re-plan the roadmap based on execution results"),
        ("strategic:auto_plan", "auto_plan", "Automatically generate a roadmap from project description"),
        ("strategic:goal_context", "goal_context", "Get roadmap context for a specific goal"),
        ("strategic:reflect", "reflect", "Run strategic reflection on roadmap progress"),
        ("strategic:predict", "predict", "Predict completion timeline and identify risks"),
    ];
    for (tool_id, name, desc) in tools {
        reg.register(
            tool_id,
            ToolInfo {
                name: name.into(),
                description: desc.into(),
                parameters: None,
                source: source.clone(),
            },
        );
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

    let auth_token = {
        let config = shared_config.read().unwrap();
        init_metrics(config.metrics.prometheus_port)?;
        config.auth_token.clone()
    };

    let addr = "[::1]:50051".parse()?;

    // Initialize bridge with shared config for dynamic updates
    let bridge = MySisyphusBridge::new(shared_config.clone()).await;

    // Spawn config hot-reload in background
    tokio::spawn(async move {
        _store.start_watching().await;
    });

    info!(
        "Sisyphus High-Performance Cognitive Bridge listening on {}",
        addr
    );

    let auth_interceptor = auth::AuthInterceptor::new(auth_token);
    let server = Server::builder()
        .add_service(SisyphusBridgeServer::with_interceptor(bridge, auth_interceptor));

    info!("Starting gRPC server on {addr}");
    server
        .serve_with_shutdown(addr, async {
            signal::ctrl_c().await.ok();
            info!("Received SIGINT, starting graceful shutdown...");
            // Allow existing requests to finish
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        })
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
use super::sisyphus_bridge::{
    sisyphus_bridge_server::SisyphusBridge,
    DecomposeGoalRequest, DecomposeGoalResponse, ExecuteSopRequest, ExecuteToolRequest,
    HealthCheckRequest, ListAgentsRequest, ListSoPsRequest, ListToolsRequest,
    ListToolsResponse, ListPluginsRequest, ListPluginsResponse,
    LoadPluginRequest, LoadPluginResponse, UnloadPluginRequest, UnloadPluginResponse,
    LearningInsightsRequest, LearningInsightsResponse,
    TaskProto,
};
    use tokio_stream::StreamExt;
    use tonic::Request;
    use std::collections::HashMap;

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

        async fn list_agents(
            &self,
            _request: Request<ListAgentsRequest>,
        ) -> Result<tonic::Response<super::sisyphus_bridge::ListAgentsResponse>, tonic::Status> {
            Ok(tonic::Response::new(super::sisyphus_bridge::ListAgentsResponse {
                agents: vec![],
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
            _request: Request<super::sisyphus_bridge::ListSkillsRequest>,
        ) -> Result<tonic::Response<super::sisyphus_bridge::ListSkillsResponse>, tonic::Status> {
            Ok(tonic::Response::new(super::sisyphus_bridge::ListSkillsResponse {
                skills: vec![],
            }))
        }

        async fn show_skill(
            &self,
            _request: Request<super::sisyphus_bridge::ShowSkillRequest>,
        ) -> Result<tonic::Response<super::sisyphus_bridge::ShowSkillResponse>, tonic::Status> {
            Ok(tonic::Response::new(super::sisyphus_bridge::ShowSkillResponse {
                found: false,
                skill: None,
            }))
        }

        async fn install_skill(
            &self,
            _request: Request<super::sisyphus_bridge::InstallSkillRequest>,
        ) -> Result<tonic::Response<super::sisyphus_bridge::InstallSkillResponse>, tonic::Status> {
            Ok(tonic::Response::new(super::sisyphus_bridge::InstallSkillResponse {
                success: true,
                error: String::new(),
                name: "mock-skill".into(),
            }))
        }

        async fn remove_skill(
            &self,
            _request: Request<super::sisyphus_bridge::RemoveSkillRequest>,
        ) -> Result<tonic::Response<super::sisyphus_bridge::RemoveSkillResponse>, tonic::Status> {
            Ok(tonic::Response::new(super::sisyphus_bridge::RemoveSkillResponse {
                success: true,
                error: String::new(),
            }))
        }

        async fn list_tools(
            &self,
            _request: Request<ListToolsRequest>,
        ) -> Result<tonic::Response<ListToolsResponse>, tonic::Status> {
            Ok(tonic::Response::new(ListToolsResponse {
                tools: vec![],
            }))
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
            _request: Request<super::sisyphus_bridge::ListBenchmarksRequest>,
        ) -> Result<tonic::Response<super::sisyphus_bridge::ListBenchmarksResponse>, tonic::Status> {
            Ok(tonic::Response::new(super::sisyphus_bridge::ListBenchmarksResponse {
                benchmarks: vec![],
            }))
        }

        async fn get_breaker_metrics(
            &self,
            _request: Request<super::sisyphus_bridge::BreakerMetricsRequest>,
        ) -> Result<tonic::Response<super::sisyphus_bridge::BreakerMetricsResponse>, tonic::Status> {
            Ok(tonic::Response::new(super::sisyphus_bridge::BreakerMetricsResponse {
                breakers: vec![],
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
