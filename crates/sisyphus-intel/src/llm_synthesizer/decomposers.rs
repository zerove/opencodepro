//! 目标分解策略：LLM 驱动，带关键词回退。

use crate::{classify_goal, ActionType};
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
/// Goal Decomposer.
pub trait GoalDecomposer: Send + Sync {
    async fn decompose(&self, goal: &str) -> Result<Vec<ActionType>, String>;
}

/// Llm Goal Decomposer.
pub struct LlmGoalDecomposer<F>
where
    F: Fn(String) -> Result<Vec<ActionType>, String> + Send + Sync + 'static,
{
    decompose_fn: F,
}

impl<F> LlmGoalDecomposer<F>
where
    F: Fn(String) -> Result<Vec<ActionType>, String> + Send + Sync + 'static,
{
    /// New.
    pub fn new(decompose_fn: F) -> Self {
        Self { decompose_fn }
    }
}

#[async_trait]
impl<F> GoalDecomposer for LlmGoalDecomposer<F>
where
    F: Fn(String) -> Result<Vec<ActionType>, String> + Send + Sync + 'static,
{
    async fn decompose(&self, goal: &str) -> Result<Vec<ActionType>, String> {
        (self.decompose_fn)(goal.to_string())
    }
}

/// Fallback Goal Decomposer.
pub struct FallbackGoalDecomposer {
    inner: Arc<dyn GoalDecomposer>,
}

impl FallbackGoalDecomposer {
    /// New.
    pub fn new(inner: Arc<dyn GoalDecomposer>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl GoalDecomposer for FallbackGoalDecomposer {
    async fn decompose(&self, goal: &str) -> Result<Vec<ActionType>, String> {
        match self.inner.decompose(goal).await {
            Ok(actions) => Ok(actions),
            Err(e) => {
                tracing::warn!(error = %e, "LLM decomposition failed, falling back to keyword matching");
                Ok(classify_goal(goal))
            },
        }
    }
}
