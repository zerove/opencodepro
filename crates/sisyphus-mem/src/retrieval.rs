use sisyphus_types::ExperienceRetriever;
use sisyphus_types::experience::{ExperienceBank, ExperienceQuery, ExperienceTuple};
use std::sync::Arc;

/// [`ExperienceRetriever`] adapter that delegates to an in-memory [`ExperienceBank`].
pub struct BankRetriever {
    pub bank: Arc<dyn ExperienceBank>,
}

impl BankRetriever {
    /// Wrap `bank` behind the [`ExperienceRetriever`] trait for use by the
    /// cognitive loop's reflection stage.
    pub fn new(bank: Arc<dyn ExperienceBank>) -> Self {
        Self { bank }
    }
}

#[async_trait::async_trait]
impl ExperienceRetriever for BankRetriever {
    async fn retrieve_relevant(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ExperienceTuple>, sisyphus_types::TypesError> {
        let q = ExperienceQuery {
            query_text: query.to_string(),
            context_hash: format!("{:x}", md5::compute(query)),
            project_id: None,
            action_id: None,
            visual_embedding: None,
            limit,
        };
        self.bank.query_similar(q).await
    }

    async fn record_experience(
        &self,
        exp: ExperienceTuple,
    ) -> Result<(), sisyphus_types::TypesError> {
        self.bank.record(exp).await
    }
}
