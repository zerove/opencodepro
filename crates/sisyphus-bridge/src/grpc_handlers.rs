use std::sync::Arc;

use sisyphus_bridge_api::sisyphus_bridge_server::SisyphusBridge;
use sisyphus_bridge_api::{
    AddMessageRequest, AddMessageResponse, AgentMetadata, BenchmarkEntry, BreakerMetricsRequest,
    BreakerMetricsResponse, BreakerStatus, DecomposeGoalRequest, DecomposeGoalResponse,
    ExecuteSopRequest, ExecuteToolRequest, ExecuteToolResponse, ExecutionEvent,
    HealthCheckRequest, HealthCheckResponse, InstallSkillRequest, InstallSkillResponse,
    ListAgentsRequest, ListAgentsResponse, ListBenchmarksRequest, ListBenchmarksResponse,
    ListPluginsRequest, ListPluginsResponse, LoadPluginRequest, LoadPluginResponse,
    ListSoPsRequest, ListSoPsResponse, ListSkillsRequest, ListSkillsResponse, ListToolsRequest,
    ListToolsResponse, PluginDescriptor, RemoveSkillRequest, RemoveSkillResponse, ShowSkillRequest,
    ShowSkillResponse, SkillMetadata, SopResponse, TaskProto, ToolMetadata,
    UnloadPluginRequest, UnloadPluginResponse,
};
use sisyphus_core::{SOPGraph, WasmSOPNode};
use sisyphus_metrics::record_bridge_request;
use sisyphus_tools_llm::LlmClient;
use sisyphus_tools_agent::team::TeamGoal;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tonic::{Request, Response, Status};
use tracing::info_span;

use super::MySisyphusBridge;

// ── Sub-modules ────────────────────────────────────────────

#[path = "grpc_exec.rs"]
mod grpc_exec;

// ── gRPC handlers ──────────────────────────────────────────

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

    async fn list_agents(
        &self,
        _request: Request<ListAgentsRequest>,
    ) -> Result<Response<ListAgentsResponse>, Status> {
        let agents = self.agent_registry.list();
        Ok(Response::new(ListAgentsResponse {
            agents: agents
                .into_iter()
                .map(|(id, info)| {
                    let description = if !info.tool_prefixes.is_empty() {
                        let tools = self.tool_registry.list_for_prefixes(&info.tool_prefixes);
                        if tools.is_empty() {
                            info.description
                        } else {
                            let tool_names: Vec<&str> =
                                tools.iter().map(|(_, t)| t.name.as_str()).collect();
                            format!("{} — Tools: {}", info.description, tool_names.join(", "))
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
                        sisyphus_tools_mcp::tool_registry::ToolSource::Builtin => "builtin",
                        sisyphus_tools_mcp::tool_registry::ToolSource::Mcp { .. } => "mcp",
                        sisyphus_tools_mcp::tool_registry::ToolSource::Strategic => "strategic",
                        sisyphus_tools_mcp::tool_registry::ToolSource::Wasm => "wasm",
                    };
                    let params_json = info.parameters.map(|p| p.to_string()).unwrap_or_default();
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
            context: if req.context.is_empty() {
                None
            } else {
                Some(req.context)
            },
        };
        match self.team_orchestrator.decompose_goal(&goal).await {
            Ok(plan) => {
                let tasks: Vec<TaskProto> = plan
                    .tasks
                    .into_iter()
                    .map(|t| TaskProto {
                        id: t.id,
                        agent_id: t.agent_id,
                        tool_id: t.tool_id,
                        args_json: t.args.to_string(),
                        depends_on: t.depends_on,
                    })
                    .collect();
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

    async fn list_so_ps(
        &self,
        _request: Request<ListSoPsRequest>,
    ) -> Result<Response<ListSoPsResponse>, Status> {
        Ok(Response::new(ListSoPsResponse { sops: vec![] }))
    }

    type StreamExecutionStream = tonic::codegen::BoxStream<ExecutionEvent>;

    async fn stream_execution(
        &self,
        request: Request<ExecuteSopRequest>,
    ) -> Result<Response<Self::StreamExecutionStream>, Status> {
        let req = request.into_inner();
        let sop_id = req.sop_id.clone();

        let (tx, rx) = mpsc::unbounded_channel::<sisyphus_types::ExecutionEvent>();

        let kernel = self.kernel.clone();
        let default_fuel = self.default_fuel;

        tokio::spawn(async move {
            let _result: Result<(String, String), String> =
                if let Some(goal) = sop_id.strip_prefix("goal:") {
                    let synthesized_sop_id = format!("synth_{}", goal);
                    kernel
                        .execute_goal(goal, &synthesized_sop_id, Some(tx.clone()))
                        .await
                        .map(|(r, report)| {
                            (
                                format!("Result: {} | Cognition: {}", r, report),
                                synthesized_sop_id,
                            )
                        })
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
                    kernel
                        .execute_with_cognition(&graph, start_node, &sop_id, None, Some(tx))
                        .await
                        .map(|(r, report, _)| {
                            (format!("Result: {} | Cognition: {}", r, report), sop_id)
                        })
                };
        });

        let output_stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx).map(
            |event| -> Result<ExecutionEvent, Status> {
                Ok(match event {
                    sisyphus_types::ExecutionEvent::NodeStarted { node_name } => ExecutionEvent {
                        event_type: 0,
                        node_name,
                        status: "STARTED".into(),
                        message: "".into(),
                        timestamp: 0,
                    },
                    sisyphus_types::ExecutionEvent::NodeCompleted { node_name, output } => {
                        ExecutionEvent {
                            event_type: 1,
                            node_name,
                            status: "COMPLETED".into(),
                            message: output.to_string(),
                            timestamp: 0,
                        }
                    }
                    sisyphus_types::ExecutionEvent::NodeFailed { node_name, error } => {
                        ExecutionEvent {
                            event_type: 2,
                            node_name,
                            status: "FAILED".into(),
                            message: error,
                            timestamp: 0,
                        }
                    }
                    sisyphus_types::ExecutionEvent::ExecutionComplete { result } => {
                        ExecutionEvent {
                            event_type: 3,
                            node_name: "".into(),
                            status: "COMPLETE".into(),
                            message: result.to_string(),
                            timestamp: 0,
                        }
                    }
                })
            },
        );

        Ok(Response::new(Box::pin(output_stream)))
    }

    async fn execute_tool(
        &self,
        request: Request<ExecuteToolRequest>,
    ) -> Result<Response<ExecuteToolResponse>, Status> {
        let req = request.into_inner();
        let tool_id = req.tool_id.clone();
        let args_json = req.args_json;
        let fuel = if req.fuel_limit > 0 {
            req.fuel_limit
        } else {
            self.default_fuel
        };

        let span = info_span!("bridge.execute_tool", tool_id = %tool_id);
        let _enter = span.enter();

        let args_bytes = args_json.into_bytes();
        let result = self
            .kernel
            .tool_executor
            .execute(&tool_id, args_bytes, fuel)
            .await;

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
        let bm = self
            .kernel
            .benchmark
            .lock()
            .map_err(|e| Status::internal(format!("Benchmark lock poisoned: {e}")))?;
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
        request: Request<sisyphus_bridge_api::LearningInsightsRequest>,
    ) -> Result<Response<sisyphus_bridge_api::LearningInsightsResponse>, Status> {
        let filter = request.into_inner().strategy_filter;
        let ml = self
            .kernel
            .meta_learner
            .lock()
            .map_err(|e| Status::internal(e.to_string()))?;
        let insight = ml.learn();
        let strategies: Vec<sisyphus_bridge_api::StrategyInsight> = insight
            .strategies
            .iter()
            .filter(|s| filter.is_empty() || s.strategy == filter)
            .map(|s| sisyphus_bridge_api::StrategyInsight {
                strategy: s.strategy.clone(),
                total_uses: s.total_uses,
                successes: s.successes,
                failures: s.failures,
                avg_duration_secs: s.avg_duration_secs,
                success_rate: s.success_rate,
            })
            .collect();
        Ok(Response::new(sisyphus_bridge_api::LearningInsightsResponse {
            total_samples: insight.total_samples,
            overall_success_rate: insight.overall_success_rate,
            strategies,
        }))
    }

    // ── Skill handlers ────────────────────────────────────

    async fn list_skills(
        &self,
        _request: Request<ListSkillsRequest>,
    ) -> Result<Response<ListSkillsResponse>, Status> {
        let skills = self.skill_loader.load_all();
        let proto_skills: Vec<SkillMetadata> = skills
            .into_iter()
            .map(|s| {
                let tool_triggers: Vec<String> =
                    s.triggers.iter().flat_map(|t| t.tools.clone()).collect();
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
                        sisyphus_tools_session::skill::SkillSource::BuiltIn => "builtin".into(),
                        sisyphus_tools_session::skill::SkillSource::User => "user".into(),
                        sisyphus_tools_session::skill::SkillSource::Project => "project".into(),
                    },
                }
            })
            .collect();
        Ok(Response::new(ListSkillsResponse {
            skills: proto_skills,
        }))
    }

    async fn show_skill(
        &self,
        request: Request<ShowSkillRequest>,
    ) -> Result<Response<ShowSkillResponse>, Status> {
        let req = request.into_inner();
        let skills = self.skill_loader.load_all();
        if let Some(skill) = skills.into_iter().find(|s| s.name == req.name) {
            let tool_triggers: Vec<String> = skill
                .triggers
                .iter()
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
                        sisyphus_tools_session::skill::SkillSource::BuiltIn => "builtin".into(),
                        sisyphus_tools_session::skill::SkillSource::User => "user".into(),
                        sisyphus_tools_session::skill::SkillSource::Project => "project".into(),
                    },
                }),
            }))
        } else {
            Ok(Response::new(ShowSkillResponse {
                found: false,
                skill: None,
            }))
        }
    }

    async fn install_skill(
        &self,
        request: Request<InstallSkillRequest>,
    ) -> Result<Response<InstallSkillResponse>, Status> {
        let req = request.into_inner();
        let path = std::path::Path::new(&req.source_path);
        match sisyphus_tools_session::skill::install_skill(path) {
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
        match sisyphus_tools_session::skill::remove_skill(&req.name) {
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

    // ── Plugin handlers ──────────────────────────────────

    async fn list_plugins(
        &self,
        _request: Request<ListPluginsRequest>,
    ) -> Result<Response<ListPluginsResponse>, Status> {
        let plugins = self.plugin_manager.list_plugins();
        let descriptors: Vec<PluginDescriptor> = plugins
            .into_iter()
            .map(|p| PluginDescriptor {
                name: p.name,
                version: p.version,
                wasm_path: p.wasm_path.to_string_lossy().to_string(),
                tool_id: p.tool_id,
                fuel_limit: p.fuel_limit,
                checksum: p.checksum.unwrap_or_default(),
            })
            .collect();
        Ok(Response::new(ListPluginsResponse {
            plugins: descriptors,
        }))
    }

    async fn load_plugin(
        &self,
        request: Request<LoadPluginRequest>,
    ) -> Result<Response<LoadPluginResponse>, Status> {
        let req = request.into_inner();
        let descriptor = sisyphus_types::PluginDescriptor {
            name: req.name,
            version: req.version,
            wasm_path: std::path::PathBuf::from(&req.wasm_path),
            tool_id: req.tool_id,
            fuel_limit: if req.fuel_limit > 0 {
                req.fuel_limit
            } else {
                self.default_fuel
            },
            checksum: None,
        };
        match self.plugin_manager.register_plugin(descriptor).await {
            Ok(()) => Ok(Response::new(LoadPluginResponse {
                success: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(LoadPluginResponse {
                success: false,
                error: e,
            })),
        }
    }

    async fn unload_plugin(
        &self,
        request: Request<UnloadPluginRequest>,
    ) -> Result<Response<UnloadPluginResponse>, Status> {
        let name = request.into_inner().name;
        match self.plugin_manager.unload_plugin(&name) {
            Ok(()) => Ok(Response::new(UnloadPluginResponse {
                success: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(UnloadPluginResponse {
                success: false,
                error: e,
            })),
        }
    }

    // ── Chat handler ────────────────────────────────────

    async fn add_message(
        &self,
        request: Request<AddMessageRequest>,
    ) -> Result<Response<AddMessageResponse>, Status> {
        let req = request.into_inner();
        let db_path = std::path::PathBuf::from(&req.session_db_path);

        let session = match sisyphus_tools_session::session_get(&db_path, &req.session_id) {
            Ok(s) => s,
            Err(e) => {
                return Ok(Response::new(AddMessageResponse {
                    success: false,
                    error: format!("Failed to load session: {e}"),
                    response_content: String::new(),
                }));
            }
        };

        if session.messages.is_empty() {
            return Ok(Response::new(AddMessageResponse {
                success: false,
                error: "Session has no messages after load".into(),
                response_content: String::new(),
            }));
        }

        if req.role != "user" {
            return Ok(Response::new(AddMessageResponse {
                success: true,
                error: String::new(),
                response_content: String::new(),
            }));
        }

        let context = session
            .messages
            .iter()
            .map(|msg| format!("{}: {}", msg.role, msg.content))
            .collect::<Vec<_>>()
            .join("\n");

        let system_prompt = "You are Sisyphus, an AI assistant with cognitive engine capabilities. \
            Respond helpfully to the user's messages based on the conversation history.\
            Keep responses concise but informative.";

        match self.llm_client.chat(system_prompt, &context, false).await {
            Ok(response) => {
                if let Err(e) = sisyphus_tools_session::session_add_message(
                    &db_path,
                    &req.session_id,
                    "assistant",
                    &response,
                ) {
                    return Ok(Response::new(AddMessageResponse {
                        success: false,
                        error: format!("Failed to save assistant response: {e}"),
                        response_content: response,
                    }));
                }
                Ok(Response::new(AddMessageResponse {
                    success: true,
                    error: String::new(),
                    response_content: response,
                }))
            }
            Err(e) => Ok(Response::new(AddMessageResponse {
                success: false,
                error: format!("LLM chat failed: {e}"),
                response_content: String::new(),
            })),
        }
    }
}
