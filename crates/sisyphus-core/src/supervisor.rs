use std::sync::Arc;
use crate::{ExecutionContext, NodeOutput};
use sisyphus_types::{RuntimeSupervisor, SupervisorDecision};
use async_trait::async_trait;

pub struct DefaultSupervisor;

#[async_trait]
impl RuntimeSupervisor for DefaultSupervisor {
    async fn evaluate(
        &self,
        _node_name: &str,
        _output: &NodeOutput,
        _ctx: Arc<ExecutionContext>,
    ) -> Result<SupervisorDecision, String> {
        // Default implementation: always continue
        Ok(SupervisorDecision::Continue { deviation: 0.0 })
    }
}
