use sisyphus_core::Consensus;

/// No-op consensus implementation: always assumes this node is the leader.
pub(crate) struct NoOpConsensus;

#[async_trait::async_trait]
impl Consensus for NoOpConsensus {
    async fn is_leader(&self) -> bool {
        true
    }
    async fn get_leader(&self) -> String {
        "self".into()
    }
}
