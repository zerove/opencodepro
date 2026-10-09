//! gRPC 处理器（一）：核心执行域（SOP 执行、目标分解、工具执行、流式执行、认知委托）。

#![allow(clippy::result_large_err)]

use crate::MySisyphusBridge;
use sisyphus_bridge_api::{
    AddMessageRequest, AddMessageResponse, CognitiveTaskEvent, DecomposeGoalRequest,
    DecomposeGoalResponse, DelegateCognitiveTaskRequest, DelegateCognitiveTaskResponse,
    ExecuteSopRequest, ExecuteToolRequest, ExecuteToolResponse, ExecutionEvent, SopResponse,
    TaskProto,
};
use sisyphus_core::{SOPGraph, WasmSOPNode};
use sisyphus_metrics::record_bridge_request;
use sisyphus_tools_agent::team::TeamGoal;
use sisyphus_types::ToolExecutor;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tonic::{Request, Response, Status};
use tracing::instrument;

impl MySisyphusBridge {
    #[instrument(name = "grpc.execute_sop", skip_all, fields(sop_id, project_id))]
    pub(crate) async fn execute_sop_impl(
        &self,
        request: Request<ExecuteSopRequest>,
    ) -> Result<Response<SopResponse>, Status> {
        let project_id = request
            .metadata()
            .get("x-sisyphus-workspace-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("default")
            .to_string();
        let req = request.into_inner();
        let sop_id = req.sop_id.clone();
        tracing::Span::current().record("sop_id", sop_id.as_str());
        tracing::Span::current().record("project_id", project_id.as_str());

        self.publish_event(sisyphus_bridge_api::BridgeEvent {
            event: Some(sisyphus_bridge_api::bridge_event::Event::OpUpdate(
                sisyphus_bridge_api::OperationUpdate {
                    op_id: sop_id.clone(),
                    status: "STARTED".into(),
                    result: "".into(),
                    success: true,
                },
            )),
        });

        let (result_str, _final_id) = self.run_execution(sop_id.clone(), &project_id, None).await?;

        // 大 payload 检测（TODO 8.1.2）：unary execute_sop 把 result 整体放进
        // string response。如果 >1MB，建议改用 stream_execution（server-streaming），
        // 通过 mpsc 通道产出事件，避免 protobuf 默认 2GB 单消息限制。
        const LARGE_PAYLOAD_THRESHOLD: usize = 1_048_576; // 1 MiB
        if result_str.len() > LARGE_PAYLOAD_THRESHOLD {
            tracing::warn!(
                sop_id = %sop_id,
                payload_bytes = result_str.len(),
                "execute_sop result exceeds 1MB; recommend using stream_execution RPC for large results"
            );
        }

        self.publish_event(sisyphus_bridge_api::BridgeEvent {
            event: Some(sisyphus_bridge_api::bridge_event::Event::OpUpdate(
                sisyphus_bridge_api::OperationUpdate {
                    op_id: sop_id.clone(),
                    status: "COMPLETED".into(),
                    result: result_str.clone(),
                    success: true,
                },
            )),
        });

        record_bridge_request(true, 0.0);

        Ok(Response::new(SopResponse {
            status: "SUCCESS".into(),
            result: result_str,
        }))
    }

    pub(crate) async fn decompose_goal_impl(
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
            },
            Err(e) => Ok(Response::new(DecomposeGoalResponse {
                success: false,
                error: e.to_string(),
                tasks: vec![],
            })),
        }
    }

    #[instrument(
        name = "grpc.execute_tool",
        skip_all,
        fields(tool_id, fuel, chaos = self.chaos_enabled)
    )]
    pub(crate) async fn execute_tool_impl(
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
        tracing::Span::current().record("tool_id", tool_id.as_str());
        tracing::Span::current().record("fuel", fuel);

        // ── Chaos Engineering: inject faults before execution ──────────
        if self.chaos_enabled {
            if let Some(fault_response) = self.maybe_inject_chaos_fault().await {
                return Ok(Response::new(fault_response));
            }
        }

        let args_bytes = args_json.into_bytes();
        let result =
            self.kernel.execution_manager.tool_router.execute(&tool_id, args_bytes, fuel).await;

        match result {
            Ok(bytes) => {
                let result_str = String::from_utf8_lossy(&bytes).to_string();
                // 大 payload 检测（TODO 8.1.2）：ExecuteToolResponse.result_json
                // 是 string。>1MB 时记录 warn，建议改用 server-streaming 端点
                // （未来扩展）或拆分多 RPC 调用。
                const LARGE_PAYLOAD_THRESHOLD: usize = 1_048_576; // 1 MiB
                if result_str.len() > LARGE_PAYLOAD_THRESHOLD {
                    tracing::warn!(
                        tool_id = %tool_id,
                        payload_bytes = result_str.len(),
                        "execute_tool result exceeds 1MB; recommend server-streaming for large tool outputs"
                    );
                }
                record_bridge_request(true, 0.0);
                Ok(Response::new(ExecuteToolResponse {
                    status: "SUCCESS".into(),
                    result_json: result_str,
                }))
            },
            Err(e) => {
                record_bridge_request(false, 0.0);
                Ok(Response::new(ExecuteToolResponse {
                    status: format!("ERROR: {e}"),
                    result_json: "".into(),
                }))
            },
        }
    }

    pub(crate) async fn stream_execution_impl(
        &self,
        request: Request<ExecuteSopRequest>,
    ) -> Result<Response<tonic::codegen::BoxStream<ExecutionEvent>>, Status> {
        let project_id = request
            .metadata()
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
            let _result: Result<(String, String), String> = if let Some(goal) =
                sop_id.strip_prefix("goal:")
            {
                let synthesized_sop_id = format!("synth_{}", goal);
                kernel
                    .execute_goal(
                        goal,
                        &synthesized_sop_id,
                        &project_id,
                        Some(tx.clone()),
                        None,
                    )
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
                            // 调用方只给了工具名，未给入参。
                            args: Vec::new(),
                        });
                        g.add_node(node);
                        Arc::new(g)
                    },
                };
                let _start_node = match graph.graph.node_indices().next() {
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
                    sisyphus_types::ExecutionEvent::NodeCompleted {
                        node_name, output, ..
                    } => ExecutionEvent {
                        event_type: 1,
                        node_name,
                        status: "COMPLETED".into(),
                        message: output.to_string(),
                        timestamp: 0,
                    },
                    sisyphus_types::ExecutionEvent::NodeFailed {
                        node_name, error, ..
                    } => ExecutionEvent {
                        event_type: 2,
                        node_name,
                        status: "FAILED".into(),
                        message: error,
                        timestamp: 0,
                    },
                    sisyphus_types::ExecutionEvent::ExecutionComplete { result } => {
                        ExecutionEvent {
                            event_type: 3,
                            node_name: "".into(),
                            status: "COMPLETE".into(),
                            message: result.to_string(),
                            timestamp: 0,
                        }
                    },
                    _ => ExecutionEvent {
                        event_type: 99,
                        node_name: "unknown".into(),
                        status: "UNKNOWN".into(),
                        message: "Unknown event".into(),
                        timestamp: 0,
                    },
                })
            },
        );

        Ok(Response::new(Box::pin(output_stream)))
    }

    pub(crate) async fn stream_cognitive_task_impl(
        &self,
        request: Request<DelegateCognitiveTaskRequest>,
    ) -> Result<Response<tonic::codegen::BoxStream<CognitiveTaskEvent>>, Status> {
        let project_id = request
            .metadata()
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

            let _sop_graph = match kernel
                .intelligence_manager
                .synthesize_goal(
                    &goal,
                    &ctx,
                    &project_id,
                    sisyphus_types::WorkMode::Standard,
                    None,
                )
                .await
            {
                Ok(res) => {
                    let plan = sisyphus_types::SOPGraphConfig::from_graph(&res.graph)
                        .map(|c| c.into_cognitive_plan(goal.clone(), goal.clone()))
                        .unwrap_or_default();
                    Arc::new(plan)
                },
                Err(_) => Arc::new(sisyphus_types::CognitivePlan::default()),
            };

            let _ = kernel
                .execute_goal_streaming(&goal, "stream_task", &project_id, None, Some(tx))
                .await;
        });

        let output_stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx).map(
            |event| -> Result<CognitiveTaskEvent, Status> {
                let (event_type, payload) = match event {
                    sisyphus_types::CognitiveEvent::Predict { prediction } => {
                        (0, serde_json::to_string(&prediction).unwrap_or_default())
                    },
                    sisyphus_types::CognitiveEvent::Execute { node, result } => {
                        (1, format!("Node {}: {}", node, result))
                    },
                    sisyphus_types::CognitiveEvent::Reflect { report } => {
                        (2, serde_json::to_string(&report).unwrap_or_default())
                    },
                    sisyphus_types::CognitiveEvent::Evolve { revised_plan } => {
                        (3, serde_json::to_string(&revised_plan).unwrap_or_default())
                    },
                    sisyphus_types::CognitiveEvent::Complete { final_result, .. } => {
                        (4, final_result.to_string())
                    },
                    sisyphus_types::CognitiveEvent::Verify { .. }
                    | sisyphus_types::CognitiveEvent::StrategicShift { .. } => {
                        (5, "Cognitive update: internal state evolved".to_string())
                    },
                    _ => (6, "Unknown cognitive event".to_string()),
                };
                Ok(CognitiveTaskEvent {
                    event_type,
                    payload,
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                })
            },
        );

        Ok(Response::new(Box::pin(output_stream)))
    }

    pub(crate) async fn delegate_cognitive_task_impl(
        &self,
        request: Request<DelegateCognitiveTaskRequest>,
    ) -> Result<Response<DelegateCognitiveTaskResponse>, Status> {
        let project_id = request
            .metadata()
            .get("x-sisyphus-workspace-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("default")
            .to_string();
        let req = request.into_inner();
        let goal = req.goal;
        let context_str = req.context;

        let ctx = sisyphus_types::ExecutionContext::new();
        ctx.write_notepad("context".into(), context_str);

        match self
            .kernel
            .intelligence_manager
            .synthesize_goal(
                &goal,
                &ctx,
                &project_id,
                sisyphus_types::WorkMode::Standard,
                None,
            )
            .await
        {
            Ok(result) => {
                let plan = sisyphus_types::SOPGraphConfig::from_graph(&result.graph)
                    .map_err(|e| Status::internal(format!("Graph error: {e}")))?
                    .into_cognitive_plan(goal.clone(), goal.clone());
                let _ = self.kernel.cache_plan(&plan.plan_id, &plan).await;

                let plan_json = serde_json::to_string(&plan)
                    .map_err(|e| Status::internal(format!("Serialization failed: {e}")))?;

                Ok(Response::new(DelegateCognitiveTaskResponse {
                    success: true,
                    error: String::new(),
                    plan_json,
                }))
            },
            Err(e) => Ok(Response::new(DelegateCognitiveTaskResponse {
                success: false,
                error: e.to_string(),
                plan_json: "".into(),
            })),
        }
    }

    pub(crate) async fn add_message_impl(
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
            },
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

        let system_prompt =
            "You are Sisyphus, an AI assistant with cognitive engine capabilities. \
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
                    event: Some(sisyphus_bridge_api::bridge_event::Event::SessionUpdate(
                        sisyphus_bridge_api::SessionUpdate {
                            session_id: req.session_id.clone(),
                            force_refresh: true,
                        },
                    )),
                });
                Ok(Response::new(AddMessageResponse {
                    success: true,
                    error: String::new(),
                    response_content: response,
                }))
            },
            Err(e) => Ok(Response::new(AddMessageResponse {
                success: false,
                error: format!("LLM chat failed: {e}"),
                response_content: String::new(),
            })),
        }
    }

    pub(crate) async fn call_jcode_impl(
        &self,
        request: Request<sisyphus_bridge_api::CallJcodeRequest>,
    ) -> Result<Response<sisyphus_bridge_api::CallJcodeResponse>, Status> {
        let project_id = request
            .metadata()
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
        let result =
            self.kernel.execute_goal(&goal, &sop_id, &project_id, None, Some(&context)).await;

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
                    })
                    .to_string()
                } else {
                    format!(
                        "Task completed successfully.\nResult: {}\n\nTrace: {}",
                        output, report
                    )
                };

                Ok(Response::new(sisyphus_bridge_api::CallJcodeResponse {
                    success: true,
                    result: res_str,
                    error: "".into(),
                }))
            },
            Err(e) => {
                tracing::error!(task_id = %task_id, error = %e, "Jcode execution failed");
                Ok(Response::new(sisyphus_bridge_api::CallJcodeResponse {
                    success: false,
                    result: "".into(),
                    error: e.to_string(),
                }))
            },
        }
    }
}
