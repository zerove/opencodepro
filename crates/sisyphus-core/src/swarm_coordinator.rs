//! SwarmCoordinator 处理认知演进的 multi-agent 协作。
//! 实现"审计循环"：专业代理审查并完善提出的变更。

use crate::{
    intelligence_manager::IntelligenceManager, strategic_manager::StrategicManager, CoreError,
};
use parking_lot::RwLock;
use sisyphus_types::AgentChannel;
use sisyphus_types::SOPGraphConfig;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
/// Agent Role.
pub enum AgentRole {
    Architect,
    Auditor,
    Oracle,
    Coordinator,
}

/// Swarm Agent.
pub struct SwarmAgent {
    pub id: String,
    pub role: AgentRole,
    pub system_prompt: String,
}

/// Swarm Coordinator.
pub struct SwarmCoordinator {
    pub strategic_manager: Arc<StrategicManager>,
    pub intelligence_manager: Arc<IntelligenceManager>,
    pub agent_channel: Arc<AgentChannel>,
    pub agents: Arc<RwLock<Vec<SwarmAgent>>>,
}

impl SwarmCoordinator {
    /// New.
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

    /// Spawn agent.
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

    /// 实现"审计循环"：架构师提议 → 审计员审查 → 架构师完善。
    pub async fn coordinate_evolution(
        &self,
        goal: &str,
        current_config: &SOPGraphConfig,
    ) -> Result<SOPGraphConfig, CoreError> {
        tracing::info!("Starting Swarm Audit Loop for goal: {}", goal);

        // 1. 识别参与者
        let (architect_id, architect_prompt, auditor_id, auditor_prompt) = {
            let agents = self.agents.read();
            let architect = agents
                .iter()
                .find(|a| a.role == AgentRole::Architect)
                .ok_or_else(|| CoreError::Internal("No Architect agent spawned".into()))?;
            let auditor = agents
                .iter()
                .find(|a| a.role == AgentRole::Auditor)
                .ok_or_else(|| CoreError::Internal("No Auditor agent spawned".into()))?;
            (
                architect.id.clone(),
                architect.system_prompt.clone(),
                auditor.id.clone(),
                auditor.system_prompt.clone(),
            )
        };
        let architect = SwarmAgent {
            id: architect_id,
            role: AgentRole::Architect,
            system_prompt: architect_prompt,
        };
        let auditor = SwarmAgent {
            id: auditor_id,
            role: AgentRole::Auditor,
            system_prompt: auditor_prompt,
        };

        // 2. 架构师提出初始变更
        //
        // 降级策略：架构师提案解析失败（或 LLM 调用失败）时，回退到「维持当前配置」
        // 继续审计，而不是中止整次 goal 执行。理由：swarm 审计的定位是「高层优化」
        // 而非安全闸门；当前配置来自合成管线且随后仍要过逻辑仿真门禁。弱模型
        // （如本地 3B）经常把 args 字节数组写成字符串数组导致 JSON 反序列化失败，
        // 一次不可解析的提案不应让整条已合成、可仿真的计划流产。
        tracing::info!("Architect {} is proposing changes...", architect.id);
        let mut proposal = match self.real_agent_proposal(&architect, goal, current_config).await {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "Architect proposal unavailable/unparseable; auditing current config unchanged"
                );
                current_config.clone()
            },
        };

        let mut iteration = 0;
        let max_iterations = 3;

        loop {
            iteration += 1;
            tracing::info!("Audit Iteration {}/{}", iteration, max_iterations);

            // 3. 审计员审查提案
            //
            // 降级策略：审查意见解析失败时记为「不批准」并给出说明性 feedback，
            // 让循环走进 refine（refine 失败同样降级为维持原提案）；
            // 绝不因审计员 JSON 损坏而静默「批准」，也不直接中止执行。
            tracing::info!("Auditor {} is reviewing proposal...", auditor.id);
            let review = match self.real_agent_review(&auditor, &proposal).await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "Auditor review unavailable/unparseable; treating as not-approved"
                    );
                    AuditReview {
                        approved: false,
                        feedback: format!(
                            "Auditor response unavailable or unparseable: {e}. Proceeding without approval."
                        ),
                    }
                },
            };

            if review.approved {
                tracing::info!("Proposal approved by Auditor.");
                break;
            }

            if iteration >= max_iterations {
                tracing::warn!("Max audit iterations reached. Forcing a decision.");
                break;
            }

            // 4. 架构师根据审查意见完善
            //
            // 降级策略：refine 失败时维持上一版提案（审计循环仍会在
            // max_iterations 后强制收尾），不因一次模型输出损坏而中止。
            tracing::info!(
                "Architect {} is refining proposal based on feedback...",
                architect.id
            );
            let refined = match self.real_agent_refine(&architect, &proposal, &review).await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "Architect refine unavailable/unparseable; keeping previous proposal"
                    );
                    if iteration >= max_iterations {
                        tracing::warn!("Max audit iterations reached. Forcing a decision.");
                        break;
                    }
                    continue;
                },
            };
            proposal = refined;
        }

        Ok(proposal)
    }

    // --- 仿真辅助（后续将通过 AgentChannel 替换为真实 LLM 调用） ---

    async fn call_agent(
        &self,
        agent: &SwarmAgent,
        user_message: &str,
    ) -> Result<String, CoreError> {
        tracing::debug!("Calling agent {}: {:?}", agent.id, agent.role);

        let response = self
            .intelligence_manager
            .llm_client
            .chat(&agent.system_prompt, user_message, true)
            .await
            .map_err(|e| {
                CoreError::Internal(format!("LLM call failed for agent {}: {}", agent.id, e))
            })?;

        Ok(response)
    }

    async fn real_agent_proposal(
        &self,
        agent: &SwarmAgent,
        goal: &str,
        config: &SOPGraphConfig,
    ) -> Result<SOPGraphConfig, CoreError> {
        let config_json = serde_json::to_string(config)
            .map_err(|e| CoreError::Internal(format!("Serialization error: {}", e)))?;

        let user_message = format!(
            "Goal: {}\nCurrent SOP Config (JSON):\n{}\n\nPlease propose an optimized SOPGraphConfig in JSON format. Return ONLY the JSON object.",
            goal, config_json
        );

        let response = self.call_agent(agent, &user_message).await?;

        let json_text = sisyphus_intel::json_extract::extract_json(&response);
        let new_config: SOPGraphConfig = serde_json::from_str(&json_text).map_err(|e| {
            CoreError::Internal(format!(
                "Failed to parse proposal JSON: {}. Response: {}",
                e, response
            ))
        })?;

        Ok(new_config)
    }

    async fn real_agent_review(
        &self,
        agent: &SwarmAgent,
        proposal: &SOPGraphConfig,
    ) -> Result<AuditReview, CoreError> {
        let proposal_json = serde_json::to_string(proposal)
            .map_err(|e| CoreError::Internal(format!("Serialization error: {}", e)))?;

        let user_message = format!(
            "Proposed SOP Config (JSON):\n{}\n\nPlease review this proposal for efficiency, safety, and logical correctness. \
            Return a JSON object with fields 'approved' (boolean) and 'feedback' (string). Return ONLY the JSON object.",
            proposal_json
        );

        let response = self.call_agent(agent, &user_message).await?;
        let json_text = sisyphus_intel::json_extract::extract_json(&response);

        let review: AuditReview = serde_json::from_str(&json_text).map_err(|e| {
            CoreError::Internal(format!(
                "Failed to parse review JSON: {}. Response: {}",
                e, response
            ))
        })?;

        Ok(review)
    }

    async fn real_agent_refine(
        &self,
        agent: &SwarmAgent,
        proposal: &SOPGraphConfig,
        review: &AuditReview,
    ) -> Result<SOPGraphConfig, CoreError> {
        let proposal_json = serde_json::to_string(proposal)
            .map_err(|e| CoreError::Internal(format!("Serialization error: {}", e)))?;

        let user_message = format!(
            "Current Proposal (JSON):\n{}\nFeedback from Auditor:\n{}\n\nPlease refine the SOPGraphConfig to address the feedback. \
            Return the updated SOPGraphConfig in JSON format. Return ONLY the JSON object.",
            proposal_json, review.feedback
        );

        let response = self.call_agent(agent, &user_message).await?;
        let json_text = sisyphus_intel::json_extract::extract_json(&response);

        let refined_config: SOPGraphConfig = serde_json::from_str(&json_text).map_err(|e| {
            CoreError::Internal(format!(
                "Failed to parse refined JSON: {}. Response: {}",
                e, response
            ))
        })?;

        Ok(refined_config)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
/// Audit Review.
pub struct AuditReview {
    pub approved: bool,
    pub feedback: String,
}
