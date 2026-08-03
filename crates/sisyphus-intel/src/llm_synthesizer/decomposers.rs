//! Goal decomposition strategies: LLM-driven with keyword fallback.

use crate::{classify_goal, ActionType};
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
pub trait GoalDecomposer: Send + Sync {
    async fn decompose(&self, goal: &str) -> Result<Vec<ActionType>, String>;
}

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

pub struct FallbackGoalDecomposer {
    inner: Arc<dyn GoalDecomposer>,
}

impl FallbackGoalDecomposer {
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
            }
        }
    }
}
