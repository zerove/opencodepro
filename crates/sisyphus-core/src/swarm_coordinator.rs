//! SwarmCoordinator handles multi-agent collaboration for cognitive evolution.
//! It implements the "Audit Loop" where specialized agents review and refine proposed changes.

use std::sync::Arc;
use parking_lot::RwLock;
use serde_json::Value;
use sisyphus_types::{AgentMessage, SOPGraphConfig};
use crate::{
    strategic_manager::StrategicManager, 
    CoreError, 
    execution_manager::ExecutionManager,
    intelligence_manager::IntelligenceManager
};
use sisyphus_tools_system::agent_channel::AgentChannel;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentRole {
    Architect,
    Auditor,
    Oracle,
    Coordinator,
}

pub struct SwarmAgent {
    pub id: String,
    pub role: AgentRole,
    pub system_prompt: String,
}

pub struct SwarmCoordinator {
    pub strategic_manager: Arc<StrategicManager>,
    pub intelligence_manager: Arc<IntelligenceManager>,
    pub agent_channel: Arc<AgentChannel>,
    pub agents: Arc<RwLock<Vec<SwarmAgent>>>,
}

impl SwarmCoordinator {
    pub fn new(
        strategic_manager: Arc<StrategicManager>,
        intelligence_manager: Arc<IntelligenceManager>,
        agent_channel: Arc<AgentChannel>,
    ) -> Self {
        Self {
            strategic_manager,
            intelligence_manager,
            agent_channel,
            agents: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn spawn_agent(&self, role: AgentRole, id: String) {
        let prompt = match role {
            AgentRole::Architect => "You are the SOP Architect. Your goal is to propose the most efficient topological changes to the SOP graph to achieve the goal.",
            AgentRole::Auditor => "You are the Security and Efficiency Auditor. Your goal is to find flaws, race conditions, or inefficiencies in proposed SOP changes.",
            AgentRole::Oracle => "You are the Domain Oracle. You provide deep theoretical insights into the problem domain to guide the Architect.",
            AgentRole::Coordinator => "You are the Swarm Coordinator. You synthesize reports from multiple agents into a final decision.",
        };

        let agent = SwarmAgent {
            id,
            role,
            system_prompt: prompt.to_string(),
        };

        self.agents.write().push(agent);
    }

    /// Implements the "Audit Loop": Architect proposes -> Auditor reviews -> Architect refines.
    pub async fn coordinate_evolution(
        &self,
        goal: &str,
        current_config: &SOPGraphConfig,
    ) -> Result<SOPGraphConfig, CoreError> {
        tracing::info!("Starting Swarm Audit Loop for goal: {}", goal);
        
        // 1. Identify participants
        let agents = self.agents.read();
        let architect = agents.iter().find(|a| a.role == AgentRole::Architect)
            .ok_or_else(|| CoreError::Internal("No Architect agent spawned".into()))?;
        let auditor = agents.iter().find(|a| a.role == AgentRole::Auditor)
            .ok_or_else(|| CoreError::Internal("No Auditor agent spawned".into()))?;
        
        // 2. architect proposes an initial change
        tracing::info!("Architect {} is proposing changes...", architect.id);
        let mut proposal = self.real_agent_proposal(architect, goal, current_config).await?;
        
        let mut iteration = 0;
        let max_iterations = 3;
        
        loop {
            iteration += 1;
            tracing::info!("Audit Iteration {}/{}", iteration, max_iterations);
            
            // 3. auditor reviews the proposal
            tracing::info!("Auditor {} is reviewing proposal...", auditor.id);
            let review = self.real_agent_review(auditor, &proposal).await?;
            
            if review.approved {
                tracing::info!("Proposal approved by Auditor.");
                break;
            }
            
            if iteration >= max_iterations {
                tracing::warn!("Max audit iterations reached. Forcing a decision.");
                break;
            }
            
            // 4. architect refines based on review
            tracing::info!("Architect {} is refining proposal based on feedback...", architect.id);
            proposal = self.real_agent_refine(architect, &proposal, &review).await?;
        }
        
        Ok(proposal)
    }

    // --- Simulation helpers (to be replaced by actual LLM calls via AgentChannel) ---

    async fn call_agent(&self, agent: &SwarmAgent, user_message: &str) -> Result<String, CoreError> {
        tracing::debug!("Calling agent {}: {:?}", agent.id, agent.role);
        
        let response = self.intelligence_manager.llm_client
            .chat(&agent.system_prompt, user_message, true)
            .await
            .map_err(|e| CoreError::Internal(format!("LLM call failed for agent {}: {}", agent.id, e)))?;

        Ok(response)
    }

    async fn real_agent_proposal(&self, agent: &SwarmAgent, goal: &str, config: &SOPGraphConfig) -> Result<SOPGraphConfig, CoreError> {
        let config_json = serde_json::to_string(config)
            .map_err(|e| CoreError::Internal(format!("Serialization error: {}", e)))?;
        
        let user_message = format!(
            "Goal: {}\nCurrent SOP Config (JSON):\n{}\n\nPlease propose an optimized SOPGraphConfig in JSON format. Return ONLY the JSON object.",
            goal, config_json
        );

        let response = self.call_agent(agent, &user_message).await?;
        
        let json_text = extract_json(&response);
        let new_config: SOPGraphConfig = serde_json::from_str(&json_text)
            .map_err(|e| CoreError::Internal(format!("Failed to parse proposal JSON: {}. Response: {}", e, response)))?;

        Ok(new_config)
    }

    async fn real_agent_review(&self, agent: &SwarmAgent, proposal: &SOPGraphConfig) -> Result<AuditReview, CoreError> {
        let proposal_json = serde_json::to_string(proposal)
            .map_err(|e| CoreError::Internal(format!("Serialization error: {}", e)))?;
        
        let user_message = format!(
            "Proposed SOP Config (JSON):\n{}\n\nPlease review this proposal for efficiency, safety, and logical correctness. \
            Return a JSON object with fields 'approved' (boolean) and 'feedback' (string). Return ONLY the JSON object.",
            proposal_json
        );

        let response = self.call_agent(agent, &user_message).await?;
        let json_text = extract_json(&response);
        
        let review: AuditReview = serde_json::from_str(&json_text)
            .map_err(|e| CoreError::Internal(format!("Failed to parse review JSON: {}. Response: {}", e, response)))?;

        Ok(review)
    }

    async fn real_agent_refine(&self, agent: &SwarmAgent, proposal: &SOPGraphConfig, review: &AuditReview) -> Result<SOPGraphConfig, CoreError> {
        let proposal_json = serde_json::to_string(proposal)
            .map_err(|e| CoreError::Internal(format!("Serialization error: {}", e)))?;
        
        let user_message = format!(
            "Current Proposal (JSON):\n{}\nFeedback from Auditor:\n{}\n\nPlease refine the SOPGraphConfig to address the feedback. \
            Return the updated SOPGraphConfig in JSON format. Return ONLY the JSON object.",
            proposal_json, review.feedback
        );

        let response = self.call_agent(agent, &user_message).await?;
        let json_text = extract_json(&response);
        
        let refined_config: SOPGraphConfig = serde_json::from_str(&json_text)
            .map_err(|e| CoreError::Internal(format!("Failed to parse refined JSON: {}. Response: {}", e, response)))?;

        Ok(refined_config)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AuditReview {
    pub approved: bool,
    pub feedback: String,
}

fn extract_json(text: &str) -> String {
    if let Some(start) = text.find("```json") {
        if let Some(end) = text[start + 7..].find("```") {
            return text[start + 7..start + 7 + text[start + 7..].find("```").unwrap()].trim().to_string();
        }
    } else if let Some(start) = text.find("```") {
        if let Some(end) = text[start + 3..].find("```") {
            return text[start + 3..start + 3 + text[start + 3..].find("```").unwrap()].trim().to_string();
        }
    }
    text.trim().to_string()
}
