use crate::types::*;
use crate::RoadmapError;
use crate::RoadmapManager;
use sisyphus_types::MilestoneStatus;
use std::collections::HashMap;

impl RoadmapManager {
    /// 查找下一个待处理目标，尊重阶段级依赖。
    ///
    /// 按顺序扫描里程碑，跳过已完成的。在每个里程碑内，
    /// 依赖未满足的阶段会被跳过。
    /// 返回找到的第一个待处理目标，全部完成时返回 `None`。
    pub async fn get_next_goal(
        &self,
        project_id: &str,
    ) -> Result<Option<sisyphus_types::GoalRef>, RoadmapError> {
        let project = self.get_project_or_err(project_id).await?;

        for milestone in &project.roadmap.milestones {
            if milestone.status == MilestoneStatus::Completed {
                continue;
            }

            for phase in &milestone.phases {
                let dependencies_met = phase.depends_on.iter().all(|dep_id| {
                    project.roadmap.milestones.iter().flat_map(|m| &m.phases).any(|p| {
                        p.id == *dep_id
                            && p.goals.iter().all(|g| g.status == MilestoneStatus::Completed)
                    })
                });

                if !dependencies_met {
                    continue;
                }

                for goal in &phase.goals {
                    if goal.status == MilestoneStatus::Pending {
                        return Ok(Some(goal.clone()));
                    }
                }
            }
        }
        Ok(None)
    }

    /// 检索特定目标在项目中所处位置的格式化上下文。
    ///
    /// 返回描述项目进度、里程碑/阶段位置及周围目标的人类可读
    /// 字符串，其中目标用 `>>>` 标记突出显示。
    /// 目标用 `>>>` 标记突出显示。
    pub async fn get_goal_context(
        &self,
        project_id: &str,
        goal_id: &str,
    ) -> Result<String, RoadmapError> {
        let project = self.get_project_or_err(project_id).await?;

        let total_goals: usize =
            project.roadmap.milestones.iter().flat_map(|m| &m.phases).map(|p| p.goals.len()).sum();
        let completed_goals: usize = project
            .roadmap
            .milestones
            .iter()
            .flat_map(|m| &m.phases)
            .flat_map(|p| &p.goals)
            .filter(|g| g.status == MilestoneStatus::Completed)
            .count();

        let mut context = format!(
            "Project: {} ({})\nOverall Progress: {}/{} goals completed ({:.0}%)\n\n",
            project.name,
            project.description,
            completed_goals,
            total_goals,
            if total_goals > 0 {
                completed_goals as f64 / total_goals as f64 * 100.0
            } else {
                0.0
            },
        );

        for milestone in &project.roadmap.milestones {
            let ms_progress = milestone.calculate_progress();
            context.push_str(&format!(
                "Milestone: {} [{:.0}%]\n",
                milestone.name,
                ms_progress * 100.0
            ));
            for phase in &milestone.phases {
                context.push_str(&format!("  Phase: {}\n", phase.name));
                for goal in &phase.goals {
                    let marker = if goal.goal_id == goal_id {
                        ">>> "
                    } else {
                        match goal.status {
                            MilestoneStatus::Completed => "  \u{2713} ",
                            MilestoneStatus::InProgress => "  \u{25B6} ",
                            _ => "    ",
                        }
                    };
                    context.push_str(&format!(
                        "{}{} ({:?})\n",
                        marker, goal.description, goal.status
                    ));
                }
            }
        }
        Ok(context)
    }

    /// 基于当前进度预测里程碑完成情况。
    ///
    /// 使用目标完成比例与里程碑状态估算风险
    /// 与顺序。返回带逐里程碑明细与预计完成顺序的
    /// `ProgressPrediction`。
    pub async fn predict_completion(
        &self,
        project_id: &str,
    ) -> Result<ProgressPrediction, RoadmapError> {
        let project = self.get_project_or_err(project_id).await?;

        let mut total_goals = 0usize;
        let mut completed_goals = 0usize;
        let mut milestone_preds = Vec::new();
        let mut risk_levels = Vec::new();

        for milestone in &project.roadmap.milestones {
            let mut ms_total = 0usize;
            let mut ms_completed = 0usize;

            for phase in &milestone.phases {
                for goal in &phase.goals {
                    ms_total += 1;
                    total_goals += 1;
                    if goal.status == MilestoneStatus::Completed {
                        ms_completed += 1;
                        completed_goals += 1;
                    }
                }
            }

            let progress = if ms_total > 0 {
                ms_completed as f64 / ms_total as f64
            } else {
                1.0
            };
            let remaining = ms_total.saturating_sub(ms_completed);

            let risk = match milestone.status {
                MilestoneStatus::Completed => "done",
                MilestoneStatus::InProgress if progress > 0.5 => "low",
                MilestoneStatus::InProgress => "medium",
                _ => "high",
            };
            risk_levels.push(risk);

            milestone_preds.push(MilestonePrediction {
                name: milestone.name.clone(),
                progress_pct: progress * 100.0,
                remaining_goals: remaining,
                risk_level: risk.to_string(),
            });
        }

        let overall_pct = if total_goals > 0 {
            completed_goals as f64 / total_goals as f64 * 100.0
        } else {
            0.0
        };
        let remaining_goals = total_goals.saturating_sub(completed_goals);

        // 进度最高者优先
        let mut sorted: Vec<&MilestonePrediction> =
            milestone_preds.iter().filter(|m| m.risk_level != "done").collect();
        sorted.sort_by(|a, b| {
            b.progress_pct.partial_cmp(&a.progress_pct).unwrap_or(std::cmp::Ordering::Equal)
        });
        let completion_order: Vec<String> = sorted.iter().map(|m| m.name.clone()).collect();

        let overall_risk = if risk_levels.contains(&"high") {
            "high"
        } else if risk_levels.contains(&"medium") {
            "medium"
        } else {
            "low"
        }
        .to_string();

        Ok(ProgressPrediction {
            project_id: project.id,
            project_name: project.name,
            overall_progress_pct: overall_pct,
            remaining_goals,
            milestone_predictions: milestone_preds,
            estimated_completion_order: completion_order,
            risk_level: overall_risk,
        })
    }

    /// 基于历史速度预测完成时间，含 95% 置信区间。
    ///
    /// 使用 ExecutionOutcome 历史计算每个里程碑的速度（目标/秒），
    /// 并预测剩余工作。返回带逐里程碑与项目级置信区间的
    /// CompletionTimePrediction。
    pub async fn predict_completion_time(
        &self,
        project_id: &str,
    ) -> Result<CompletionTimePrediction, RoadmapError> {
        let project = self.get_project_or_err(project_id).await?;
        let outcomes = self.get_outcomes(project_id).await?;

        // 从历史结果构建速度映射
        // 键：milestone_id -> (total_duration_secs, completed_goals_count)
        let mut velocity_map: HashMap<String, (f64, usize)> = HashMap::new();
        for outcome in &outcomes {
            if outcome.success && outcome.duration_secs > 0.0 {
                let entry = velocity_map.entry(outcome.milestone_id.clone()).or_insert((0.0, 0));
                entry.0 += outcome.duration_secs;
                entry.1 += 1;
            }
        }

        // 计算每个里程碑的速度（目标/秒）
        let mut milestone_velocities: HashMap<String, f64> = HashMap::new();
        for (ms_id, (total_secs, count)) in &velocity_map {
            if *count > 0 {
                milestone_velocities.insert(ms_id.clone(), *count as f64 / total_secs);
            }
        }

        // 所有里程碑的总体速度
        let total_secs: f64 = velocity_map.values().map(|(s, _)| *s).sum();
        let total_count: usize = velocity_map.values().map(|(_, c)| *c).sum();
        let overall_velocity = if total_secs > 0.0 && total_count > 0 {
            total_count as f64 / total_secs
        } else {
            0.0
        };

        let mut total_goals = 0usize;
        let mut completed_goals = 0usize;
        let mut milestone_time_preds = Vec::new();
        let mut risk_levels = Vec::new();

        for milestone in &project.roadmap.milestones {
            let mut ms_total = 0usize;
            let mut ms_completed = 0usize;

            for phase in &milestone.phases {
                for goal in &phase.goals {
                    ms_total += 1;
                    total_goals += 1;
                    if goal.status == MilestoneStatus::Completed {
                        ms_completed += 1;
                        completed_goals += 1;
                    }
                }
            }

            let progress = if ms_total > 0 {
                ms_completed as f64 / ms_total as f64
            } else {
                1.0
            };
            let remaining = ms_total.saturating_sub(ms_completed);

            let risk = match milestone.status {
                MilestoneStatus::Completed => "done",
                MilestoneStatus::InProgress if progress > 0.5 => "low",
                MilestoneStatus::InProgress => "medium",
                _ => "high",
            };
            risk_levels.push(risk);

            // 获取该里程碑的速度
            let velocity =
                milestone_velocities.get(&milestone.id).copied().unwrap_or(overall_velocity);

            // 估算剩余时间
            let (estimated_remaining, ci_lower, ci_upper) = if velocity > 0.0 && remaining > 0 {
                let mean = remaining as f64 / velocity;
                // 95% 置信区间使用简单近似：mean ± 1.96 * (mean / sqrt(n))
                // 其中 n 是该里程碑的历史样本数
                let historical_count =
                    velocity_map.get(&milestone.id).map(|(_, c)| *c).unwrap_or(0);
                let std_err = if historical_count > 1 {
                    mean / (historical_count as f64).sqrt()
                } else {
                    mean * 0.5 // high uncertainty with little data
                };
                let margin = 1.96 * std_err;
                (mean, (mean - margin).max(0.0), mean + margin)
            } else {
                // 无速度数据 — 使用基于总体速度的回退估算
                if overall_velocity > 0.0 && remaining > 0 {
                    let mean = remaining as f64 / overall_velocity;
                    let margin = mean * 0.5; // high uncertainty
                    (mean, (mean - margin).max(0.0), mean + margin)
                } else {
                    (0.0, 0.0, 0.0)
                }
            };

            milestone_time_preds.push(MilestoneTimePrediction {
                milestone_id: milestone.id.clone(),
                name: milestone.name.clone(),
                estimated_remaining_secs: estimated_remaining,
                ci_lower_secs: ci_lower,
                ci_upper_secs: ci_upper,
                progress_pct: progress * 100.0,
                remaining_goals: remaining,
                risk_level: risk.to_string(),
                velocity_goals_per_sec: velocity,
            });
        }

        let overall_pct = if total_goals > 0 {
            completed_goals as f64 / total_goals as f64 * 100.0
        } else {
            0.0
        };
        let remaining_goals = total_goals.saturating_sub(completed_goals);

        // 进度最高者优先
        let mut sorted: Vec<&MilestoneTimePrediction> =
            milestone_time_preds.iter().filter(|m| m.risk_level != "done").collect();
        sorted.sort_by(|a, b| {
            b.progress_pct.partial_cmp(&a.progress_pct).unwrap_or(std::cmp::Ordering::Equal)
        });
        let completion_order: Vec<String> = sorted.iter().map(|m| m.name.clone()).collect();

        // 汇总剩余总时间与置信区间
        let estimated_total: f64 =
            milestone_time_preds.iter().map(|m| m.estimated_remaining_secs).sum();
        let ci_lower_total: f64 = milestone_time_preds.iter().map(|m| m.ci_lower_secs).sum();
        let ci_upper_total: f64 = milestone_time_preds.iter().map(|m| m.ci_upper_secs).sum();

        let overall_risk = if risk_levels.contains(&"high") {
            "high"
        } else if risk_levels.contains(&"medium") {
            "medium"
        } else {
            "low"
        }
        .to_string();

        let has_sufficient_data = total_count >= 5; // Need at least 5 historical samples

        Ok(CompletionTimePrediction {
            project_id: project.id,
            project_name: project.name,
            overall_progress_pct: overall_pct,
            remaining_goals,
            estimated_total_remaining_secs: estimated_total,
            ci_lower_total_secs: ci_lower_total,
            ci_upper_total_secs: ci_upper_total,
            milestone_predictions: milestone_time_preds,
            estimated_completion_order: completion_order,
            risk_level: overall_risk,
            overall_velocity_goals_per_sec: overall_velocity,
            has_sufficient_data,
        })
    }

    /// 基于预测与预期完成时间检查延迟告警。
    ///
    /// 对预测完成时间超过阈值（默认：按当前速度估算时间的 2 倍）
    /// 的里程碑返回 DelayAlert 列表。
    pub async fn check_delay_alerts(
        &self,
        project_id: &str,
        threshold_multiplier: Option<f64>,
    ) -> Result<Vec<DelayAlert>, RoadmapError> {
        let prediction = self.predict_completion_time(project_id).await?;
        let threshold = threshold_multiplier.unwrap_or(2.0);
        let mut alerts = Vec::new();

        for ms_pred in &prediction.milestone_predictions {
            if ms_pred.risk_level == "done" {
                continue;
            }

            // 上置信区间界超过 阈值 * 均值 时告警
            if ms_pred.ci_upper_secs > ms_pred.estimated_remaining_secs * threshold {
                alerts.push(DelayAlert {
                    project_id: project_id.to_string(),
                    milestone_id: ms_pred.milestone_id.clone(),
                    milestone_name: ms_pred.name.clone(),
                    estimated_remaining_secs: ms_pred.estimated_remaining_secs,
                    ci_upper_secs: ms_pred.ci_upper_secs,
                    threshold_multiplier: threshold,
                    severity: if ms_pred.ci_upper_secs > ms_pred.estimated_remaining_secs * threshold * 1.5 {
                        "critical"
                    } else {
                        "warning"
                    }.to_string(),
                    message: format!(
                        "Milestone '{}' may be delayed: upper CI bound ({:.0}s) exceeds {:.1}x estimated ({:.0}s)",
                        ms_pred.name, ms_pred.ci_upper_secs, threshold, ms_pred.estimated_remaining_secs
                    ),
                });
            }
        }

        Ok(alerts)
    }
}
