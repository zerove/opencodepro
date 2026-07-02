use std::sync::{Arc, RwLock, Weak};

use sisyphus_core::SisyphusKernel;
use sisyphus_mem::MfsStore;
use sisyphus_roadmap::{ProgressReport, RoadmapManager};
use sisyphus_tools_llm::LlmClient;
use sisyphus_types::{MilestoneStatus, ToolExecutor, TypesError};
use tracing::info;

pub(crate) struct StrategicExecutor {
    kernel: Arc<RwLock<Option<Weak<SisyphusKernel>>>>,
    state_store: Arc<MfsStore>,
    llm_client: Arc<dyn LlmClient>,
}

impl StrategicExecutor {
    pub(crate) fn new(
        state_store: Arc<MfsStore>,
        llm_client: Arc<dyn LlmClient>,
    ) -> Self {
        Self {
            kernel: Arc::new(RwLock::new(None)),
            state_store,
            llm_client,
        }
    }

    pub(crate) fn set_kernel(&self, kernel: Weak<SisyphusKernel>) {
        match self.kernel.write() {
            Ok(mut guard) => *guard = Some(kernel),
            Err(poisoned) => {
                let msg = format!("Kernel RwLock poisoned in set_kernel: {}", poisoned);
                tracing::warn!("{}", msg);
                *poisoned.into_inner() = Some(kernel);
            }
        }
    }
}

#[tonic::async_trait]
impl ToolExecutor for StrategicExecutor {
    async fn execute(
        &self,
        tool_id: &str,
        args: Vec<u8>,
        _fuel: u64,
    ) -> Result<Vec<u8>, TypesError> {
        let kernel_weak = self.kernel.read().map_err(|e| {
            TypesError::ToolExecutionError {
                tool_id: tool_id.to_string(),
                details: format!("Kernel RwLock poisoned: {e}"),
            }
        })?.clone();
        let kernel = kernel_weak
            .and_then(|w| w.upgrade())
            .ok_or_else(|| TypesError::ToolExecutionError {
                tool_id: tool_id.to_string(),
                details: "SisyphusKernel is not initialized or has been dropped".to_string(),
            })?;

        if tool_id == "next_goal" {
            let args_str = String::from_utf8_lossy(&args);
            let v: serde_json::Value =
                serde_json::from_str(&args_str).map_err(|e| sisyphus_types::TypesError::ToolExecutionError { tool_id: tool_id.to_string(), details: format!("Invalid JSON args: {e}") })?;

            let project_id = v["project_id"]
                .as_str()
                .ok_or_else(|| sisyphus_types::TypesError::ToolExecutionError { tool_id: tool_id.to_string(), details: "Missing 'project_id' in arguments".to_string() })?
                .to_string();

            info!("Strategic loop triggered for project: {}", project_id);

            let manager = RoadmapManager::new(self.state_store.clone());

            let goal_ref = manager
                .get_next_goal(&project_id)
                .await
                .map_err(|e| sisyphus_types::TypesError::ToolExecutionError { tool_id: tool_id.to_string(), details: format!("Failed to get next goal: {e}") })?;

            let goal = match goal_ref {
                Some(g) => g,
                None => return Ok(b"No pending goals in the roadmap.".to_vec()),
            };

            info!(
                "Strategic Loop: Executing next goal [{}] - {}",
                goal.goal_id, goal.description
            );

            let roadmap_context = manager
                .get_goal_context(&project_id, &goal.goal_id)
                .await
                .unwrap_or_else(|_| "Failed to load roadmap context.".to_string());
            let enriched_goal = format!(
                "{}\n\n--- Roadmap Context ---\n{}",
                goal.description, roadmap_context,
            );

            let synth_id = format!("strategic_{}", goal.goal_id);
            match kernel.execute_goal(&enriched_goal, &synth_id, None).await {
                Ok((result, report)) => {
                    if let Ok(Some(project)) = manager.get_project(&project_id).await {
                        let (ms_id, ph_id) = project
                            .roadmap
                            .milestones
                            .iter()
                            .flat_map(|ms| {
                                ms.phases
                                    .iter()
                                    .map(move |ph| (ms.id.as_str(), ph.id.as_str(), &ph.goals))
                            })
                            .filter(|(_, _, goals)| goals.iter().any(|g| g.goal_id == goal.goal_id))
                            .map(|(ms_id, ph_id, _)| (ms_id.to_string(), ph_id.to_string()))
                            .next()
                            .unwrap_or_default();
                        let _ = manager
                            .update_goal_status(
                                &project_id,
                                &ms_id,
                                &ph_id,
                                &goal.goal_id,
                                MilestoneStatus::Completed,
                            )
                            .await;
                    }

                    let reflection = manager
                        .generate_progress_report(&project_id)
                        .await
                        .unwrap_or_else(|_| ProgressReport {
                            project_id: project_id.clone(),
                            project_name: String::new(),
                            overall_progress: 0.0,
                            milestone_count: 0,
                            total_goals: 0,
                            completed_goals: 0,
                            milestones: vec![],
                        });

                    let output = format!(
                        "Strategic Goal [{}] Completed.\nResult: {}\nCognition: {}\n\n--- Reflection ---\nProgress: {:.0}% | On track: {}\nRecommendations:\n{}",
                        goal.goal_id,
                        result,
                        report,
                        reflection.overall_progress * 100.0,
                        "yes",
                        "  None.",
                    );
                    Ok(output.into_bytes())
                }
                Err(e) => Err(sisyphus_types::TypesError::ToolExecutionError {
                    tool_id: tool_id.to_string(),
                    details: format!("Strategic Goal [{}] execution failed: {e}", goal.goal_id),
                }),
            }
        } else if tool_id == "progress" {
            let args_str = String::from_utf8_lossy(&args);
            let v: serde_json::Value = serde_json::from_str(&args_str).map_err(|e| {
                TypesError::ToolExecutionError {
                    tool_id: tool_id.to_string(),
                    details: format!("Invalid JSON args: {e}"),
                }
            })?;

            let project_id = v["project_id"].as_str().ok_or_else(|| {
                TypesError::ToolExecutionError {
                    tool_id: tool_id.to_string(),
                    details: "Missing 'project_id' in arguments".to_string(),
                }
            })?.to_string();

            info!("Strategic loop triggered for project: {}", project_id);

            let manager = sisyphus_roadmap::RoadmapManager::new(self.state_store.clone());

            let goal_ref = manager
                .get_next_goal(&project_id)
                .await
                .map_err(|e| TypesError::ToolExecutionError {
                    tool_id: tool_id.to_string(),
                    details: format!("Failed to get next goal: {e}"),
                })?;

            let goal = match goal_ref {
                Some(g) => g,
                None => return Ok(b"No pending goals in the roadmap.".to_vec()),
            };

            info!(
                "Strategic Loop: Executing next goal [{}] - {}",
                goal.goal_id, goal.description
            );

            let roadmap_context = manager
                .get_goal_context(&project_id, &goal.goal_id)
                .await
                .unwrap_or_else(|_| "Failed to load roadmap context.".to_string());
            let enriched_goal = format!(
                "{}\n\n--- Roadmap Context ---\n{}",
                goal.description, roadmap_context,
            );

            let synth_id = format!("strategic_{}", goal.goal_id);
            match kernel.execute_goal(&enriched_goal, &synth_id, None).await {
                Ok((result, report)) => {
                    if let Ok(Some(project)) = manager.get_project(&project_id).await {
                        let (ms_id, ph_id) = project
                            .roadmap
                            .milestones
                            .iter()
                            .flat_map(|ms| {
                                ms.phases
                                    .iter()
                                    .map(move |ph| (ms.id.as_str(), ph.id.as_str(), &ph.goals))
                            })
                            .filter(|(_, _, goals)| goals.iter().any(|g| g.goal_id == goal.goal_id))
                            .map(|(ms_id, ph_id, _)| (ms_id.to_string(), ph_id.to_string()))
                            .next()
                            .unwrap_or_default();

                        // Record outcome
                        let outcome = sisyphus_roadmap::ExecutionOutcome {
                            goal_id: goal.goal_id.clone(),
                            project_id: project_id.clone(),
                            milestone_id: ms_id.clone(),
                            phase_id: ph_id.clone(),
                            success: true,
                            deviation_score: None,
                            error: None,
                            timestamp: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs(),
                            actual_cost: 0.0,
                            duration_secs: 0.0,
                        };
                        let _ = manager.record_outcome(&outcome).await;
                        let _ = manager.update_goal_status(
                            &project_id,
                            &ms_id,
                            &ph_id,
                            &goal.goal_id,
                            sisyphus_types::MilestoneStatus::Completed,
                        ).await;
                    }
                    let duration = 0.0;
                    sisyphus_metrics::record_sop_execution(&synth_id, true, duration);
                    let output = serde_json::json!({
                        "result": result,
                        "cognition": report,
                    });
                    Ok(serde_json::to_vec(&output).unwrap_or_default())
                }
                Err(e) => {
                    let duration = 0.0;
                    sisyphus_metrics::record_sop_execution(&synth_id, false, duration);
                    Err(TypesError::ToolExecutionError {
                        tool_id: tool_id.to_string(),
                        details: format!("Goal execution failed: {e}"),
                    })
                }
            }
        } else if tool_id == "show_roadmap" {
            let args_str = String::from_utf8_lossy(&args);
            let v: serde_json::Value = serde_json::from_str(&args_str).map_err(|e| {
                TypesError::ToolExecutionError {
                    tool_id: tool_id.to_string(),
                    details: format!("Invalid JSON args: {e}"),
                }
            })?;

            let project_id = v["project_id"].as_str().ok_or_else(|| {
                TypesError::ToolExecutionError {
                    tool_id: tool_id.to_string(),
                    details: "Missing 'project_id' in arguments".to_string(),
                }
            })?.to_string();

            let manager = sisyphus_roadmap::RoadmapManager::new(self.state_store.clone());
            match manager.generate_progress_report(&project_id).await {
                Ok(report) => {
                    let output = serde_json::to_string_pretty(&report)
                        .map_err(|e| TypesError::ToolExecutionError {
                            tool_id: tool_id.to_string(),
                            details: format!("Failed to serialize report: {e}"),
                        })?;
                    Ok(output.into_bytes())
                }
                Err(e) => Err(TypesError::ToolExecutionError {
                    tool_id: tool_id.to_string(),
                    details: format!("Failed to generate report: {e}"),
                }),
            }
        } else if tool_id == "replan" {
            let args_str = String::from_utf8_lossy(&args);
            let v: serde_json::Value = serde_json::from_str(&args_str).map_err(|e| {
                TypesError::ToolExecutionError {
                    tool_id: tool_id.to_string(),
                    details: format!("Invalid JSON args: {e}"),
                }
            })?;

            let project_id = v["project_id"].as_str().ok_or_else(|| {
                TypesError::ToolExecutionError {
                    tool_id: tool_id.to_string(),
                    details: "Missing 'project_id' in arguments".to_string(),
                }
            })?.to_string();

            let manager = sisyphus_roadmap::RoadmapManager::new(self.state_store.clone());
            let strategist = sisyphus_roadmap::strategic_reflection::StrategicReflector::new(
                self.llm_client.clone(),
            );
            match strategist.reflect(&manager, &project_id).await {
                Ok(reflection) => {
                    let output = serde_json::to_string_pretty(&reflection)
                        .map_err(|e| TypesError::ToolExecutionError {
                            tool_id: tool_id.to_string(),
                            details: format!("Failed to serialize: {e}"),
                        })?;
                    Ok(output.into_bytes())
                }
                Err(e) => Err(TypesError::ToolExecutionError {
                    tool_id: tool_id.to_string(),
                    details: format!("Replan reflection failed: {e}"),
                }),
            }
        } else if tool_id == "auto_plan" {
            let args_str = String::from_utf8_lossy(&args);
            let v: serde_json::Value = serde_json::from_str(&args_str).map_err(|e| {
                TypesError::ToolExecutionError {
                    tool_id: tool_id.to_string(),
                    details: format!("Invalid JSON args: {e}"),
                }
            })?;

            let project_id = v["project_id"].as_str().ok_or_else(|| {
                TypesError::ToolExecutionError {
                    tool_id: tool_id.to_string(),
                    details: "Missing 'project_id' in arguments".to_string(),
                }
            })?.to_string();
            let description = v["description"].as_str().unwrap_or("").to_string();

            let manager = sisyphus_roadmap::RoadmapManager::new(self.state_store.clone());
            let generator = sisyphus_roadmap::milestone_generator::MilestoneGenerator::new(
                self.llm_client.clone(),
            );
            match generator
                .generate_milestones(&manager, &project_id, &description)
                .await
            {
                Ok(milestones) => {
                    let output = serde_json::to_string_pretty(&milestones)
                        .map_err(|e| TypesError::ToolExecutionError {
                            tool_id: tool_id.to_string(),
                            details: format!("Failed to serialize: {e}"),
                        })?;
                    Ok(output.into_bytes())
                }
                Err(e) => Err(TypesError::ToolExecutionError {
                    tool_id: tool_id.to_string(),
                    details: format!("Auto-plan failed: {e}"),
                }),
            }
        } else if tool_id == "goal_context" {
            let args_str = String::from_utf8_lossy(&args);
            let v: serde_json::Value = serde_json::from_str(&args_str).map_err(|e| {
                TypesError::ToolExecutionError {
                    tool_id: tool_id.to_string(),
                    details: format!("Invalid JSON args: {e}"),
                }
            })?;

            let project_id = v["project_id"].as_str().ok_or_else(|| {
                TypesError::ToolExecutionError {
                    tool_id: tool_id.to_string(),
                    details: "Missing 'project_id' in arguments".to_string(),
                }
            })?.to_string();
            let goal_id = v["goal_id"].as_str().ok_or_else(|| {
                TypesError::ToolExecutionError {
                    tool_id: tool_id.to_string(),
                    details: "Missing 'goal_id' in arguments".to_string(),
                }
            })?.to_string();

            let manager = sisyphus_roadmap::RoadmapManager::new(self.state_store.clone());
            let context = manager
                .get_goal_context(&project_id, &goal_id)
                .await
                .unwrap_or_else(|_| "Failed to load roadmap context.".to_string());
            Ok(context.into_bytes())
        } else if tool_id == "reflect" {
            let args_str = String::from_utf8_lossy(&args);
            let v: serde_json::Value = serde_json::from_str(&args_str).map_err(|e| {
                TypesError::ToolExecutionError {
                    tool_id: tool_id.to_string(),
                    details: format!("Invalid JSON args: {e}"),
                }
            })?;

            let project_id = v["project_id"].as_str().ok_or_else(|| {
                TypesError::ToolExecutionError {
                    tool_id: tool_id.to_string(),
                    details: "Missing 'project_id' in arguments".to_string(),
                }
            })?.to_string();

            let manager = sisyphus_roadmap::RoadmapManager::new(self.state_store.clone());

            let strategist = sisyphus_roadmap::strategic_reflection::StrategicReflector::new(
                self.llm_client.clone(),
            );
            match strategist.reflect(&manager, &project_id).await {
                Ok(reflection) => {
                    let output = serde_json::to_string_pretty(&reflection)
                        .map_err(|e| TypesError::ToolExecutionError {
                            tool_id: tool_id.to_string(),
                            details: format!("Failed to serialize reflection: {e}"),
                        })?;
                    Ok(output.into_bytes())
                }
                Err(e) => Err(TypesError::ToolExecutionError {
                    tool_id: tool_id.to_string(),
                    details: format!("Reflection failed: {e}"),
                }),
            }
        } else if tool_id == "predict" {
            let args_str = String::from_utf8_lossy(&args);
            let v: serde_json::Value = serde_json::from_str(&args_str).map_err(|e| {
                TypesError::ToolExecutionError {
                    tool_id: tool_id.to_string(),
                    details: format!("Invalid JSON args: {e}"),
                }
            })?;

            let project_id = v["project_id"].as_str().ok_or_else(|| {
                TypesError::ToolExecutionError {
                    tool_id: tool_id.to_string(),
                    details: "Missing 'project_id' in arguments".to_string(),
                }
            })?.to_string();

            let manager = sisyphus_roadmap::RoadmapManager::new(self.state_store.clone());
            let outcomes = manager.get_outcomes(&project_id).await.unwrap_or_default();

            let project = manager.get_project(&project_id).await.ok().flatten();
            let total_goals = project
                .map(|p| {
                    p.roadmap
                        .milestones
                        .iter()
                        .flat_map(|ms| ms.phases.iter().flat_map(|ph| &ph.goals))
                        .count()
                })
                .unwrap_or(0);
            let completed_goals = outcomes
                .iter()
                .filter(|o| o.success)
                .count();

            let optimizer = sisyphus_roadmap::resource_optimizer::ResourceOptimizer::new();
            let projection = optimizer.project_costs(&outcomes, total_goals, completed_goals);

            let output = serde_json::to_string_pretty(&projection)
                .map_err(|e| TypesError::ToolExecutionError {
                    tool_id: tool_id.to_string(),
                    details: format!("Failed to serialize projection: {e}"),
                })?;
            Ok(output.into_bytes())
        } else if tool_id == "project_costs" {
            let args_str = String::from_utf8_lossy(&args);
            let v: serde_json::Value = serde_json::from_str(&args_str).map_err(|e| {
                TypesError::ToolExecutionError {
                    tool_id: tool_id.to_string(),
                    details: format!("Invalid JSON args: {e}"),
                }
            })?;

            let project_id = v["project_id"].as_str().ok_or_else(|| {
                TypesError::ToolExecutionError {
                    tool_id: tool_id.to_string(),
                    details: "Missing 'project_id' in arguments".to_string(),
                }
            })?.to_string();

            let manager = sisyphus_roadmap::RoadmapManager::new(self.state_store.clone());
            let outcomes = manager.get_outcomes(&project_id).await.unwrap_or_default();

            let project = manager.get_project(&project_id).await.ok().flatten();
            let total_goals = project
                .map(|p| {
                    p.roadmap
                        .milestones
                        .iter()
                        .flat_map(|ms| ms.phases.iter().flat_map(|ph| &ph.goals))
                        .count()
                })
                .unwrap_or(0);
            let completed_goals = outcomes
                .iter()
                .filter(|o| o.success)
                .count();

            let optimizer = sisyphus_roadmap::resource_optimizer::ResourceOptimizer::new();
            let projection = optimizer.project_costs(&outcomes, total_goals, completed_goals);

            let output = serde_json::to_string_pretty(&projection)
                .map_err(|e| TypesError::ToolExecutionError {
                    tool_id: tool_id.to_string(),
                    details: format!("Failed to serialize projection: {e}"),
                })?;
            Ok(output.into_bytes())
        } else if tool_id == "bottlenecks" {
            let args_str = String::from_utf8_lossy(&args);
            let v: serde_json::Value = serde_json::from_str(&args_str).map_err(|e| {
                TypesError::ToolExecutionError {
                    tool_id: tool_id.to_string(),
                    details: format!("Invalid JSON args: {e}"),
                }
            })?;

            let project_id = v["project_id"].as_str().ok_or_else(|| {
                TypesError::ToolExecutionError {
                    tool_id: tool_id.to_string(),
                    details: "Missing 'project_id' in arguments".to_string(),
                }
            })?.to_string();

            let manager = sisyphus_roadmap::RoadmapManager::new(self.state_store.clone());
            let optimizer = sisyphus_roadmap::resource_optimizer::ResourceOptimizer::new();
            let bottlenecks = optimizer
                .find_bottlenecks(&manager.get_outcomes(&project_id).await.unwrap_or_default());

            let output = serde_json::to_string_pretty(&bottlenecks)
                .map_err(|e| TypesError::ToolExecutionError {
                    tool_id: tool_id.to_string(),
                    details: format!("Failed to serialize bottlenecks: {e}"),
                })?;
            Ok(output.into_bytes())
        } else {
            return Err(TypesError::ToolExecutionError {
                tool_id: tool_id.to_string(),
                details: format!("Unknown strategic tool: {tool_id}"),
            });
        }
    }
}
