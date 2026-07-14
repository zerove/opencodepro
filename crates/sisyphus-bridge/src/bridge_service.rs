//! REST API for Sisyphus Bridge (axum HTTP layer alongside gRPC).
use async_trait::async_trait;
use crate::jcode_caller::DebugSocketToolCaller;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use sisyphus_core::{Consensus, SisyphusKernel};
use sisyphus_mem::{MfsStore, SopSqlStore, SisyphusStore};

use sisyphus_sandbox::{PluginManager, WasmToolExecutor};
use sisyphus_mem::skill_store::SkillStore;
use sisyphus_tools_agent::agent_registry::AgentRegistry;
use sisyphus_tools_agent::skill_registry::{RemoteSkillRegistry, SkillRegistry};
use sisyphus_tools_agent::workspace_manager::WorkspaceManager;
use sisyphus_tools_agent::team::TeamOrchestrator;
use sisyphus_tools_core::feedback::FeedbackStore;
use sisyphus_tools_core::hook::{HookRegistry, hooks_from_skill};
use sisyphus_tools_executor::executor::BuiltinToolExecutor;
use sisyphus_tools_executor::executor_mcp::MCPToolExecutor;
use sisyphus_tools_llm::{DynamicLlmClient, LlmClient};
use sisyphus_tools_mcp::custom_tool::CustomToolLoader;
use sisyphus_tools_mcp::mcp::McpTool;
use sisyphus_tools_mcp::tool_registry::{ToolInfo, ToolRegistry, ToolSource};
use sisyphus_tools_session::skill::SkillLoader;
use sisyphus_bridge_api::sisyphus_bridge_client::SisyphusBridgeClient;
use sisyphus_tools_agent::jcode_executor::JcodeToolCaller;
use sisyphus_tools_agent::jcode_executor::JcodeToolExecutor;
use sisyphus_types::{Config, ExecutorRouter, ToolExecutor};
use tracing::{info, warn};

use crate::consensus::{ClusterConsensus, NoOpConsensus};
use crate::strategic_executor::StrategicExecutor;
use crate::tools::{register_builtin_tools, register_strategic_tools};
use sisyphus_intel::MetaLearningSynthesizer;

pub struct MySisyphusBridge {
    pub(crate) kernel: Arc<SisyphusKernel>,
    pub(crate) event_bus: tokio::sync::broadcast::Sender<sisyphus_bridge_api::BridgeEvent>,
    pub(crate) consensus: Arc<dyn Consensus>,
    pub(crate) state_store: Arc<dyn sisyphus_types::StateStore>,
    pub(crate) default_fuel: u64,
    pub(crate) agent_registry: Arc<AgentRegistry>,
    pub(crate) tool_registry: Arc<ToolRegistry>,
    pub(crate) team_orchestrator: Arc<TeamOrchestrator>,
    pub(crate) skill_registry: Arc<dyn SkillRegistry>,
    pub(crate) workspace_manager: Arc<WorkspaceManager>,
    pub(crate) skill_loader: Arc<SkillLoader>,
    pub(crate) llm_client: Arc<dyn LlmClient>,
    pub(crate) plugin_manager: Arc<PluginManager>,
    pub(crate) jcode_caller: Arc<dyn JcodeToolCaller>,
}

impl Clone for MySisyphusBridge {
    fn clone(&self) -> Self {
        Self {
            kernel: self.kernel.clone(),
            event_bus: self.event_bus.clone(),
            consensus: self.consensus.clone(),
            state_store: self.state_store.clone(),
            default_fuel: self.default_fuel,
            agent_registry: self.agent_registry.clone(),
            tool_registry: self.tool_registry.clone(),
            team_orchestrator: self.team_orchestrator.clone(),
            skill_registry: self.skill_registry.clone(),
            workspace_manager: self.workspace_manager.clone(),
            skill_loader: self.skill_loader.clone(),
            llm_client: self.llm_client.clone(),
            plugin_manager: self.plugin_manager.clone(),
            jcode_caller: self.jcode_caller.clone(),
        }
    }
}

impl MySisyphusBridge {
    pub async fn new(shared_config: Arc<RwLock<Config>>, llm_client_override: Option<Arc<dyn LlmClient>>) -> Result<Self, sisyphus_types::TypesError> {
        let (cache_path, l1_cap, max_tasks, wasm_tools_dir, max_mem_pages, stdio_servers, http_servers, default_fuel, embedding_provider, embedding_model, embedding_endpoint) = {
            let config = shared_config.read().map_err(|e| sisyphus_types::TypesError::ToolExecutionError { tool_id: "init".into(), details: format!("Config lock poisoned: {e}") })?;
            (
                config.cache.sqlite_path.clone(),
                config.cache.l1_capacity,
                config.executor.max_concurrent_tasks,
                config.sandbox.wasm_tools_dir.clone(),
                config.sandbox.max_memory_pages,
                config.mcp.stdio_servers.clone(),
                config.mcp.http_servers.clone(),
                config.executor.default_fuel,
                config.llm.embedding.provider.clone(),
                config.llm.embedding.model.clone(),
                config.llm.embedding.endpoint.clone(),
            )
        };
        let store = Arc::new(
            SisyphusStore::new(&cache_path).await
                .map_err(|e| sisyphus_types::TypesError::StateStoreError(format!("Failed to init SisyphusStore: {e}")))?,
        );
        let state_store = store.state.clone();

        let sim_iterations = max_tasks.max(1) * 64;
        let embedding_client: Arc<dyn sisyphus_tools_llm::EmbeddingClient> = {
            if embedding_provider == "openai" {
                let config = shared_config.read().map_err(|e| sisyphus_types::TypesError::ToolExecutionError { tool_id: "init".into(), details: format!("Config lock poisoned: {e}") })?;
                Arc::new(sisyphus_tools_llm::OpenAiEmbeddingClient::new(
                    config.llm.api_key.clone(),
                    embedding_endpoint.clone().unwrap_or_else(|| "https://api.openai.com/v1".into()),
                    embedding_model,
                ).map_err(|e| sisyphus_types::TypesError::ConfigReadError { path: "embedding_config".into(), source: std::io::Error::new(std::io::ErrorKind::Other, e) })?)
            } else {
                Arc::new(sisyphus_tools_llm::DummyEmbeddingClient)
            }
        };
        let experience_bank = Arc::new(sisyphus_mem::experience_bank::VectorExperienceBank::new(state_store.clone(), embedding_client.clone()));
        let simulator = Arc::new(sisyphus_intel::MonteCarloSimulator::with_experience_bank(
            sim_iterations,
            experience_bank.clone(),
        ));


        // Primary executor: WASM (backward-compatible default)
        let wasm_executor = Arc::new(
            WasmToolExecutor::with_config(
                Some(PathBuf::from(&wasm_tools_dir)),
                Some(max_mem_pages),
            )
            .map_err(|e| sisyphus_types::TypesError::ToolExecutionError { tool_id: "init".into(), details: format!("Failed to create WasmToolExecutor: {e}") })?,
        );

        // Initialize PluginManager with hot-reload support
        let wasm_executor_inner = wasm_executor.as_ref().clone();
        let plugin_manager = PluginManager::new(
            wasm_executor_inner,
        );

        // Secondary executor: built-in tools
        let mut registry = HookRegistry::new();
        let loader = SkillLoader::new()
            .with_user_dir()
            .with_project_dir(std::path::Path::new("."));
        let skills = loader.load_all();
        for skill in skills {
            let hooks = hooks_from_skill(
                &skill.name,
                &skill.triggers,
                &skill.instructions,
            );
            registry.register_all(hooks);
        }

        // Create shared DynamicLlmClient for all LLM-dependent components
        let dyn_llm_client = match llm_client_override {
            Some(client) => client,
            None => Arc::new(DynamicLlmClient::new(shared_config.clone())),
        };
        let llm_client_for_bridge = dyn_llm_client.clone();

        let builtin_executor: Arc<dyn ToolExecutor> = Arc::new(BuiltinToolExecutor {
            registry,
            llm_client: {
                let c: Arc<dyn LlmClient> = dyn_llm_client.clone();
                c
            },
            feedback_store: Arc::new(FeedbackStore::new(
                ".omo/feedback",
            )),
            bridge_client: None,
        });

        // Build router: WASM = default (backward compat), builtin = "builtin:" prefix
        let router = Arc::new(ExecutorRouter::new(wasm_executor.clone()));
        router.register("builtin", builtin_executor);

        // Register Jcode high-order agent via privileged RPC channel
        let jcode_executor = Arc::new(JcodeToolExecutor::new());
        router.register("jcode", jcode_executor.clone());



        // ── Roadmap Storage Init ──────────────────────────────────────────
        let sql_store = store.roadmap.clone();
        let proposal_manager = Arc::new(sisyphus_roadmap::proposal_manager::ProposalManager::new(state_store.clone()));

        let proposal_manager = Arc::new(sisyphus_roadmap::proposal_manager::ProposalManager::new(state_store.clone()));

        let strategic_executor = Arc::new(StrategicExecutor::new(
            state_store.clone(),
            dyn_llm_client.clone(),
            cache_path.clone().into(),
            proposal_manager.clone(),
        ).await?);


        router.register("strategic", strategic_executor.clone());

        // ── Tool Registry & Custom Tools ──
        let tool_registry = Arc::new(ToolRegistry::new());

        // Load custom tools (.opencode/tools/*.toml)
        let custom_loader = CustomToolLoader::new(".opencode/tools");
        let custom_defs = custom_loader.load_and_register(&tool_registry).await.unwrap_or_default();

        // Create CustomToolExecutor for these tools
        let custom_executor = Arc::new(
            sisyphus_sandbox::CustomToolExecutor::new(custom_defs)
                .map_err(|e| sisyphus_types::TypesError::ToolExecutionError { tool_id: "init".into(), details: format!("Failed to create CustomToolExecutor: {e}") })?

        );
        router.register("tools", custom_executor);

        // Collect MCP tool info for agent registration
        let mut mcp_servers: Vec<(String, String, Vec<McpTool>)> = Vec::new();

        // Optional MCP executors
        for (prefix, command_line) in &stdio_servers {
            let parts: Vec<&str> = command_line.split_whitespace().collect();
            if parts.is_empty() {
                warn!("Empty MCP stdio command for prefix '{prefix}', skipping");
                continue;
            }
            let cmd = parts[0];
            let args: Vec<&str> = parts[1..].to_vec();
            match MCPToolExecutor::from_stdio(cmd, &args).await {
                Ok(mcp) => {
                    let tools = mcp.list_tools().await.unwrap_or_default();
                    info!(
                        "Connected MCP server '{prefix}' via stdio: {command_line} ({} tools)",
                        tools.len()
                    );
                    mcp_servers.push((prefix.clone(), command_line.clone(), tools));
                    router.register(prefix, Arc::new(mcp));
                }
                Err(e) => {
                    warn!("Failed to start MCP server for prefix '{prefix}': {e}");
                }
            }
        }
        for (prefix, url) in &http_servers {
            match MCPToolExecutor::from_url(url).await {
                Ok(mcp) => {
                    let tools = mcp.list_tools().await.unwrap_or_default();
                    info!(
                        "Connected MCP server '{prefix}' via HTTP: {url} ({} tools)",
                        tools.len()
                    );
                    mcp_servers.push((prefix.clone(), url.clone(), tools));
                    router.register(prefix, Arc::new(mcp));
                }
                Err(e) => {
                    warn!("Failed to connect MCP server for '{prefix}' at {url}: {e}");
                }
            }
        }

        let tool_executor: Arc<dyn ToolExecutor> = router.clone();
        let standard_reflector = Arc::new(sisyphus_intel::StandardReflector);
        let llm_reflector = Arc::new(sisyphus_intel::LlmReflector::new(dyn_llm_client.clone()));
        let reflector = Arc::new(sisyphus_intel::HybridReflector::new(
            standard_reflector,
            llm_reflector,
        ));
        let topo_evolver = Arc::new(sisyphus_intel::TopologicalSOPEvolver::new());
        let evolver = Arc::new(sisyphus_intel::LlmEvolver::new(
            dyn_llm_client.clone(),
            topo_evolver,
            tool_executor.clone(),
        ));
        let intel = Arc::new(sisyphus_intel::SisyphusIntel::new(simulator.clone(), reflector, evolver));

        let meta_learner = Arc::new(parking_lot::Mutex::new(sisyphus_tools_llm_tools::meta_learner::MetaLearner::new(".omo/meta")));
        let meta_synthesizer = Arc::new(MetaLearningSynthesizer::new(
            Arc::new(sisyphus_intel::GoalDrivenSynthesizer::new(
                tool_executor.clone(),
            )),
            meta_learner.clone(),
        ));
        let synthesizer = Arc::new(sisyphus_intel::StrategicSopSynthesizer::new(
            meta_synthesizer,
            simulator.clone(),
        ));
        
        let consensus = Arc::new(ClusterConsensus::new(
            "node-1".to_string(), 
            vec![], // In production, this would be loaded from config
        ));



        // ── Vector Memory Store Init ──────────────────────────────────────────
        let vector_store = Arc::new(sisyphus_mem::VectorMemoryStore::new(store.pool()).await
            .map_err(|e| sisyphus_types::TypesError::StateStoreError(format!("Failed to init VectorMemoryStore: {e}")))?);

        let embedding_client = Arc::new(sisyphus_tools_llm::DummyEmbeddingClient);

        // ── SOP Structured Store Init & Migration ──────────────────────────────
        let sop_store = store.sop.clone();
        
        match sop_store.migrate_from_mfs(&state_store).await {
            Ok(count) => info!(count, "Successfully migrated {} SOPs to structured storage", count),
            Err(e) => warn!(error = %e, "SOP migration failed (might be already migrated"),
        }

        let workspace_manager = Arc::new(sisyphus_tools_agent::workspace_manager::WorkspaceManager::new(state_store.clone()));
        workspace_manager.init().await
            .map_err(|e| sisyphus_types::TypesError::StateStoreError(format!("Failed to init WorkspaceManager: {e}")))?;
        let kernel = Arc::new(SisyphusKernel::new(
            state_store.clone(),
            sop_store.clone(),
            vector_store.clone(),
            tool_executor,
            intel,
            dyn_llm_client.clone(),
            dyn_llm_client.clone(),
            synthesizer,
            meta_learner,
            consensus.clone(),
            sql_store.clone(),
            embedding_client, workspace_manager.clone(), workspace_manager.clone(),
        ));

        strategic_executor.set_kernel(Arc::downgrade(&kernel));

        // Create and populate the agent registry
        let agent_registry = Arc::new(AgentRegistry::new(dyn_llm_client));
        agent_registry.register_builtin();

        // Populate tool registry from MCP servers
        for (prefix, _label, tools) in &mcp_servers {
            for tool in tools {
                let tool_id = format!("{}:{}", prefix, tool.name);
                tool_registry.register(
                    &tool_id,
                    ToolInfo {
                        name: tool.name.clone(),
                        description: tool.description.clone().unwrap_or_default(),
                        parameters: tool.input_schema.clone(),
                        source: ToolSource::Mcp {
                            server_prefix: prefix.clone(),
                        },
                    },
                );
            }
        }
        // Register built-in and agent tools from BuiltinToolExecutor
        register_builtin_tools(&tool_registry);
        // Register strategic tools from StrategicExecutor
        register_strategic_tools(&tool_registry);

        info!(
            "Tool registry initialized with {} tools",
            tool_registry.len()
        );

        // Register MCP server agents with their tool prefixes
        for (prefix, label, tools) in &mcp_servers {
            agent_registry.register_mcp_server(prefix, label, tools.len());
        }
        info!(
            "Agent registry initialized with {} agents",
            agent_registry.list().len()
        );

        let team_llm: Option<Arc<dyn LlmClient>> = None; // planner_llm is optional
        let team_orchestrator = TeamOrchestrator::new(
            agent_registry.clone(),
            kernel.execution_manager.tool_router.clone(),
            team_llm,
        );

        // ── Roadmap Migration ──────────────────────────────────────────────
        let roadmap_manager = sisyphus_roadmap::RoadmapManager::new(state_store.clone(), sql_store.clone(), proposal_manager.clone());
        match roadmap_manager.migrate_to_sql(&sql_store).await {
            Ok(count) => info!(count, "Successfully migrated {} projects to structured storage", count),
            Err(e) => warn!(error = %e, "Roadmap migration failed (might be already migrated)"),
        }

        // ── Skill Registry Init ──────────────────────────────────────────
        let skill_store = Arc::new(SkillStore::new(store.pool()).await
            .map_err(|e| sisyphus_types::TypesError::StateStoreError(format!("Failed to init SkillStore: {e}")))?);
        
        let registry_url = {
            let config = shared_config.read().map_err(|e| sisyphus_types::TypesError::ToolExecutionError { tool_id: "init".into(), details: format!("Config lock poisoned: {e}") })?;
            config.registry.registry_url.clone()
        };
        
        let skill_registry = Arc::new(RemoteSkillRegistry::new(
            registry_url,
            skill_store.clone(),
            router.clone(),
        ));
        
        // ── Workspace Management Init ──────────────────────────────────────
        
        let bridge = MySisyphusBridge {
            kernel,
            state_store,
            default_fuel,
            agent_registry,
            tool_registry,
            team_orchestrator: Arc::new(team_orchestrator),
            skill_registry,
            workspace_manager,
            skill_loader: Arc::new(loader),
            llm_client: llm_client_for_bridge,
            consensus,
            event_bus: tokio::sync::broadcast::channel(1024).0,
            plugin_manager,
            jcode_caller: Arc::new(DebugSocketToolCaller::new(
                shared_config.read().map_err(|_| sisyphus_types::TypesError::ToolExecutionError { tool_id: "init".into(), details: "Config lock poisoned".into() })?.jcode.socket_path.clone()
            )),
        };

        let bridge_arc = Arc::new(bridge);
        jcode_executor.set_caller(bridge_arc.clone());

        Ok(bridge_arc.as_ref().clone())
    }

}

#[async_trait]
impl JcodeToolCaller for MySisyphusBridge {
    async fn call_tool(&self, tool_id: &str, args: Vec<u8>) -> Result<Vec<u8>, String> {
        tracing::info!(tool_id = %tool_id, "Executing jcode tool via debug socket");
        self.jcode_caller.call_tool(tool_id, args).await
    }
}

// ── Accessors for HTTP API ──────────────────────────────────────────────
impl MySisyphusBridge {
    pub fn kernel(&self) -> &Arc<SisyphusKernel> {
        &self.kernel
    }
    pub(crate) fn tool_registry(&self) -> &Arc<ToolRegistry> {
        &self.tool_registry
    }
    pub(crate) fn agent_registry(&self) -> &Arc<AgentRegistry> {
        &self.agent_registry
    }
    pub(crate) fn skill_loader(&self) -> &Arc<SkillLoader> {
        &self.skill_loader
    }
    pub(crate) fn default_fuel(&self) -> u64 {
        self.default_fuel
    }

    pub(crate) async fn get_current_project_id(&self) -> Option<String> {
        self.workspace_manager.get_current().await.map(|ws| ws.project_id)
    }

    pub(crate) fn publish_event(&self, event: sisyphus_bridge_api::BridgeEvent) {
        let _ = self.event_bus.send(event);
    }
}

