//! LLM 驱动与逻辑 SOP 合成器。
//!
//! 提供 [`GoalDecomposer`] 实现用于动作分解，以及将目标
//! 转化为 SOP 图谱的 [`FlowSynthesizer`] 实现。模块按域拆分：
//! 模块按域拆分为文件：
//! - [`decomposers`]：目标分解策略
//! - [`synthesizer`]：LLM 驱动的流水线合成器
//! - logical：对抗式、评估器驱动的逻辑合成器

mod decomposers;
mod logical;
mod synthesizer;
#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategicLesson {
    pub goal: String,
    pub sop_json: String,
    pub feedback: String,
    pub success: bool,
}

pub use decomposers::{FallbackGoalDecomposer, GoalDecomposer, LlmGoalDecomposer};
pub use logical::LogicalSOPSynthesizer;
pub use synthesizer::LlmDrivenSynthesizer;
