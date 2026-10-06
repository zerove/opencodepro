use crate::reflect::Reflector;
use crate::simulation::SimulationResult;
use async_trait::async_trait;
use sisyphus_types::SOPGraph;
use sisyphus_types::{ExecutionContext, NodeOutput, RuntimeSupervisor, SupervisorDecision};
use std::sync::Arc;

/// 偏差判定的**绝对**阈值下限。
///
/// 成功概率缩放不得让阈值塌缩到「任何非零偏差即转向」的程度：
/// `success_probability` 取自 MCTS `max_score`（simulation.rs:778），
/// 在迭代预算耗尽前普遍偏低，纯乘性缩放会把 High 档 0.01 压到 0.001 量级。
/// 取 0.01（High 档档位）作为地板，使低概率路径**更严**（可比 base 更低）
/// 但不会退化为恒真。
pub const MIN_EFFECTIVE_THRESHOLD: f64 = 0.01;

/// 按仿真成功概率缩放偏差阈值。
///
/// 方向性：概率越低 → 监督者越敏感 → 阈值越低。
/// 上下界：结果夹在 `[min(base, MIN_EFFECTIVE_THRESHOLD), base]`，
/// 既不塌缩到不可用，也不会因概率 > 1 而放宽超过用户设定。
pub fn scaled_deviation_threshold(base_threshold: f64, success_probability: f64) -> f64 {
    let floor = MIN_EFFECTIVE_THRESHOLD.min(base_threshold);
    (base_threshold * success_probability).max(floor).min(base_threshold)
}

/// `RuntimeSupervisor` 的具体实现，使用 `Reflector`
/// 检测与预测执行路径的运行时偏差。
pub struct SisyphusSupervisor {
    reflector: Arc<dyn Reflector>,
    prediction: SimulationResult,
    _graph: Arc<SOPGraph>,
    threshold: f64,
    experience_bank: Arc<dyn sisyphus_types::experience::ExperienceBank>,
    goal_text: String,
}

impl SisyphusSupervisor {
    /// New.
    pub fn new(
        reflector: Arc<dyn Reflector>,
        prediction: SimulationResult,
        graph: Arc<SOPGraph>,
        threshold: f64,
        experience_bank: Arc<dyn sisyphus_types::experience::ExperienceBank>,
        goal_text: String,
    ) -> Self {
        Self {
            reflector,
            prediction,
            _graph: graph,
            threshold,
            experience_bank,
            goal_text,
        }
    }

    /// 从当前执行上下文构造轨迹的辅助函数。
    fn build_trace(&self, ctx: &ExecutionContext) -> String {
        let trace_points = ctx.trace.read().map(|t| t.clone()).unwrap_or_default();
        let mut lines = Vec::new();

        for point in trace_points {
            // P0-3 延伸：显式状态字段优先，仅对缺失字段的旧数据
            // 回退到文本嗅探。
            let status = point.status.clone().unwrap_or_else(|| {
                if point.output_summary.contains("Error")
                    || point.output_summary.contains("ResourceExhausted")
                {
                    "error".to_string()
                } else {
                    "ok".to_string()
                }
            });

            lines.push(format!(
                r#"{{"node": "{}", "status": "{}", "output": "{}"}}"#,
                point.node_name, status, point.output_summary
            ));
        }
        lines.join("\n")
    }
}

#[async_trait]
impl RuntimeSupervisor for SisyphusSupervisor {
    async fn evaluate(
        &self,
        node_name: &str,
        output: &NodeOutput,
        ctx: Arc<ExecutionContext>,
    ) -> Result<SupervisorDecision, sisyphus_types::TypesError> {
        tracing::debug!(node = %node_name, "Runtime supervisor evaluating node output");

        // 1. 快速启发式反思（第 1 层）
        // P0-3：显式状态字段优先（status/success/exit_code），
        // 无显式状态时才回退到文本嗅探。
        let output_str = output.0.as_str().unwrap_or("");
        let is_obvious_error = output.indicates_failure()
            || output_str.contains("Exception")
            || output_str.contains("ResourceExhausted");

        if !is_obvious_error && !output_str.is_empty() && self.prediction.success_probability >= 0.5
        {
            tracing::info!(node = %node_name, "Fast-track: node output appears healthy, skipping LLM reflection");
            return Ok(SupervisorDecision::Continue { deviation: 0.05 });
        }

        // 2. 构建执行过程中的当前轨迹
        let trace = self.build_trace(&ctx);

        // 检索当前目标与节点的语义经验
        let exp_query = sisyphus_types::experience::ExperienceQuery {
            query_text: self.goal_text.clone(),
            context_hash: format!("{:x}", md5::compute(&self.goal_text)),
            project_id: None,
            action_id: Some(node_name.to_string()),
            visual_embedding: None,
            limit: 5,
        };
        let experiences = self.experience_bank.query_similar(exp_query).await.ok();

        // 2. 对比当前状态与预测进行反思
        let reflection = self
            .reflector
            .reflect(&trace, &self.prediction, experiences.as_deref())
            .await
            .map_err(|e| format!("Reflection error during runtime supervision: {}", e))?;

        tracing::info!(
            deviation = reflection.deviation_score,
            threshold = self.threshold,
            "Runtime deviation check"
        );

        // 3. 决定是否转向
        // 基于仿真成功概率调整阈值：
        // 较低的成功概率（较高风险）使监督者对偏差更敏感。
        // 缩放必须以**绝对阈值下限**收口，不能只用乘数下限：
        // 旧公式 `threshold * prob.max(0.1)` 在 High 档（base 0.01）叠加 MCTS
        // 早期低分（prob 0.1）后得到 0.001，偏差判定退化为「恒不等于零即转向」。
        //
        // 不用 `success_probability >= 0.5` 之类的门禁跳过低概率路径：
        // 上方快速通道**本就**要求 prob >= 0.5，低概率路径走完整反思是设计意图。
        // 正确做法是保留敏感性方向（prob 越低越严），但夹住下界。
        let effective_threshold =
            scaled_deviation_threshold(self.threshold, self.prediction.success_probability);

        if reflection.deviation_score > effective_threshold {
            tracing::warn!(
                deviation = reflection.deviation_score,
                threshold = effective_threshold,
                node = %node_name,
                "Runtime deviation threshold exceeded! Triggering Strategic Pivot."
            );

            return Ok(SupervisorDecision::Pivot {
                reason: format!(
                    "Node {} produced output with deviation score {:.2} (effective threshold {:.2}, base {:.2})",
                    node_name, reflection.deviation_score, effective_threshold, self.threshold
                ),
                suggested_correction: None, // Local re-synthesis will handle the correction
            });
        }

        Ok(SupervisorDecision::Continue {
            deviation: reflection.deviation_score,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{scaled_deviation_threshold, MIN_EFFECTIVE_THRESHOLD};

    /// 回归：High 档 base 0.01 叠加 MCTS 低分 0.1 时，
    /// 旧纯乘性公式得到 0.001，偏差判定退化为「任何非零偏差即转向」。
    /// 注意下限可被突破：0.10 偏差仍 > 0.01，仍会 pivot。
    /// 本测试固定的是「下限存在且非退化」，不是「恒真/恒假」。
    #[test]
    fn test_threshold_does_not_collapse_on_low_probability() {
        let scaled = scaled_deviation_threshold(0.01, 0.1);
        assert_eq!(scaled, MIN_EFFECTIVE_THRESHOLD);
        assert!(scaled > 0.001, "阈值不得塌缩到 0.001 量级，实际 {scaled}");
    }

    /// 敏感性方向必须保留：概率越低阈值越严（不得反向）。
    #[test]
    fn test_lower_probability_is_stricter() {
        let hi = scaled_deviation_threshold(0.2, 0.9);
        let lo = scaled_deviation_threshold(0.2, 0.4);
        assert!(
            lo < hi,
            "低概率路径阈值应更低（更严），实际 lo={lo} hi={hi}"
        );
    }

    /// 上界不得放宽超过 base：prob > 1 也不得突破用户设定。
    #[test]
    fn test_never_exceeds_base_threshold() {
        assert_eq!(scaled_deviation_threshold(0.05, 1.0), 0.05);
        assert_eq!(scaled_deviation_threshold(0.05, 2.0), 0.05);
    }

    /// Ultra 档 base 0.005 本身低于地板：不得被抬高到 0.01。
    #[test]
    fn test_floor_never_raises_a_lower_base() {
        assert_eq!(scaled_deviation_threshold(0.005, 1.0), 0.005);
        assert_eq!(scaled_deviation_threshold(0.005, 0.1), 0.005);
    }

    /// prob == 1.0 恒等：满信心路径不做任何缩放。
    #[test]
    fn test_full_confidence_is_identity() {
        for base in [0.2, 0.05, 0.01, 0.005] {
            assert_eq!(scaled_deviation_threshold(base, 1.0), base);
        }
    }

    /// prob == 0.0（仿真未给出可行路径）夹到下限，不得为 0。
    #[test]
    fn test_zero_probability_clamps_to_floor() {
        assert_eq!(
            scaled_deviation_threshold(0.01, 0.0),
            MIN_EFFECTIVE_THRESHOLD
        );
        assert_eq!(
            scaled_deviation_threshold(0.05, 0.0),
            MIN_EFFECTIVE_THRESHOLD
        );
    }
}
