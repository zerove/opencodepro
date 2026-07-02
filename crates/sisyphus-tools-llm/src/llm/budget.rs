//! Budget provider trait and request context for LLM cost tracking.

use async_trait::async_trait;

/// Interface for linking LLM usage to project budgets and resource tracking.
#[async_trait]
pub trait BudgetProvider: Send + Sync {
    /// Verify if the estimated cost for a goal is within the projected budget.
    async fn check_budget(
        &self,
        project_id: &str,
        goal_id: &str,
        estimated_cost: f64,
    ) -> Result<(), String>;
    /// Record the actual cost and duration of an LLM call for a specific goal.
    async fn record_actual_cost(
        &self,
        project_id: &str,
        goal_id: &str,
        actual_cost: f64,
        duration_secs: f64,
    ) -> Result<(), String>;
}

/// Context used to attribute LLM costs to specific roadmap goals.
#[derive(Debug, Clone)]
pub struct RequestContext {
    pub project_id: String,
    pub goal_id: String,
}
