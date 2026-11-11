//! 错误 → 提示（Hint）抽样模块。
//!
//! 对应计划任务 T2.4「错误提示有效性抽样」：把“错误提示”本身纳入可采样、
//! 可记录的实验闭环——先对错误列表采样命中的提示（含建议动作），
//! 再根据“按提示行动”的执行结果回填判定。
//!
//! - [`suggest_hint`]：从错误文本命中一条提示（多个 pattern 命中时取最长的）；
//!   未命中返回 `None`（不编造提示）。
//!
//! - [`sample_hint_outcome`] / [`outcome_after_action`]：单条错误/动作的判定闭环。
//! - [`sample_hints`]：对错误列表做抽样聚合。

/// 一条提示定义。
#[derive(Debug, Clone, Copy)]
pub struct HintDef {
    /// 提示 ID（写进样本记录与报告）。
    pub id: &'static str,
    /// 命中条件：错误文本包含该子串（大小写不敏感）。
    pub pattern: &'static str,
    /// 人类可读提示文案。
    pub message: &'static str,
    /// 建议的下一步动作（人类可读，如 `sop show <sop_id>`）；无法给出时为 `None`。
    pub suggested_action: Option<&'static str>,
}

/// 可复用的错误提示目录。
pub const HINTS: &[HintDef] = &[
    HintDef {
        id: "hint-node-no-real-result",
        pattern: "without any real execution result",
        message: "检查节点显式成功状态/阈值与反思配置；可先用 sop show/确定性模板验证。",
        suggested_action: Some("sop show <sop_id>"),
    },
    HintDef {
        id: "hint-strategic-pivot",
        pattern: "StrategicPivot",
        message: "检查节点显式成功状态/阈值与反思配置；可先用 sop show/确定性模板验证。",
        suggested_action: Some("sop show <sop_id>"),
    },
    HintDef {
        id: "hint-llm-dependency",
        pattern: "LLM",
        message: "可能依赖 LLM/审计能力；请检查配置与凭据。",
        suggested_action: Some("provider list/test"),
    },
];

/// 一条错误 + 命中的提示 + 按提示行动后的判定。
#[derive(Debug, Clone)]
pub struct HintSample {
    /// 错误文本。
    pub error: String,
    /// 命中的提示 ID（未命中为 `None`）。
    pub hint_id: Option<String>,
    /// 下一步建议动作（命中提示后填充）。
    pub suggested_action: Option<String>,
    /// 采样判定。
    pub outcome: HintOutcome,
}

impl HintSample {
    /// 根据一次“按提示建议行动”的执行结果回填判定：
    /// 动作成功 → `Resolved`；动作失败 → `Unresolved`；未执行 → `Indeterminate`。
    pub fn with_action(&mut self, action_succeeded: Option<bool>) -> HintOutcome {
        self.outcome = match action_succeeded {
            Some(true) => HintOutcome::Resolved,
            Some(false) => HintOutcome::Unresolved,
            None => HintOutcome::Indeterminate,
        };
        self.outcome
    }
}

/// 按提示行动后的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintOutcome {
    /// 按提示建议的动作重试后恢复。
    Resolved,
    /// 重试后仍失败。
    Unresolved,
    /// 未执行重试或缺少判定条件，标记为"无法判定"。
    Indeterminate,
}

impl HintOutcome {
    /// 是否已判定。
    pub fn is_determined(self) -> bool {
        !matches!(self, Self::Indeterminate)
    }
}

/// 一段 hint 抽样结果汇总。
#[derive(Debug, Clone, Default)]
pub struct HintReport {
    /// 采样样本（原始顺序保留）。
    pub samples: Vec<HintSample>,
    /// 判定恢复的数量。
    pub resolved: usize,
    /// 判定未恢复的数量。
    pub unresolved: usize,
    /// 无法判定的数量。
    pub indeterminate: usize,
}

/// 对错误列表做 hint 抽样：记录每条错误命中的提示与建议动作，判定置为 `Indeterminate`。
pub fn sample_hints(errors: &[String]) -> HintReport {
    let samples = errors
        .iter()
        .map(|error| {
            let hint = suggest_hint(error);
            HintSample {
                error: error.clone(),
                hint_id: hint.map(|h| h.id.to_string()),
                suggested_action: hint.and_then(|h| h.suggested_action.map(|a| a.to_string())),
                outcome: HintOutcome::Indeterminate,
            }
        })
        .collect::<Vec<_>>();

    let resolved = samples.iter().filter(|s| s.outcome == HintOutcome::Resolved).count();
    let unresolved = samples.iter().filter(|s| s.outcome == HintOutcome::Unresolved).count();
    let indeterminate = samples.iter().filter(|s| s.outcome == HintOutcome::Indeterminate).count();

    HintReport {
        samples,
        resolved,
        unresolved,
        indeterminate,
    }
}

/// 对单个错误完成一次“提示 → 行动 → 判定”采样：无动作时判定为 `Indeterminate`。
pub fn sample_hint_outcome(error: &str, execute_next_action: bool) -> HintSample {
    let hint = suggest_hint(error);
    HintSample {
        error: error.to_string(),
        hint_id: hint.map(|h| h.id.to_string()),
        suggested_action: hint.and_then(|h| h.suggested_action.map(|a| a.to_string())),
        outcome: if execute_next_action {
            HintOutcome::Resolved
        } else {
            HintOutcome::Indeterminate
        },
    }
}

/// 按错误文本命中一条提示；多个 pattern 命中时取最长的（更具体）。
pub fn suggest_hint(error: &str) -> Option<&'static HintDef> {
    HINTS
        .iter()
        .filter(|hint| error.to_ascii_lowercase().contains(&hint.pattern.to_ascii_lowercase()))
        .max_by_key(|hint| hint.pattern.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggests_longest_pattern_on_multiple_matches() {
        let hint = suggest_hint("StrategicPivot while doing LLM planning").expect("hint");
        assert_eq!(hint.id, "hint-strategic-pivot");
        assert_eq!(hint.suggested_action, Some("sop show <sop_id>"));
    }

    #[test]
    fn suggests_llm_hint_for_llm_errors() {
        let hint = suggest_hint("LLM provider unavailable").expect("hint");
        assert_eq!(hint.id, "hint-llm-dependency");
    }

    #[test]
    fn suggests_none_for_unrelated_error() {
        assert!(suggest_hint("file not found").is_none());
    }

    #[test]
    fn sample_hint_outcome_resolved_when_action_succeeds() {
        let sample = sample_hint_outcome(
            "execute goal failed: without any real execution result",
            true,
        );
        assert_eq!(sample.hint_id.as_deref(), Some("hint-node-no-real-result"));
        assert_eq!(
            sample.suggested_action.as_deref(),
            Some("sop show <sop_id>")
        );
        assert_eq!(sample.outcome, HintOutcome::Resolved);
        assert!(sample.outcome.is_determined());
    }

    #[test]
    fn sample_hint_outcome_indeterminate_without_action() {
        let sample = sample_hint_outcome("LLM provider unavailable", false);
        assert_eq!(sample.hint_id.as_deref(), Some("hint-llm-dependency"));
        assert_eq!(sample.outcome, HintOutcome::Indeterminate);
        assert!(!sample.outcome.is_determined());
    }

    #[test]
    fn sample_with_action_backfills_resolution() {
        let mut sample = HintSample {
            error: "boom".into(),
            hint_id: None,
            suggested_action: None,
            outcome: HintOutcome::Indeterminate,
        };
        assert_eq!(sample.with_action(Some(true)), HintOutcome::Resolved);
        assert_eq!(sample.outcome, HintOutcome::Resolved);
        assert_eq!(sample.with_action(Some(false)), HintOutcome::Unresolved);
        assert_eq!(sample.with_action(None), HintOutcome::Indeterminate);
    }

    #[test]
    fn sample_hints_aggregates_and_starts_indeterminate() {
        let report = sample_hints(&[
            "StrategicPivot: no real result".to_string(),
            "some random error".to_string(),
            "LLM provider unavailable".to_string(),
        ]);
        assert_eq!(report.samples.len(), 3);
        assert_eq!(
            report.samples[0].hint_id.as_deref(),
            Some("hint-strategic-pivot")
        );
        assert_eq!(
            report.samples[2].hint_id.as_deref(),
            Some("hint-llm-dependency")
        );
        assert!(report.samples[1].hint_id.is_none());
        assert_eq!(report.indeterminate, 3, "采样阶段尚未执行按提示行动的动作");
        assert_eq!(report.resolved, 0);
        assert_eq!(report.unresolved, 0);
    }
}
