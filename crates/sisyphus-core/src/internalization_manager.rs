use std::sync::Arc;
use tracing::info;

#[derive(Debug, Clone)]
/// Internalization Request.
pub struct InternalizationRequest {
    pub node_id: String,
    pub prompt: String,
    pub context: String,
    pub correction: String,
}

/// Internalization Manager.
pub struct InternalizationManager {
    llm_client: Arc<dyn sisyphus_types::LlmClient>,
    knowledge_manager: Arc<sisyphus_knowledge::AssetManager>,
}

impl InternalizationManager {
    /// New.
    pub fn new(
        llm_client: Arc<dyn sisyphus_types::LlmClient>,
        knowledge_manager: Arc<sisyphus_knowledge::AssetManager>,
    ) -> Self {
        Self {
            llm_client,
            knowledge_manager,
        }
    }

    /// Internalize feedback.
    pub async fn internalize_feedback(&self, req: InternalizationRequest) -> Result<(), String> {
        info!(node_id = %req.node_id, "Starting cognitive internalization for human feedback");

        // 1. 将原始修正提炼为可泛化的洞察
        let insight = self.distill_insight(&req).await?;

        info!(node_id = %req.node_id, insight = %insight, "Distilled cognitive insight from feedback");

        // 2. 将洞察作为高置信度资产持久化到知识图谱
        // 在真实系统中，我们会创建结构化的 'Insight' 资产。
        // 目前，我们将其作为元数据标签或特定资产类型存储在知识管理器中。
        let asset_id = format!("insight_{}_{}", req.node_id, uuid::Uuid::new_v4());

        // 我们将洞察封装为专用的 Insight 资产。
        let content =
            sisyphus_types::asset::AssetContent::Insight(sisyphus_types::asset::InsightData {
                observation: insight.clone(),
                reasoning: "Human-provided correction".into(),
                evidence_ids: vec![req.node_id.clone()],
                confidence: 1.0,
            });

        self.knowledge_manager
            .save_asset(
                &asset_id,
                format!("Human Correction: {}", req.node_id),
                content,
                1.0, // Max confidence since it's human-provided
            )
            .await
            .map_err(|e| e.to_string())?;

        info!(asset_id = %asset_id, "Successfully internalized feedback into knowledge graph");
        Ok(())
    }

    async fn distill_insight(&self, req: &InternalizationRequest) -> Result<String, String> {
        let prompt = format!(
            "You are a cognitive distillation engine. A human user has corrected an AI agent's execution.\n\n\
            Original Prompt: {}\n\
            Execution Context: {}\n\
            Human Correction: {}\n\n\
            Task: Extract the generalizable 'lesson learned' from this correction. \n\
            The output should be a concise, imperative rule (e.g., 'When X occurs, always do Y instead of Z') \
            that can be used to guide future SOP synthesis or evolution. Avoid mentioning specific IDs, focus on the semantic pattern.\n\
            Output only the rule.",
            req.prompt, req.context, req.correction
        );

        let response = self.llm_client.generate(&prompt).await.map_err(|e| e.to_string())?;

        Ok(response.trim().to_string())
    }
}
