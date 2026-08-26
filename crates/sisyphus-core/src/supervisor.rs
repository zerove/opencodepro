use crate::{ExecutionContext, NodeOutput};
use async_trait::async_trait;
use sisyphus_types::{RuntimeSupervisor, SupervisorDecision};
use std::sync::Arc;

pub struct DefaultSupervisor;

#[async_trait]
impl RuntimeSupervisor for DefaultSupervisor {
    async fn evaluate(
        &self,
        _node_name: &str,
        _output: &NodeOutput,
        _ctx: Arc<ExecutionContext>,
    ) -> Result<SupervisorDecision, sisyphus_types::TypesError> {
        // 默认实现：始终继续
        Ok(SupervisorDecision::Continue { deviation: 0.0 })
    }
}
