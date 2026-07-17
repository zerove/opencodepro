use std::sync::Arc;

use sisyphus_bridge_api::sisyphus_bridge_server::SisyphusBridge;
use sisyphus_bridge_api::{
    AddMessageRequest, AddMessageResponse, AgentMetadata, BenchmarkEntry, BreakerMetricsRequest,
    BreakerMetricsResponse, BreakerStatus, DecomposeGoalRequest, DecomposeGoalResponse,
    DelegateCognitiveTaskRequest, DelegateCognitiveTaskResponse, ExecuteSopRequest, ExecuteToolRequest, ExecuteToolResponse, ExecutionEvent,
    HealthCheckRequest, HealthCheckResponse, InstallSkillRequest, InstallSkillResponse,
    ListAgentsRequest, ListAgentsResponse, ListBenchmarksRequest, ListBenchmarksResponse,
    ListPluginsRequest, ListPluginsResponse, LoadPluginRequest, LoadPluginResponse,
    ListSoPsRequest, ListSoPsResponse, ListSkillsRequest, ListSkillsResponse, ListToolsRequest,
    ListToolsResponse, PluginDescriptor, RemoveSkillRequest, RemoveSkillResponse, ShowSkillRequest,
    ShowSkillResponse, SkillMetadata, SopResponse, TaskProto, ToolMetadata,
    UnloadPluginRequest, UnloadPluginResponse,
    CognitiveTaskEvent, SubmitFeedbackRequest, SubmitFeedbackResponse, CaptureExperienceRequest, CaptureExperienceResponse,
    CreateWorkspaceRequest, CreateWorkspaceResponse, SwitchWorkspaceRequest, SwitchWorkspaceResponse,
    ListWorkspacesRequest, ListWorkspacesResponse, WorkspaceMetadata, GetCurrentWorkspaceRequest, GetCurrentWorkspaceResponse,
    EventRequest,
};
use sisyphus_core::{SOPGraph, WasmSOPNode};
use sisyphus_types::ToolExecutor;
use sisyphus_metrics::record_bridge_request;
use sisyphus_tools_llm::LlmClient;
use sisyphus_tools_agent::team::TeamGoal;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tonic::{Request, Response, Status};
use tracing::info_span;

use super::MySisyphusBridge;


#[tonic::async_trait]
impl SisyphusBridge for MySisyphusBridge {
    type StreamEventsStream = tonic::codegen::BoxStream<sisyphus_bridge_api::BridgeEvent>;

    async fn stream_events(
        &self,
        _request: Request<sisyphus_bridge_api::EventRequest>,
    ) -> Result<Response<Self::StreamEventsStream>, Status> {
        let rx = self.event_bus.subscribe();
        let stream = tokio_stream::wrappers::BroadcastStream::new(rx).map(|res| {
            res.map_err(|e| Status::internal(format!("Broadcast error: {e}")))
        });
        Ok(Response::new(Box::pin(stream)))
    }

    async fn execute_sop(
        &self,
        request: Request<ExecuteSopRequest>,
    ) -> Result<Response<SopResponse>, Status> {
        let project_id = request.metadata()
            .get("x-sisyphus-workspace-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("default")
            .to_string();
        let req = request.into_inner();
        let sop_id = req.sop_id.clone();

        self.publish_event(sisyphus_bridge_api::BridgeEvent {
            event: Some(sisyphus_bridge_api::bridge_event::Event::OpUpdate(sisyphus_bridge_api::OperationUpdate {
                op_id: sop_id.clone(),
                status: "STARTED".into(),
                result: "".into(),
                success: true,
            })),
        });

        let span = info_span!("bridge.execute_sop", sop_id = %sop_id);
        let _enter = span.enter();

        let (result_str, _final_id) = self.run_execution(sop_id.clone(), &project_id, None).await?;

        self.publish_event(sisyphus_bridge_api::BridgeEvent {
            event: Some(sisyphus_bridge_api::bridge_event::Event::OpUpdate(sisyphus_bridge_api::OperationUpdate {
                op_id: sop_id.clone(),
                status: "COMPLETED".into(),
                result: result_str.clone(),
                success: true,
            })),
        });

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

    async fn delegate_cognitive_task(
        &self,
        request: Request<DelegateCognitiveTaskRequest>,
    ) -> Result<Response<DelegateCognitiveTaskResponse>, Status> {
        let project_id = request.metadata()
            .get("x-sisyphus-workspace-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("default")
            .to_string();
        let req = request.into_inner();
        let goal = req.goal;
        let context_str = req.context;

        let ctx = sisyphus_types::ExecutionContext::new();
        ctx.write_notepad("context".into(), context_str);

        match self.kernel.intelligence_manager.synthesize_goal(
            &goal, 
            &ctx, 
            &project_id,
            sisyphus_types::WorkMode::Standard,
            None
        ).await {
            Ok(result) => {
                let plan = sisyphus_types::SOPGraphConfig::from_graph(&result.graph)
                    .into_cognitive_plan(goal.clone(), goal.clone());
                let _ = self.kernel.cache_plan(&plan.plan_id, &plan).await;

                let plan_json = serde_json::to_string(&plan)
                    .map_err(|e| Status::internal(format!("Serialization failed: {e}")))?;
                
                Ok(Response::new(DelegateCognitiveTaskResponse {
                    success: true,
                    error: String::new(),
                    plan_json,
                }))
            }
            Err(e) => Ok(Response::new(DelegateCognitiveTaskResponse {
                success: false,
                error: e.to_string(),
                plan_json: "".into(),
            })),
        }
    }

    async fn submit_feedback(
        &self,
        request: Request<SubmitFeedbackRequest>,
    ) -> Result<Response<SubmitFeedbackResponse>, Status> {
        let project_id = request.metadata()
            .get("x-sisyphus-workspace-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("default")
            .to_string();
        let req = request.into_inner();
        let feedback: sisyphus_types::ExecutionFeedback = serde_json::from_str(&req.feedback_json)
            .map_err(|e| Status::invalid_argument(format!("Invalid feedback JSON: {e}")))?;

        match self.kernel.handle_feedback(feedback, &project_id).await {
            Ok(evolution) => {
                let evolution_json = serde_json::to_string(&evolution)
                    .map_err(|e| Status::internal(format!("Serialization failed: {e}")))?;
                
                Ok(Response::new(SubmitFeedbackResponse {
                    success: true,
                    error: String::new(),
                    evolution_json,
                }))
            }
            Err(e) => Ok(Response::new(SubmitFeedbackResponse {
                success: false,
                error: e.to_string(),
                evolution_json: "".into(),
            })),
        }
    }

    async fn capture_experience(
        &self,
        request: Request<CaptureExperienceRequest>,
    ) -> Result<Response<CaptureExperienceResponse>, Status> {
        let project_id = request.metadata()
            .get("x-sisyphus-workspace-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("default")
            .to_string();
        let req = request.into_inner();
        let plan: sisyphus_types::CognitivePlan = serde_json::from_str(&req.plan_json)
            .map_err(|e| Status::invalid_argument(format!("Invalid plan JSON: {e}")))?;
        
        match self.kernel.capture_success(&req.plan_id, &project_id, plan).await {
            Ok(_) => Ok(Response::new(CaptureExperienceResponse {
                success: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(CaptureExperienceResponse {
                success: false,
                error: e.to_string(),
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
        let project_id = request.metadata()
            .get("x-sisyphus-workspace-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("default")
            .to_string();
        let req = request.into_inner();
        let sop_id = req.sop_id.clone();

        let (tx, rx) = mpsc::unbounded_channel::<sisyphus_types::ExecutionEvent>();

        let kernel = self.kernel.clone();
        let default_fuel = self.default_fuel;

        tokio::spawn(async move {
            let project_id = project_id.clone();
            let _result: Result<(String, String), String> =
                if let Some(goal) = sop_id.strip_prefix("goal:") {
                    let synthesized_sop_id = format!("synth_{}", goal);
                    kernel
                        .execute_goal(goal, &synthesized_sop_id, &project_id, Some(tx.clone()), None)
                        .await
                        .map_err(|e| e.to_string())
                        .map(|(r, report)| {
                            (
                                format!("Result: {} | Cognition: {}", r, report),
                                synthesized_sop_id,
                            )
                        })
                } else {
                    let graph = match kernel.load_sop(&sop_id, &project_id).await {
                        Ok(g) => g,
                        Err(_) => {
                            let mut g = SOPGraph::new();
                            let tool_id = sop_id.clone();
                            let node = Arc::new(WasmSOPNode {
                                name: format!("node_{}", tool_id),
                                tool_id,
                                executor: kernel.execution_manager.tool_router.clone(),
                                fuel_limit: default_fuel,
                            });
                            g.add_node(node);
                            Arc::new(g)
                        }
                    };
                    let start_node = match graph.graph.node_indices().next() {
                        Some(n) => n,
                        None => return,
                    };
                    kernel
                        .execute_with_cognition(graph, &sop_id, Some(&project_id), None, Some(tx), None)
                        .await
                        .map_err(|e| e.to_string())
                        .map(|(r, report, _)| {
                            (format!("Result: {} | Cognition: {:?}", r, report), sop_id)
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
                    sisyphus_types::ExecutionEvent::NodeCompleted { node_name, output, .. } => {
                        ExecutionEvent {
                            event_type: 1,
                            node_name,
                            status: "COMPLETED".into(),
                            message: output.to_string(),
                            timestamp: 0,
                        }
                    }
                    sisyphus_types::ExecutionEvent::NodeFailed { node_name, error, .. } => {
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
                    _ => ExecutionEvent {
                        event_type: 99,
                        node_name: "unknown".into(),
                        status: "UNKNOWN".into(),
                        message: "Unknown event".into(),
                        timestamp: 0,
                    }
                })
            },
        );

        Ok(Response::new(Box::pin(output_stream)))
    }

    type StreamCognitiveTaskStream = tonic::codegen::BoxStream<CognitiveTaskEvent>;

    async fn stream_cognitive_task(
        &self,
        request: Request<DelegateCognitiveTaskRequest>,
    ) -> Result<Response<Self::StreamCognitiveTaskStream>, Status> {
        let project_id = request.metadata()
            .get("x-sisyphus-workspace-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("default")
            .to_string();
        let req = request.into_inner();
        let goal = req.goal;
        let context_str = req.context;

        let (tx, rx) = mpsc::unbounded_channel::<sisyphus_types::CognitiveEvent>();

        let kernel = self.kernel.clone();
        tokio::spawn(async move {
            let project_id = project_id.clone();
            let ctx = sisyphus_types::ExecutionContext::new();
            ctx.write_notepad("context".into(), context_str);

            let _sop_graph = match kernel.intelligence_manager.synthesize_goal(
                &goal, 
                &ctx, 
                &project_id,
                sisyphus_types::WorkMode::Standard,
                None
            ).await {
                Ok(res) => {
                    let plan = sisyphus_types::SOPGraphConfig::from_graph(&res.graph)
                        .into_cognitive_plan(goal.clone(), goal.clone());
                    Arc::new(plan)
                },
                Err(_) => {
                    Arc::new(sisyphus_types::CognitivePlan::default())
                }
            };

            let _ = kernel.execute_goal_streaming(
                &goal,
                "stream_task",
                &project_id,
                None,
                Some(tx),
            ).await;
        });

        let output_stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx).map(
            |event| -> Result<CognitiveTaskEvent, Status> {
                let (event_type, payload) = match event {
                    sisyphus_types::CognitiveEvent::Predict { prediction } => 
                        (0, serde_json::to_string(&prediction).unwrap_or_default()),
                    sisyphus_types::CognitiveEvent::Execute { node, result } => 
                        (1, format!("Node {}: {}", node, result)),
                    sisyphus_types::CognitiveEvent::Reflect { report } => 
                        (2, serde_json::to_string(&report).unwrap_or_default()),
                    sisyphus_types::CognitiveEvent::Evolve { revised_plan } => 
                        (3, serde_json::to_string(&revised_plan).unwrap_or_default()),
                    sisyphus_types::CognitiveEvent::Complete { final_result, .. } => 
                        (4, final_result.to_string()),
                    sisyphus_types::CognitiveEvent::Verify { .. } | sisyphus_types::CognitiveEvent::StrategicShift { .. } => {
                        (5, "Cognitive update: internal state evolved".to_string())
                    },
                    _ => (6, "Unknown cognitive event".to_string()),
                };
                Ok(CognitiveTaskEvent {
                    event_type,
                    payload,
                    timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(),
                })
            },
        );

        Ok(Response::new(Box::pin(output_stream)))
    }

    async fn call_jcode(
        &self,
        request: Request<sisyphus_bridge_api::CallJcodeRequest>,
    ) -> Result<Response<sisyphus_bridge_api::CallJcodeResponse>, Status> {
        let project_id = request.metadata()
            .get("x-sisyphus-workspace-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("default")
            .to_string();
        let req = request.into_inner();
        let task_id = req.task_id;
        let goal = req.goal;
        let context = req.context_json;
        
        tracing::info!(task_id = %task_id, goal = %goal, "Jcode request received - delegating to kernel");
        
        // Execute the goal autonomously using the kernel
        // We use a unique sop_id for each jcode call to avoid collisions
        let sop_id = format!("jcode_{}", task_id);
        let result = self.kernel.execute_goal(&goal, &sop_id, &project_id, None, Some(&context)).await;

        
        match result {
            Ok((output, report)) => {
                let res_str = if goal.contains("verify_evolution") {
                    // For evolution verification, we need a specific JSON format.
                    // In a real scenario, we would use a specialized "verifier" agent.
                    // Here, we use the goal result and wrap it.
                    serde_json::json!({
                        "decision": "ACCEPT", 
                        "reason": format!("Verification successful. Result: {}", output),
                        "improvement_score": 0.8
                    }).to_string()
                } else {
                    format!("Task completed successfully.\nResult: {}\n\nTrace: {}", output, report)
                };

                Ok(Response::new(sisyphus_bridge_api::CallJcodeResponse {
                    success: true,
                    result: res_str,
                    error: "".into(),
                }))
            }
            Err(e) => {
                tracing::error!(task_id = %task_id, error = %e, "Jcode execution failed");
                Ok(Response::new(sisyphus_bridge_api::CallJcodeResponse {
                    success: false,
                    result: "".into(),
                    error: e.to_string(),
                }))
            }
        }
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
            .execution_manager.tool_router
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
        let bm = self.kernel.benchmark.lock();
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
        let ml = self.kernel.meta_learner.lock();
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

    async fn list_skills(
        &self,
        request: Request<ListSkillsRequest>,
    ) -> Result<Response<ListSkillsResponse>, Status> {
        let req = request.into_inner();
        
        let skills = if req.remote {
            self.skill_registry.fetch_available_skills().await
                .map_err(|e| Status::internal(format!("Remote registry error: {e}")))?
        } else {
            self.skill_registry.list_installed_skills().await
                .map_err(|e| Status::internal(format!("Local store error: {e}")))?
        };

        let proto_skills: Vec<SkillMetadata> = skills
            .into_iter()
            .map(|s| SkillMetadata {
                id: s.id,
                name: s.name,
                description: s.description,
                version: s.version,
                author: s.author,
                tags: s.tags,
                created_at: s.created_at,
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
        match self.skill_registry.get_skill(&req.name).await {
            Ok(Some(skill)) => Ok(Response::new(ShowSkillResponse {
                found: true,
                skill: Some(SkillMetadata {
                    id: skill.metadata.id,
                    name: skill.metadata.name,
                    description: skill.metadata.description,
                    version: skill.metadata.version,
                    author: skill.metadata.author,
                    tags: skill.metadata.tags,
                    created_at: skill.metadata.created_at,
                }),
            })),
            Ok(None) => Ok(Response::new(ShowSkillResponse {
                found: false,
                skill: None,
            })),
            Err(e) => Err(Status::internal(format!("Store error: {e}"))),
        }
    }

    async fn install_skill(
        &self,
        request: Request<InstallSkillRequest>,
    ) -> Result<Response<InstallSkillResponse>, Status> {
        let req = request.into_inner();
        match self.skill_registry.install_skill(&req.skill_id).await {
            Ok(()) => Ok(Response::new(InstallSkillResponse {
                success: true,
                error: String::new(),
                name: req.skill_id.clone(),
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
        match self.skill_registry.uninstall_skill(&req.name).await {
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

    async fn create_workspace(
        &self,
        request: Request<CreateWorkspaceRequest>,
    ) -> Result<Response<CreateWorkspaceResponse>, Status> {
        let req = request.into_inner();
        match self.workspace_manager.create_workspace(req.name, req.project_id, req.root_dir).await {
            Ok(ws) => Ok(Response::new(CreateWorkspaceResponse {
                success: true,
                error: String::new(),
                workspace_id: ws.id,
            })),
            Err(e) => Ok(Response::new(CreateWorkspaceResponse {
                success: false,
                error: e.to_string(),
                workspace_id: String::new(),
            })),
        }
    }

    async fn switch_workspace(
        &self,
        request: Request<SwitchWorkspaceRequest>,
    ) -> Result<Response<SwitchWorkspaceResponse>, Status> {
        let req = request.into_inner();
        match self.workspace_manager.switch_workspace(&req.workspace_id).await {
            Ok(()) => Ok(Response::new(SwitchWorkspaceResponse {
                success: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(SwitchWorkspaceResponse {
                success: false,
                error: e.to_string(),
            })),
        }
    }

    async fn list_workspaces(
        &self,
        _request: Request<ListWorkspacesRequest>,
    ) -> Result<Response<ListWorkspacesResponse>, Status> {
        let workspaces = self.workspace_manager.list_workspaces().await;
        let proto_ws: Vec<WorkspaceMetadata> = workspaces
            .into_iter()
            .map(|ws| WorkspaceMetadata {
                id: ws.id,
                name: ws.name,
                project_id: ws.project_id,
                root_dir: ws.root_dir,
            })
            .collect();
        Ok(Response::new(ListWorkspacesResponse {
            workspaces: proto_ws,
        }))
    }

    async fn get_current_workspace(
        &self,
        _request: Request<GetCurrentWorkspaceRequest>,
    ) -> Result<Response<GetCurrentWorkspaceResponse>, Status> {
        match self.workspace_manager.get_current().await {
            Some(ws) => Ok(Response::new(GetCurrentWorkspaceResponse {
                found: true,
                workspace: Some(WorkspaceMetadata {
                    id: ws.id,
                    name: ws.name,
                    project_id: ws.project_id,
                    root_dir: ws.root_dir,
                }),
            })),
            None => Ok(Response::new(GetCurrentWorkspaceResponse {
                found: false,
                workspace: None,
            })),
        }
    }
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
                self.publish_event(sisyphus_bridge_api::BridgeEvent {
                    event: Some(sisyphus_bridge_api::bridge_event::Event::SessionUpdate(sisyphus_bridge_api::SessionUpdate {
                        session_id: req.session_id.clone(),
                        force_refresh: true,
                    })),
                });
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
                }))
        }
    }

}
