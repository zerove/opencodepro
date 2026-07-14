//! Orchestrator for the Sisyphus self-programming loop.

use std::sync::Arc;
use tokio::sync::mpsc;
use sisyphus_intel::{ToolCoder, WasmCompiler, ToolGap};
use sisyphus_types::CognitiveEvent;
use crate::ExecutionManager;
use tracing::{info, warn, error};

pub struct SelfProgrammingManager {
    pub coder: ToolCoder,
    pub compiler: WasmCompiler,
    pub verifier: sisyphus_intel::ToolVerifier,
    pub execution_manager: Arc<ExecutionManager>,
    pub knowledge_manager: Arc<sisyphus_knowledge::AssetManager>,
    pub event_tx: parking_lot::Mutex<Option<mpsc::UnboundedSender<CognitiveEvent>>>,
}

impl SelfProgrammingManager {
    pub fn new(
        llm_client: Arc<dyn sisyphus_tools_llm::LlmClient>,
        execution_manager: Arc<ExecutionManager>,
        knowledge_manager: Arc<sisyphus_knowledge::AssetManager>,
    ) -> Self {
        Self {
            coder: ToolCoder::new(llm_client.clone()),
            compiler: WasmCompiler::new(),
            verifier: sisyphus_intel::ToolVerifier::new(llm_client),
            execution_manager,
            knowledge_manager,
            event_tx: parking_lot::Mutex::new(None),
        }
    }

    pub fn set_event_sender(&self, tx: mpsc::UnboundedSender<CognitiveEvent>) {
        *self.event_tx.lock() = Some(tx);
    }

    async fn notify(&self, event: CognitiveEvent) {
        if let Some(tx) = self.event_tx.lock().clone() {
            let _ = tx.send(event);
        }
    }

    /// Executes the full self-programming loop for a detected gap.
    pub async fn resolve_gap(&self, gap: ToolGap) -> Result<(), crate::CoreError> {
        info!(gap_id = %gap.gap_id, "Starting self-programming loop to resolve tool gap");

        let mut feedback: Option<String> = None;
        let max_retries = 3;

        for attempt in 1..=max_retries {
            info!("Self-programming attempt {}/{}", attempt, max_retries);

            // 1. Generate Code
            self.notify(CognitiveEvent::ToolGenerationStarted { 
                tool_id: gap.suggested_tool_spec.tool_id.clone(), 
                capability: gap.required_capability.clone() 
            }).await;

            let code = self.coder.generate_code(&gap.suggested_tool_spec, feedback.clone()).await
                .map_err(|e| crate::CoreError::Internal(e.to_string()))?;

            // 2. Compile to WASM
            let wasm_path = self.compiler.compile(&gap.suggested_tool_spec.tool_id, &code).await
                .map_err(|e| crate::CoreError::Internal(e.to_string()))?;

            // 3. Verify Tool
            let v_report = self.verifier.verify(&gap.suggested_tool_spec, &wasm_path).await
                .map_err(|e| crate::CoreError::Internal(e.to_string()))?;

            if v_report.confidence_score >= 0.7 {
                self.notify(CognitiveEvent::ToolVerified { 
                    tool_id: gap.suggested_tool_spec.tool_id.clone(), 
                    score: v_report.confidence_score as f64 
                }).await;
                info!("Tool verified successfully (score: {:.2})", v_report.confidence_score);

                // 4. Deploy and Sediment
                let asset_id = self.knowledge_manager.sediment_tool(
                    gap.suggested_tool_spec.clone(),
                    wasm_path
                ).await.map_err(|e| crate::CoreError::Internal(e.to_string()))?;
                
                self.notify(CognitiveEvent::ToolDeployed { 
                    tool_id: gap.suggested_tool_spec.tool_id.clone(), 
                    asset_id: asset_id.clone() 
                }).await;
                self.execution_manager.notify_tool_deployed(&gap.suggested_tool_spec.tool_id);

                info!("Successfully resolved tool gap {}. Tool {} is now available.", gap.gap_id, gap.suggested_tool_spec.tool_id);
                return Ok(());
            }

            warn!("Attempt {} failed (score: {:.2}). Failures: {:?}", attempt, v_report.confidence_score, v_report.failures);
            feedback = Some(format!("Attempt {} failures:\n{}", attempt, v_report.failures.join("\n")));
        }

        Err(crate::CoreError::Internal(format!("Failed to resolve gap after {} attempts", max_retries)))
    }
}
