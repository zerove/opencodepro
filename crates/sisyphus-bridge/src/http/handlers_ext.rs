//! REST API 处理器（二）：技能市场/对等注册表、工具执行、反馈、混沌工程、插件。

use super::types::*;
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use sisyphus_types::types::BenchmarkRun;
use sisyphus_types::ToolExecutor;
use uuid::Uuid;

/// 从请求头或请求体提取 project_id，默认 "default"。
fn extract_project_id<'a>(headers: &'a HeaderMap, body_project_id: Option<&'a str>) -> &'a str {
    headers
        .get("x-project-id")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .or(body_project_id)
        .unwrap_or("default")
}

/// 将 BenchmarkRun 转换为 BenchmarkEntry。
fn benchmark_run_to_entry(run: &BenchmarkRun) -> BenchmarkEntry {
    BenchmarkEntry {
        name: run.name.clone(),
        duration_secs: run.duration_secs,
        timestamp: run.timestamp,
        success: run.success,
        metrics_json: serde_json::to_string(&run.metrics).unwrap_or_default(),
    }
}

// ── Missing gRPC Service Handlers ────────────────────────────────────────────

pub(super) async fn show_skill_handler(
    State(state): State<RestApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.bridge.skill_registry.get_skill(&id).await {
        Ok(Some(skill)) => (
            StatusCode::OK,
            Json(SkillDetailResponse {
                found: true,
                skill: Some(skill.metadata),
            }),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(SkillDetailResponse {
                found: false,
                skill: None,
            }),
        )
            .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

// ── Skill Market & Peer Registry Handlers ───────────────────────────────

pub(super) async fn search_skills_handler(
    State(state): State<RestApiState>,
    req: axum::extract::Query<SearchSkillsRequest>,
) -> impl IntoResponse {
    match state.bridge.skill_market.search(&req.q).await {
        Ok(results) => (StatusCode::OK, Json(SkillSearchResponse { results })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

pub(super) async fn publish_skill_handler(
    State(state): State<RestApiState>,
    Json(req): Json<PublishSkillRequest>,
) -> impl IntoResponse {
    // Export the locally installed skill bundle.
    let bundle = match state.bridge.skill_store.get(&req.skill_id).await {
        Ok(Some(bundle)) => bundle,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(PublishSkillResponse {
                    success: false,
                    entry: None,
                    error: format!("Skill {} not installed locally", req.skill_id),
                }),
            )
                .into_response()
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(PublishSkillResponse {
                    success: false,
                    entry: None,
                    error: e.to_string(),
                }),
            )
                .into_response()
        }
    };

    // Publish to the remote registry (peer protocol: POST {registry}/api/v1/skills).
    match state
        .bridge
        .skill_market
        .publish(&req.registry_url, &bundle)
        .await
    {
        Ok(entry) => (
            StatusCode::OK,
            Json(PublishSkillResponse {
                success: true,
                entry: Some(entry),
                error: String::new(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(PublishSkillResponse {
                success: false,
                entry: None,
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

// ── Peer Registry Protocol (this instance acts as a distributed registry) ──

/// List skills this instance can serve to peers.
pub(super) async fn peer_list_skills_handler(
    State(state): State<RestApiState>,
) -> impl IntoResponse {
    match state.bridge.skill_store.list().await {
        Ok(skills) => (StatusCode::OK, Json(PeerSkillListResponse { skills })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

/// Serve a full skill bundle to a peer for installation.
pub(super) async fn peer_get_skill_handler(
    State(state): State<RestApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.bridge.skill_store.get(&id).await {
        Ok(Some(bundle)) => (StatusCode::OK, Json(bundle)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Skill not found" })),
        )
            .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

/// Receive a skill bundle published by a peer and install it locally.
pub(super) async fn peer_publish_skill_handler(
    State(state): State<RestApiState>,
    Json(bundle): Json<sisyphus_types::skill::SkillBundle>,
) -> impl IntoResponse {
    match state.bridge.skill_store.install(bundle).await {
        Ok(_) => StatusCode::CREATED.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

pub(super) async fn execute_tool_handler(
    State(state): State<RestApiState>,
    Json(req): Json<ExecuteToolRequest>,
) -> impl IntoResponse {
    let fuel = req.fuel_limit.unwrap_or(state.bridge.default_fuel);
    let args_bytes = req.args_json.into_bytes();
    match state
        .bridge
        .kernel()
        .execution_manager
        .tool_router
        .execute(&req.tool_id, args_bytes, fuel)
        .await
    {
        Ok(bytes) => {
            let result_str = String::from_utf8_lossy(&bytes).to_string();
            (
                StatusCode::OK,
                Json(ExecuteToolResponse {
                    status: "SUCCESS".into(),
                    result_json: result_str,
                }),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ExecuteToolResponse {
                status: format!("ERROR: {e}"),
                result_json: String::new(),
            }),
        )
            .into_response(),
    }
}

pub(super) async fn delegate_cognitive_task_handler(
    State(state): State<RestApiState>,
    headers: HeaderMap,
    Json(req): Json<DelegateCognitiveTaskRequest>,
) -> impl IntoResponse {
    let project_id = extract_project_id(&headers, req.project_id.as_deref());
    let ctx = sisyphus_types::ExecutionContext::new();
    ctx.write_notepad("context".into(), req.context);
    match state
        .bridge
        .kernel()
        .intelligence_manager
        .synthesize_goal(
            &req.goal,
            &ctx,
            project_id,
            sisyphus_types::WorkMode::Standard,
            None,
        )
        .await
    {
        Ok(result) => {
            let graph_config = match sisyphus_types::SOPGraphConfig::from_graph(&result.graph) {
                Ok(c) => c,
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(DelegateCognitiveTaskResponse {
                            success: false,
                            error: format!("Graph error: {e}"),
                            plan_json: "".into(),
                        }),
                    )
                        .into_response();
                }
            };
            let plan = graph_config.into_cognitive_plan(req.goal.clone(), req.goal.clone());
            let _ = state.bridge.kernel().cache_plan(&plan.plan_id, &plan).await;
            let plan_json = match serde_json::to_string(&plan) {
                Ok(s) => s,
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(DelegateCognitiveTaskResponse {
                            success: false,
                            error: format!("Serialization failed: {e}"),
                            plan_json: "".into(),
                        }),
                    )
                        .into_response();
                }
            };
            (
                StatusCode::OK,
                Json(DelegateCognitiveTaskResponse {
                    success: true,
                    error: String::new(),
                    plan_json,
                }),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(DelegateCognitiveTaskResponse {
                success: false,
                error: e.to_string(),
                plan_json: "".into(),
            }),
        )
            .into_response(),
    }
}

pub(super) async fn submit_feedback_handler(
    State(state): State<RestApiState>,
    headers: HeaderMap,
    Json(req): Json<SubmitFeedbackRequest>,
) -> impl IntoResponse {
    let project_id = extract_project_id(&headers, req.project_id.as_deref());
    let feedback: sisyphus_types::ExecutionFeedback = match serde_json::from_str(&req.feedback_json)
    {
        Ok(f) => f,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(SubmitFeedbackResponse {
                    success: false,
                    error: format!("Invalid feedback JSON: {e}"),
                    evolution_json: "".into(),
                }),
            )
                .into_response()
        }
    };
    match state
        .bridge
        .kernel()
        .handle_feedback(feedback, project_id)
        .await
    {
        Ok(evolution) => {
            let evolution_json = serde_json::to_string(&evolution).unwrap_or_default();
            (
                StatusCode::OK,
                Json(SubmitFeedbackResponse {
                    success: true,
                    error: String::new(),
                    evolution_json,
                }),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(SubmitFeedbackResponse {
                success: false,
                error: e.to_string(),
                evolution_json: "".into(),
            }),
        )
            .into_response(),
    }
}

pub(super) async fn capture_experience_handler(
    State(state): State<RestApiState>,
    headers: HeaderMap,
    Json(req): Json<CaptureExperienceRequest>,
) -> impl IntoResponse {
    let project_id = extract_project_id(&headers, req.project_id.as_deref());
    let plan: sisyphus_types::CognitivePlan = match serde_json::from_str(&req.plan_json) {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(CaptureExperienceResponse {
                    success: false,
                    error: format!("Invalid plan JSON: {e}"),
                }),
            )
                .into_response()
        }
    };
    match state
        .bridge
        .kernel()
        .capture_success(&req.plan_id, project_id, plan)
        .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(CaptureExperienceResponse {
                success: true,
                error: String::new(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(CaptureExperienceResponse {
                success: false,
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

pub(super) async fn share_experience_handler(
    State(state): State<RestApiState>,
    Json(req): Json<ShareExperienceRequest>,
) -> impl IntoResponse {
    match state
        .bridge
        .experience_bank
        .share_experience(&req.exp_key)
        .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(ShareExperienceResponse {
                success: true,
                error: String::new(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ShareExperienceResponse {
                success: false,
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

pub(super) async fn unshare_experience_handler(
    State(state): State<RestApiState>,
    Json(req): Json<ShareExperienceRequest>,
) -> impl IntoResponse {
    let project_id = match req.project_id {
        Some(pid) if !pid.is_empty() => pid,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ShareExperienceResponse {
                    success: false,
                    error: "project_id is required to unshare".into(),
                }),
            )
                .into_response()
        }
    };

    match state
        .bridge
        .experience_bank
        .unshare_experience(&req.exp_key, &project_id)
        .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(ShareExperienceResponse {
                success: true,
                error: String::new(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ShareExperienceResponse {
                success: false,
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

pub(super) async fn list_shared_experiences_handler(
    State(state): State<RestApiState>,
) -> impl IntoResponse {
    match state.bridge.experience_bank.list_shared().await {
        Ok(experiences) => (
            StatusCode::OK,
            Json(SharedExperiencesResponse { experiences }),
        )
            .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}

pub(super) async fn list_benchmarks_handler(
    State(state): State<RestApiState>,
    _req: axum::extract::Query<ListBenchmarksRequest>,
) -> impl IntoResponse {
    let benchmark = state.bridge.kernel().benchmark.lock();
    let names = benchmark.list_benchmarks();
    let mut benchmarks = Vec::new();
    for name in names {
        let trend = benchmark.get_trend(&name, 100); // 获取最近 100 次运行
        let entries: Vec<BenchmarkEntry> = trend.iter().map(benchmark_run_to_entry).collect();
        benchmarks.extend(entries);
    }
    (StatusCode::OK, Json(ListBenchmarksResponse { benchmarks })).into_response()
}

pub(super) async fn get_breaker_metrics_handler(
    State(_state): State<RestApiState>,
    req: axum::extract::Query<BreakerMetricsRequest>,
) -> impl IntoResponse {
    // Note: Circuit breaker metrics are tracked per-model in the LLM client.
    // The bridge stores LLM client as a trait object; exposing breaker internals
    // would require downcasting or a dedicated metrics API on LlmClient trait.
    // For now, return empty list; extend LlmClient trait if needed.
    let _ = req; // suppress unused warning
    (
        StatusCode::OK,
        Json(BreakerMetricsResponse { breakers: vec![] }),
    )
        .into_response()
}

pub(super) async fn get_learning_insights_handler(
    State(state): State<RestApiState>,
    req: axum::extract::Query<LearningInsightsRequest>,
) -> impl IntoResponse {
    let filter = req.strategy_filter.clone().unwrap_or_default();
    let ml = state.bridge.kernel().meta_learner.lock();
    let insight = ml.learn();
    let strategies: Vec<StrategyInsight> = insight
        .strategies
        .iter()
        .filter(|s| filter.is_empty() || s.strategy == filter)
        .map(|s| StrategyInsight {
            strategy: s.strategy.clone(),
            total_uses: s.total_uses,
            successes: s.successes,
            failures: s.failures,
            avg_duration_secs: s.avg_duration_secs,
            success_rate: s.success_rate,
        })
        .collect();
    (
        StatusCode::OK,
        Json(LearningInsightsResponse {
            total_samples: insight.total_samples,
            overall_success_rate: insight.overall_success_rate,
            strategies,
        }),
    )
        .into_response()
}

pub(super) async fn list_plugins_handler(State(state): State<RestApiState>) -> impl IntoResponse {
    let plugins = state.bridge.plugin_manager.list_plugins();
    let entries: Vec<PluginDescriptor> = plugins
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
    (StatusCode::OK, Json(entries)).into_response()
}

pub(super) async fn load_plugin_handler(
    State(state): State<RestApiState>,
    Json(req): Json<LoadPluginRequest>,
) -> impl IntoResponse {
    let descriptor = sisyphus_types::PluginDescriptor {
        name: req.name,
        version: req.version,
        wasm_path: req.wasm_path.into(),
        tool_id: req.tool_id,
        fuel_limit: req.fuel_limit.unwrap_or(0),
        checksum: None,
    };
    match state
        .bridge
        .plugin_manager
        .register_plugin(descriptor)
        .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(LoadPluginResponse {
                success: true,
                error: String::new(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(LoadPluginResponse {
                success: false,
                error: e,
            }),
        )
            .into_response(),
    }
}

pub(super) async fn unload_plugin_handler(
    State(state): State<RestApiState>,
    Json(req): Json<UnloadPluginRequest>,
) -> impl IntoResponse {
    match state.bridge.plugin_manager.unload_plugin(&req.name) {
        Ok(_) => (
            StatusCode::OK,
            Json(UnloadPluginResponse {
                success: true,
                error: String::new(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(UnloadPluginResponse {
                success: false,
                error: e,
            }),
        )
            .into_response(),
    }
}

pub(super) async fn call_jcode_handler(
    State(state): State<RestApiState>,
    Json(req): Json<CallJcodeRequest>,
) -> impl IntoResponse {
    let args: Vec<u8> = match serde_json::to_vec(&serde_json::json!({
        "task_id": req.task_id,
        "goal": req.goal,
        "context": req.context_json
    })) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(CallJcodeResponse {
                    success: false,
                    result: String::new(),
                    error: format!("Invalid context JSON: {e}"),
                }),
            )
                .into_response()
        }
    };
    match state
        .bridge
        .jcode_caller
        .call_tool("jcode/call", args)
        .await
    {
        Ok(result) => {
            let result_str = String::from_utf8_lossy(&result).to_string();
            (
                StatusCode::OK,
                Json(CallJcodeResponse {
                    success: true,
                    result: result_str,
                    error: String::new(),
                }),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(CallJcodeResponse {
                success: false,
                result: String::new(),
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

pub(super) async fn chaos_start_handler(
    State(state): State<RestApiState>,
    Json(req): Json<ChaosStartRequest>,
) -> impl IntoResponse {
    // Convert string fault types to FaultType enum
    let fault_types: Vec<sisyphus_core::FaultType> = req
        .fault_types
        .iter()
        .filter_map(|s| match s.as_str() {
            "LlmTimeout" => Some(sisyphus_core::FaultType::LlmTimeout),
            "LlmGarbage" => Some(sisyphus_core::FaultType::LlmGarbage),
            "ToolCrash" => Some(sisyphus_core::FaultType::ToolCrash),
            "ToolHang" => Some(sisyphus_core::FaultType::ToolHang),
            "StorageWriteFailure" => Some(sisyphus_core::FaultType::StorageWriteFailure),
            "StorageReadFailure" => Some(sisyphus_core::FaultType::StorageReadFailure),
            "NetworkPartition" => Some(sisyphus_core::FaultType::NetworkPartition),
            _ if s.starts_with("RandomDelay(") && s.ends_with(")") => {
                let inner = s["RandomDelay(".len()..s.len() - 1].parse().ok()?;
                Some(sisyphus_core::FaultType::RandomDelay(inner))
            }
            _ => None,
        })
        .collect();

    let experiment = sisyphus_core::ChaosExperiment {
        name: req.name,
        fault_types,
        fault_probability: req.fault_probability,
        duration: std::time::Duration::from_secs(req.duration_seconds),
        target_operations: if req.target_operations.is_empty() {
            None
        } else {
            Some(req.target_operations)
        },
    };
    state.bridge.chaos_runner.start_experiment(experiment);
    // ChaosExperiment doesn't return an ID, we generate one
    let experiment_id = Uuid::new_v4().to_string();
    (
        StatusCode::OK,
        Json(ChaosStartResponse {
            success: true,
            experiment_id,
            error: String::new(),
        }),
    )
        .into_response()
}

pub(super) async fn chaos_stop_handler(
    State(state): State<RestApiState>,
    Json(_req): Json<ChaosStopRequest>,
) -> impl IntoResponse {
    let result = state.bridge.chaos_runner.stop_experiment();
    match result {
        Some(result) => {
            let name = result.name.clone();
            let operations_attempted = result.operations_attempted;
            let operations_succeeded = result.operations_succeeded;
            let operations_failed = result.operations_failed;
            let faults_injected = result.faults_injected;
            let success_rate = result.success_rate();
            let exp_result = ChaosExperimentResult {
                name,
                operations_attempted,
                operations_succeeded,
                operations_failed,
                faults_injected,
                success_rate,
            };
            (
                StatusCode::OK,
                Json(ChaosStopResponse {
                    success: true,
                    result: exp_result,
                    error: String::new(),
                }),
            )
                .into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(ChaosStopResponse {
                success: false,
                result: ChaosExperimentResult {
                    name: String::new(),
                    operations_attempted: 0,
                    operations_succeeded: 0,
                    operations_failed: 0,
                    faults_injected: 0,
                    success_rate: 0.0,
                },
                error: "No running experiment".into(),
            }),
        )
            .into_response(),
    }
}

pub(super) async fn chaos_status_handler(
    State(state): State<RestApiState>,
    _req: axum::extract::Query<ChaosStatusRequest>,
) -> impl IntoResponse {
    // Return current experiment status based on enabled flag
    let running = state.bridge.chaos_runner.is_running();
    let exp_result = if running {
        ChaosExperimentResult {
            name: "active".to_string(),
            operations_attempted: 0,
            operations_succeeded: 0,
            operations_failed: 0,
            faults_injected: 0,
            success_rate: 0.0,
        }
    } else {
        ChaosExperimentResult {
            name: String::new(),
            operations_attempted: 0,
            operations_succeeded: 0,
            operations_failed: 0,
            faults_injected: 0,
            success_rate: 0.0,
        }
    };
    (
        StatusCode::OK,
        Json(ChaosStatusResponse {
            running,
            result: exp_result,
        }),
    )
        .into_response()
}

pub(super) async fn anomaly_report_handler(
    State(_state): State<RestApiState>,
    req: axum::extract::Query<AnomalyRequest>,
) -> impl IntoResponse {
    // Note: Anomaly detection is available via sisyphus_intel::AnomalyDetector.
    // The kernel's intelligence_manager has access to it, but requires downcasting.
    // For now, return empty list; extend IntelligenceManager trait if needed.
    let _ = req; // suppress unused warning
    (StatusCode::OK, Json(Vec::<AnomalyReport>::new())).into_response()
}

pub(super) async fn delete_workspace_handler(
    State(state): State<RestApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.bridge.workspace_manager.delete_workspace(&id).await {
        Ok(_) => (
            StatusCode::OK,
            Json(DeleteWorkspaceResponse {
                success: true,
                error: String::new(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(DeleteWorkspaceResponse {
                success: false,
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}
