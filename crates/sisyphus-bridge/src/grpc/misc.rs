//! gRPC 处理器（三）：辅助域（健康检查、反馈、经验、基准、熔断、混沌工程）。

#![allow(clippy::result_large_err)]

use crate::MySisyphusBridge;
use sisyphus_bridge_api::{
    BenchmarkEntry,
    BreakerMetricsRequest,
    BreakerMetricsResponse,
    BreakerStatus,
    CaptureExperienceRequest,
    CaptureExperienceResponse,
    HealthCheckRequest,
    HealthCheckResponse,
    ListBenchmarksRequest,
    ListBenchmarksResponse,
    // Experience sharing
    ListSharedExperiencesRequest,
    ListSharedExperiencesResponse,
    PullExperienceRequest,
    PullExperienceResponse,
    ShareExperienceRequest,
    ShareExperienceResponse,
    SubmitFeedbackRequest,
    SubmitFeedbackResponse,
};
use sisyphus_types::experience::ExperienceBank;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tonic::{Request, Response, Status};

impl MySisyphusBridge {
    pub(crate) async fn health_check_impl(
        &self,
        _request: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        Ok(Response::new(HealthCheckResponse {
            status: "SERVING".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }))
    }

    pub(crate) async fn submit_feedback_impl(
        &self,
        request: Request<SubmitFeedbackRequest>,
    ) -> Result<Response<SubmitFeedbackResponse>, Status> {
        let project_id = request
            .metadata()
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
            },
            Err(e) => Ok(Response::new(SubmitFeedbackResponse {
                success: false,
                error: e.to_string(),
                evolution_json: "".into(),
            })),
        }
    }

    pub(crate) async fn capture_experience_impl(
        &self,
        request: Request<CaptureExperienceRequest>,
    ) -> Result<Response<CaptureExperienceResponse>, Status> {
        let project_id = request
            .metadata()
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

    pub(crate) async fn list_benchmarks_impl(
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

    pub(crate) async fn get_breaker_metrics_impl(
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

    pub(crate) async fn get_learning_insights_impl(
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
        Ok(Response::new(
            sisyphus_bridge_api::LearningInsightsResponse {
                total_samples: insight.total_samples,
                overall_success_rate: insight.overall_success_rate,
                strategies,
            },
        ))
    }

    pub(crate) async fn start_chaos_impl(
        &self,
        request: Request<sisyphus_bridge_api::ChaosStartRequest>,
    ) -> Result<Response<sisyphus_bridge_api::ChaosStartResponse>, Status> {
        let req = request.into_inner();
        let fault_types: Vec<sisyphus_core::FaultType> = req
            .fault_types
            .iter()
            .map(|ft| match ft.as_str() {
                "llm_timeout" => sisyphus_core::FaultType::LlmTimeout,
                "llm_garbage" => sisyphus_core::FaultType::LlmGarbage,
                "tool_crash" => sisyphus_core::FaultType::ToolCrash,
                "tool_hang" => sisyphus_core::FaultType::ToolHang,
                "storage_write" => sisyphus_core::FaultType::StorageWriteFailure,
                "storage_read" => sisyphus_core::FaultType::StorageReadFailure,
                "network_partition" => sisyphus_core::FaultType::NetworkPartition,
                _ => sisyphus_core::FaultType::ToolCrash,
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

        self.chaos_runner.start_experiment(experiment);
        let experiment_id = format!(
            "chaos_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );

        tracing::info!(experiment_id, "Chaos experiment started");
        Ok(Response::new(sisyphus_bridge_api::ChaosStartResponse {
            success: true,
            experiment_id,
            error: String::new(),
        }))
    }

    pub(crate) async fn stop_chaos_impl(
        &self,
        _request: Request<sisyphus_bridge_api::ChaosStopRequest>,
    ) -> Result<Response<sisyphus_bridge_api::ChaosStopResponse>, Status> {
        let result = self.chaos_runner.stop_experiment();
        let proto_result = result.map(|r| {
            let rate = r.success_rate();
            sisyphus_bridge_api::ChaosExperimentResult {
                name: r.name,
                operations_attempted: r.operations_attempted,
                operations_succeeded: r.operations_succeeded,
                operations_failed: r.operations_failed,
                faults_injected: r.faults_injected,
                success_rate: rate,
            }
        });

        tracing::info!(
            attempted = proto_result.as_ref().map(|r| r.operations_attempted).unwrap_or(0),
            faults = proto_result.as_ref().map(|r| r.faults_injected).unwrap_or(0),
            "Chaos experiment stopped"
        );

        Ok(Response::new(sisyphus_bridge_api::ChaosStopResponse {
            success: true,
            result: proto_result,
            error: String::new(),
        }))
    }

    pub(crate) async fn get_chaos_status_impl(
        &self,
        _request: Request<sisyphus_bridge_api::ChaosStatusRequest>,
    ) -> Result<Response<sisyphus_bridge_api::ChaosStatusResponse>, Status> {
        // Check via the experiment lock: if an experiment is active it's running.
        // We can't easily introspect the AtomicBool from here, so we try the lock.
        let running = self.chaos_runner.is_running();
        let result = None; // status response doesn't include full result

        Ok(Response::new(sisyphus_bridge_api::ChaosStatusResponse {
            running,
            result,
        }))
    }

    pub(crate) async fn stream_anomaly_report_impl(
        &self,
        request: Request<sisyphus_bridge_api::AnomalyRequest>,
    ) -> Result<Response<tonic::codegen::BoxStream<sisyphus_bridge_api::AnomalyReport>>, Status>
    {
        let _req = request.into_inner();
        let (tx, rx) = mpsc::unbounded_channel::<sisyphus_bridge_api::AnomalyReport>();

        // Access the AnomalyLog from the kernel's strategic manager.
        // 异常观测的可观测性来源是战略管理器上的旁路日志（AnomalyLog），
        // 检测器本身只做 per-run 控制决策。
        let anomaly_log = self.kernel.strategic_manager.anomaly_log.clone();
        let event_bus = self.event_bus.clone();

        // Spawn a background task that periodically pushes new anomalies.
        // 只推送新检测到的异常：日志里的 `seq` 单调递增，用它做游标
        // 就不会每 5 秒把同一批异常重发一遍。
        tokio::spawn(async move {
            let mut last_seq = 0u64;
            loop {
                let observed = anomaly_log.recent(None);
                // 本轮推送的新异常（seq ≥ 游标）；数量同时驱动状态事件。
                let new_entries: Vec<_> =
                    observed.into_iter().filter(|entry| entry.seq >= last_seq).collect();
                for entry in &new_entries {
                    last_seq = entry.seq.saturating_add(1);
                    let severity_str = match entry.anomaly.severity {
                        sisyphus_core::AnomalySeverity::Low => "low",
                        sisyphus_core::AnomalySeverity::Medium => "medium",
                        sisyphus_core::AnomalySeverity::High => "high",
                        sisyphus_core::AnomalySeverity::Critical => "critical",
                    };
                    let proto_anomaly = sisyphus_bridge_api::AnomalyReport {
                        node_name: entry.anomaly.node_name.clone(),
                        severity: severity_str.into(),
                        metric: entry.anomaly.metric.clone(),
                        observed: entry.anomaly.observed,
                        expected: entry.anomaly.expected,
                        description: entry.anomaly.description.clone(),
                        timestamp: entry.detected_at,
                    };
                    if tx.send(proto_anomaly).is_err() {
                        return; // client disconnected
                    }
                }

                // Also bridge-publish a compact status event. 必须读 AnomalyLog：
                // per-run 检测器的 `generate_report()` 永远是空报告
                //（共享检测器不再被喂），用它会导致仪表盘永远显示 0。
                let new_count = new_entries.len();
                let latest_severity = new_entries.last().map(|entry| entry.anomaly.severity);
                let _ = event_bus.send(sisyphus_bridge_api::BridgeEvent {
                    event: Some(sisyphus_bridge_api::bridge_event::Event::OpUpdate(
                        sisyphus_bridge_api::OperationUpdate {
                            op_id: "anomaly_detector".into(),
                            status: if latest_severity.is_some() {
                                "ANOMALY"
                            } else {
                                "NORMAL"
                            }
                            .into(),
                            result: format!("{new_count} anomalies"),
                            success: latest_severity.is_none(),
                        },
                    )),
                });

                // Poll interval
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        });

        let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx).map(Ok);
        Ok(Response::new(Box::pin(stream)))
    }

    pub(crate) async fn stream_events_impl(
        &self,
        _request: Request<sisyphus_bridge_api::EventRequest>,
    ) -> Result<Response<tonic::codegen::BoxStream<sisyphus_bridge_api::BridgeEvent>>, Status> {
        let rx = self.event_bus.subscribe();
        let stream = tokio_stream::wrappers::BroadcastStream::new(rx)
            .map(|res| res.map_err(|e| Status::internal(format!("Broadcast error: {e}"))));
        Ok(Response::new(Box::pin(stream)))
    }

    // ── Experience Sharing (Federated Learning) ──────────────────────────

    pub(crate) async fn share_experience_impl(
        &self,
        request: Request<ShareExperienceRequest>,
    ) -> Result<Response<ShareExperienceResponse>, Status> {
        let req = request.into_inner();
        let exp_key = req.exp_key;

        if req.make_global {
            self.experience_bank
                .share_experience(&exp_key)
                .await
                .map_err(|e| Status::internal(format!("share experience failed: {e}")))?;
        } else if !req.target_project_id.is_empty() {
            self.experience_bank
                .unshare_experience(&exp_key, &req.target_project_id)
                .await
                .map_err(|e| Status::internal(format!("unshare experience failed: {e}")))?;
        } else {
            return Ok(Response::new(ShareExperienceResponse {
                success: false,
                error: "make_global=false requires target_project_id".into(),
            }));
        }

        Ok(Response::new(ShareExperienceResponse {
            success: true,
            error: String::new(),
        }))
    }

    pub(crate) async fn pull_experience_impl(
        &self,
        request: Request<PullExperienceRequest>,
    ) -> Result<Response<PullExperienceResponse>, Status> {
        let req = request.into_inner();

        let query = sisyphus_types::experience::ExperienceQuery {
            query_text: req.query_text,
            context_hash: req.context_hash,
            project_id: if req.project_id.is_empty() {
                None
            } else {
                Some(req.project_id)
            },
            action_id: if req.action_id.is_empty() {
                None
            } else {
                Some(req.action_id)
            },
            visual_embedding: None,
            limit: req.limit.max(1) as usize,
        };

        let experiences = self
            .experience_bank
            .query_similar(query)
            .await
            .map_err(|e| Status::internal(format!("query similar failed: {e}")))?;

        let proto_experiences: Vec<sisyphus_bridge_api::ExperienceTupleProto> = experiences
            .into_iter()
            .map(|exp| sisyphus_bridge_api::ExperienceTupleProto {
                context_text: exp.context_text,
                context_hash: exp.context_hash,
                action_id: exp.action_id,
                project_id: exp.project_id.unwrap_or_default(),
                outcome_success: exp.outcome_success,
                deviation_score: exp.deviation_score,
                cost: exp.cost,
                timestamp: exp.timestamp,
                embedding: exp.embedding.unwrap_or_default(),
                visual_embedding: exp.visual_embedding.unwrap_or_default(),
                insight: exp.insight.unwrap_or_default(),
            })
            .collect();

        Ok(Response::new(PullExperienceResponse {
            success: true,
            error: String::new(),
            experiences: proto_experiences,
        }))
    }

    pub(crate) async fn list_shared_experiences_impl(
        &self,
        _request: Request<ListSharedExperiencesRequest>,
    ) -> Result<Response<ListSharedExperiencesResponse>, Status> {
        let experiences = self
            .experience_bank
            .list_shared()
            .await
            .map_err(|e| Status::internal(format!("list shared failed: {e}")))?;

        let proto_experiences: Vec<sisyphus_bridge_api::ExperienceTupleProto> = experiences
            .into_iter()
            .map(|exp| sisyphus_bridge_api::ExperienceTupleProto {
                context_text: exp.context_text,
                context_hash: exp.context_hash,
                action_id: exp.action_id,
                project_id: exp.project_id.unwrap_or_default(),
                outcome_success: exp.outcome_success,
                deviation_score: exp.deviation_score,
                cost: exp.cost,
                timestamp: exp.timestamp,
                embedding: exp.embedding.unwrap_or_default(),
                visual_embedding: exp.visual_embedding.unwrap_or_default(),
                insight: exp.insight.unwrap_or_default(),
            })
            .collect();

        Ok(Response::new(ListSharedExperiencesResponse {
            success: true,
            error: String::new(),
            experiences: proto_experiences,
        }))
    }
}
