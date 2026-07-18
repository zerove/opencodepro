use sisyphus_types::ExperienceRetriever;
use sisyphus_types::experience::{ExperienceBank, ExperienceQuery, ExperienceTuple};
use std::sync::Arc;

pub struct BankRetriever {
    pub bank: Arc<dyn ExperienceBank>,
}

impl BankRetriever {
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
    ) -> Result<Vec<ExperienceTuple>, String> {
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

    async fn record_experience(&self, exp: ExperienceTuple) -> Result<(), String> {
        self.bank.record(exp).await
    }
}
