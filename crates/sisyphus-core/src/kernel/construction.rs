//! 内核构建：构建器、执行器与 MCP 接线。
//!
//! `SisyphusKernel` 实现的一部分，为可读性从 `kernel.rs` 拆出。
//! 通过 `pub use kernel::*` 重新导出。

use std::sync::Arc;

use crate::*;

impl SisyphusKernel {
    #[allow(clippy::too_many_arguments)]
    /// New.
    pub fn new(
        state_store: Arc<MfsStore>,
        sop_store: Arc<SopSqlStore>,
        _vector_store: Arc<sisyphus_mem::VectorMemoryStore>,
        experience_bank: Arc<dyn sisyphus_types::experience::ExperienceBank>,
        tool_executor: Arc<dyn ToolExecutor>,
        test_executor: Arc<dyn sisyphus_types::TestExecutor>,
        intel: Arc<SisyphusIntel>,
        llm_client: Arc<dyn sisyphus_types::LlmClient>,
        fast_llm_client: Arc<dyn sisyphus_types::LlmClient>,
        expert_reviewer: Arc<dyn sisyphus_types::ExpertReviewer>,
        synthesizer: Arc<dyn sisyphus_intel::FlowSynthesizer>,
        meta_learner: Arc<parking_lot::Mutex<MetaLearner>>,
        consensus: Arc<dyn Consensus>,
        sql_store: Arc<sisyphus_mem::RoadmapSqlStore>,
        _embedding_client: Arc<dyn sisyphus_types::EmbeddingClient>,
    ) -> Self {
        let pm = Arc::new(sisyphus_roadmap::proposal_manager::ProposalManager::new(
            state_store.clone(),
        ));
        let rm = Arc::new(RoadmapManager::new(
            state_store.clone(),
            sql_store,
            pm.clone(),
        ));

        // A4 同类断层修复：此前此处用默认参数重建 LlmLogicalSimulator，
        // 架空了调用方（bridge）注入的配置超时实例；改为与其他组件
        // 一致地复用注入的 intel 字段。
        let intelligence_manager = Arc::new(IntelligenceManager::new(
            Arc::new(SisyphusIntel::new(
                intel.simulator.clone(),
                intel.logical_simulator.clone(),
                intel.reflector.clone(),
                intel.evolver.clone(),
            )),
            synthesizer,
            llm_client.clone(),
            expert_reviewer,
        ));

        let knowledge_manager = Arc::new(sisyphus_knowledge::AssetManager::new(
            std::path::PathBuf::from(".sisyphus/knowledge"),
        ));

        let intervention_manager =
            Arc::new(crate::intervention_manager::InterventionManager::new());
        let internalization_manager =
            Arc::new(crate::internalization_manager::InternalizationManager::new(
                intelligence_manager.llm_client.clone(),
                knowledge_manager.clone(),
            ));
        let visual_manager = Arc::new(sisyphus_visual::VisualPerceptionManager::new(
            Arc::new(sisyphus_visual::SisyphusVisualProvider::new(
                intelligence_manager.llm_client.clone(),
            )),
            intelligence_manager.llm_client.clone(),
        ));

        let execution_manager = Arc::new(ExecutionManager::new(
            state_store,
            sop_store,
            tool_executor,
            test_executor.clone(),
            intervention_manager.clone(),
            internalization_manager.clone(),
            Some(visual_manager.clone()),
        ));

        let channel = Arc::new(AgentChannel::new());
        let benchmark = Arc::new(parking_lot::Mutex::new(ContinuousBenchmark::new(
            ".omo/benchmarks",
        )));

        let strategic_manager = Arc::new(strategic_manager::StrategicManager::new(
            intelligence_manager.clone(),
            execution_manager.clone(),
            sisyphus_types::config::CognitiveLoopConfig::default(),
            channel.clone(),
            benchmark.clone(),
            meta_learner.clone(),
            experience_bank.clone(),
            knowledge_manager.clone(),
            rm.clone(),
            intervention_manager.clone(),
            internalization_manager.clone(),
            Some(visual_manager.clone()),
            test_executor.clone(),
        ));

        let swarm_coordinator = Arc::new(SwarmCoordinator::new(
            strategic_manager.clone(),
            intelligence_manager.clone(),
            channel.clone(),
        ));

        let logical_simulator = intel.logical_simulator.clone();

        // 为审计循环（Audit Loop）启动默认 swarm 代理
        swarm_coordinator.spawn_agent(AgentRole::Architect, "architect-0".into());
        swarm_coordinator.spawn_agent(AgentRole::Auditor, "auditor-0".into());

        let micro_reflector = Arc::new(sisyphus_intel::micro_reflector::LlmMicroReflector::new(
            fast_llm_client,
        ));

        Self {
            execution_manager,
            intelligence_manager,
            strategic_manager,
            swarm_coordinator,
            logical_simulator,
            consensus,
            intervention_manager,
            channel,
            benchmark,
            meta_learner,
            roadmap_manager: rm,
            proposal_manager: pm,
            experience_bank,
            micro_reflector: Some(micro_reflector),
            test_executor,
            strategic_reflector: None,
            milestone_generator: None,
            resource_optimizer: sisyphus_roadmap::resource_optimizer::ResourceOptimizer::new(),
        }
    }

    /// 覆盖认知循环配置。委托给 `apply_cognitive_loop_config`，
    /// 同时完成 AdaptiveDepthController 的运行时重配（评审 A4 接线）。
    pub fn with_cognitive_loop_config(
        self,
        config: sisyphus_types::config::CognitiveLoopConfig,
    ) -> Self {
        self.strategic_manager.apply_cognitive_loop_config(config);
        self
    }

    /// 附加 StrategicReflector 用于 LLM 增强的路线图分析。
    pub fn with_strategic_reflector(mut self, llm: Arc<dyn sisyphus_types::LlmClient>) -> Self {
        self.strategic_reflector = Some(Arc::new(
            sisyphus_roadmap::strategic_reflection::StrategicReflector::new(llm),
        ));

        self
    }

    /// 附加 MilestoneGenerator 用于 LLM 驱动的路线图创建。
    pub fn with_milestone_generator(mut self, llm: Arc<dyn sisyphus_types::LlmClient>) -> Self {
        self.milestone_generator = Some(Arc::new(
            sisyphus_roadmap::milestone_generator::MilestoneGenerator::new(llm),
        ));

        self
    }

    /// 在给定前缀下注册专用工具执行器（例如 "fs" 表示文件系统）。
    pub fn register_tool_executor(
        &self,

        prefix: &str,

        executor: Arc<dyn ToolExecutor>,
    ) -> Result<(), CoreError> {
        self.execution_manager.tool_router.register(prefix, executor);

        Ok(())
    }

    /// Init mcp executors.
    pub async fn init_mcp_executors(
        &self,
        config: &sisyphus_types::config::McpConfig,
        factory: &dyn sisyphus_types::McpExecutorFactory,
    ) -> Result<(), CoreError> {
        for (prefix, cmd_str) in &config.stdio_servers {
            let parts: Vec<&str> = cmd_str.split_whitespace().collect();

            if parts.is_empty() {
                tracing::warn!(prefix = %prefix, "Empty MCP stdio command, skipping");

                continue;
            }

            let command = parts[0];

            let args = &parts[1..];

            let owned_args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            match factory.connect_stdio(command.to_string(), owned_args).await {
                Ok(executor) => {
                    self.register_tool_executor(prefix, executor)?;

                    tracing::info!(prefix = %prefix, "Registered MCP stdio executor");
                },

                Err(e) => {
                    tracing::error!(prefix = %prefix, error = %e, "Failed to initialize MCP stdio executor");
                },
            }
        }

        for (prefix, url) in &config.http_servers {
            match factory.connect_url(url.clone()).await {
                Ok(executor) => {
                    self.register_tool_executor(prefix, executor)?;

                    tracing::info!(prefix = %prefix, "Registered MCP HTTP executor");
                },

                Err(e) => {
                    tracing::error!(prefix = %prefix, error = %e, "Failed to initialize MCP HTTP executor");
                },
            }
        }

        Ok(())
    }
}
