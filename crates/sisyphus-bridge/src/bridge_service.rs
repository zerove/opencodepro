//! REST API for Sisyphus Bridge (axum HTTP layer alongside gRPC).
use crate::jcode_caller::DebugSocketToolCaller;
use async_trait::async_trait;
use std::collections::HashMap;
#[cfg(feature = "sandbox")]
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use sisyphus_core::{Consensus, SisyphusKernel};
use sisyphus_intel::ToolGap;
use sisyphus_mem::SisyphusStore;

use sisyphus_mem::skill_store::SkillStore;

#[cfg(feature = "sandbox")]
use sisyphus_sandbox::{PluginManager, WasmToolExecutor};
use sisyphus_tools_agent::agent_registry::AgentRegistry;
use sisyphus_tools_agent::jcode_executor::JcodeToolCaller;
use sisyphus_tools_agent::jcode_executor::JcodeToolExecutor;
use sisyphus_tools_agent::skill_registry::{RemoteSkillRegistry, SkillRegistry};
use sisyphus_tools_agent::team::TeamOrchestrator;
use sisyphus_tools_agent::workspace_manager::WorkspaceManager;
use sisyphus_tools_core::feedback::FeedbackStore;
use sisyphus_tools_core::hook::{hooks_from_skill, HookRegistry};
use sisyphus_tools_executor::executor::BuiltinToolExecutor;
use sisyphus_tools_executor::executor_mcp::MCPToolExecutor;
use sisyphus_tools_llm::{DynamicLlmClient, LlmClient};
use sisyphus_tools_mcp::custom_tool::CustomToolLoader;
use sisyphus_tools_mcp::mcp::McpTool;
use sisyphus_tools_mcp::tool_registry::{ToolInfo, ToolRegistry, ToolSource};
use sisyphus_tools_session::skill::SkillLoader;
use sisyphus_types::tools::ToolSpec;
#[cfg(not(feature = "sandbox"))]
use sisyphus_types::{CacheInvalidatable, PluginDescriptor, ToolMetadata, TypesError};
use sisyphus_types::{
    Config, ExecutorRouter, MetaLearningSample, PluginManagerTrait, ToolExecutor,
};
use tracing::{info, warn};

use crate::consensus::ClusterConsensus;
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
    pub(crate) skill_store: Arc<sisyphus_mem::skill_store::SkillStore>,
    pub(crate) skill_market: Arc<sisyphus_mem::skill_market::SkillMarket>,
    pub(crate) workspace_manager: Arc<WorkspaceManager>,
    pub(crate) experience_bank: Arc<sisyphus_mem::experience_bank::VectorExperienceBank>,
    pub(crate) skill_loader: Arc<SkillLoader>,
    pub(crate) llm_client: Arc<dyn LlmClient>,
    pub(crate) plugin_manager: Arc<dyn PluginManagerTrait>,
    pub(crate) jcode_caller: Arc<dyn JcodeToolCaller>,
    pub(crate) chaos_runner: Arc<sisyphus_core::ChaosRunner>,
    /// Whether chaos fault injection is enabled at startup (from ChaosConfig).
    pub(crate) chaos_enabled: bool,
    /// Default fault probability for bridge-level chaos injection.
    pub(crate) chaos_fault_probability: f64,
    /// Total count of CHAOS_ERROR faults injected (direction 4: auto-fix pipeline).
    pub(crate) chaos_fault_counter: Arc<AtomicU64>,
    /// Per-fault-type counter: maps fault type string → occurrence count.
    pub(crate) chaos_fault_type_counters: Arc<parking_lot::Mutex<HashMap<String, u64>>>,
    /// Set to true when auto-fix is triggered (for testing).
    pub(crate) chaos_auto_fix_triggered: Arc<AtomicBool>,
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
            skill_store: self.skill_store.clone(),
            skill_market: self.skill_market.clone(),
            workspace_manager: self.workspace_manager.clone(),
            experience_bank: self.experience_bank.clone(),
            skill_loader: self.skill_loader.clone(),
            llm_client: self.llm_client.clone(),
            plugin_manager: self.plugin_manager.clone(),
            jcode_caller: self.jcode_caller.clone(),
            chaos_runner: self.chaos_runner.clone(),
            chaos_enabled: self.chaos_enabled,
            chaos_fault_probability: self.chaos_fault_probability,
            chaos_fault_counter: self.chaos_fault_counter.clone(),
            chaos_fault_type_counters: self.chaos_fault_type_counters.clone(),
            chaos_auto_fix_triggered: self.chaos_auto_fix_triggered.clone(),
        }
    }
}

impl MySisyphusBridge {
    pub async fn new(
        shared_config: Arc<RwLock<Config>>,
        llm_client_override: Option<Arc<dyn LlmClient>>,
    ) -> Result<Self, sisyphus_types::TypesError> {
        let config_snapshot = shared_config
            .read()
            .map_err(|e| sisyphus_types::TypesError::ToolExecutionError {
                tool_id: "init".into(),
                details: format!("Config lock poisoned: {e}"),
            })?
            .clone();
        let (
            cache_config,
            max_tasks,
            _wasm_tools_dir,
            _max_mem_pages,
            stdio_servers,
            http_servers,
            default_fuel,
            embedding_provider,
            embedding_model,
            embedding_endpoint,
            _use_sandboxd,
            _sandboxd_path,
        ) = (
            config_snapshot.cache.clone(),
            config_snapshot.executor.max_concurrent_tasks,
            config_snapshot.sandbox.wasm_tools_dir.clone(),
            config_snapshot.sandbox.max_memory_pages,
            config_snapshot.mcp.stdio_servers.clone(),
            config_snapshot.mcp.http_servers.clone(),
            config_snapshot.executor.default_fuel,
            config_snapshot.llm.embedding.provider.clone(),
            config_snapshot.llm.embedding.model.clone(),
            config_snapshot.llm.embedding.endpoint.clone(),
            config_snapshot.sandbox.use_sandboxd,
            config_snapshot.sandbox.sandboxd_path.clone(),
        );

        // Destructure sandbox-specific vars (underscore-prefixed to suppress unused warnings when sandbox is disabled)
        #[cfg(feature = "sandbox")]
        let (wasm_tools_dir, max_mem_pages, use_sandboxd, sandboxd_path) = (
            _wasm_tools_dir,
            _max_mem_pages,
            _use_sandboxd,
            _sandboxd_path,
        );

        let store = Arc::new(
            SisyphusStore::new(&cache_config.database_url, &config_snapshot)
                .await
                .map_err(|e| {
                    sisyphus_types::TypesError::StateStoreError(format!(
                        "Failed to init SisyphusStore: {e}"
                    ))
                })?,
        );
        let state_store = store.state.clone();

        let vector_store = Arc::new(
            sisyphus_mem::VectorMemoryStore::new(store.pool())
                .await
                .map_err(|e| {
                    sisyphus_types::TypesError::StateStoreError(format!(
                        "Failed to init VectorMemoryStore: {e}"
                    ))
                })?,
        );

        let sim_iterations = max_tasks.max(1) * 64;
        let embedding_client: Arc<dyn sisyphus_tools_llm::EmbeddingClient> = {
            if embedding_provider == "openai" {
                let config = shared_config.read().map_err(|e| {
                    sisyphus_types::TypesError::ToolExecutionError {
                        tool_id: "init".into(),
                        details: format!("Config lock poisoned: {e}"),
                    }
                })?;
                Arc::new(
                    sisyphus_tools_llm::OpenAiEmbeddingClient::new(
                        config.llm.api_key.clone(),
                        embedding_endpoint
                            .clone()
                            .unwrap_or_else(|| "https://api.openai.com/v1".into()),
                        embedding_model,
                    )
                    .map_err(|e| {
                        sisyphus_types::TypesError::ConfigReadError {
                            path: "embedding_config".into(),
                            source: std::io::Error::other(e),
                        }
                    })?,
                )
            } else {
                Arc::new(sisyphus_tools_llm::DummyEmbeddingClient)
            }
        };
        let experience_bank = Arc::new(
            sisyphus_mem::experience_bank::VectorExperienceBank::new(
                state_store.clone(),
                vector_store.clone(),
                embedding_client.clone(),
            )
            .await
            .map_err(|e| {
                sisyphus_types::TypesError::StateStoreError(format!(
                    "Failed to init VectorExperienceBank: {e}"
                ))
            })?,
        );
        let simulator = Arc::new(sisyphus_intel::MonteCarloSimulator::with_experience_bank(
            sim_iterations,
            experience_bank.clone(),
        ));

        // ── Primary executor ──────────────────────────────────
        // WASM executor (requires wasmtime); falls back to noop when sandbox is disabled.
        // When use_sandboxd is true, spawns sandboxd as a subprocess instead of embedding wasmtime.
        // PluginManager is initialized alongside the wasm executor.
        #[allow(unused_mut)]
        let (wasm_executor, plugin_manager): (
            Arc<dyn ToolExecutor>,
            Arc<dyn PluginManagerTrait>,
        ) = {
            #[cfg(feature = "sandbox")]
            {
                use crate::sandboxd_client::SandboxdClient;
                use sisyphus_types::types::PluginDescriptor;

                // Minimal empty PluginManager for sandboxd mode (plugin lifecycle
                // is managed by the daemon process, not the bridge).
                struct SandboxdPluginManager;
                #[async_trait]
                impl PluginManagerTrait for SandboxdPluginManager {
                    async fn register_plugin(&self, _d: PluginDescriptor) -> Result<(), String> {
                        Err("Plugins handled by sandboxd process, not bridge".into())
                    }
                    fn unload_plugin(&self, _n: &str) -> Result<(), String> {
                        Err("Plugins handled by sandboxd process, not bridge".into())
                    }
                    fn list_plugins(&self) -> Vec<PluginDescriptor> {
                        Vec::new()
                    }
                }

                let (exec, pm): (Arc<dyn ToolExecutor>, Arc<dyn PluginManagerTrait>) =
                    if use_sandboxd {
                        let sandboxd_bin =
                            sandboxd_path.unwrap_or_else(|| "sisyphus-sandboxd".into());
                        info!(
                            "Using sandboxd at {sandboxd_bin} (wasm_tools_dir={wasm_tools_dir}, max_mem_pages={max_mem_pages})",
                        );
                        let client =
                            SandboxdClient::spawn(&sandboxd_bin, &wasm_tools_dir, max_mem_pages)
                                .await
                                .map_err(|e| sisyphus_types::TypesError::ToolExecutionError {
                                    tool_id: "sandboxd".into(),
                                    details: format!("Failed to spawn sandboxd: {e}"),
                                })?;
                        let noop_pm: Arc<dyn PluginManagerTrait> = Arc::new(SandboxdPluginManager);
                        (Arc::new(client) as Arc<dyn ToolExecutor>, noop_pm)
                    } else {
                        let exec = WasmToolExecutor::with_config(
                            Some(PathBuf::from(&wasm_tools_dir)),
                            Some(max_mem_pages),
                        )
                        .map_err(|e| {
                            sisyphus_types::TypesError::ToolExecutionError {
                                tool_id: "init".into(),
                                details: format!("Failed to create WasmToolExecutor: {e}"),
                            }
                        })?;
                        let inner = exec.clone();
                        let pm = PluginManager::new(inner);
                        (
                            Arc::new(exec) as Arc<dyn ToolExecutor>,
                            pm as Arc<dyn PluginManagerTrait>,
                        )
                    };
                (exec, pm)
            }
            #[cfg(not(feature = "sandbox"))]
            {
                warn!("Sandbox disabled: WASM execution & plugin management unavailable");
                let noop_exec: Arc<dyn ToolExecutor> = Arc::new(NoopToolExecutor);
                let noop_pm: Arc<dyn PluginManagerTrait> = Arc::new(NoopPluginManager);
                (noop_exec, noop_pm)
            }
        };

        // Secondary executor: built-in tools
        let mut registry = HookRegistry::new();
        let loader = SkillLoader::new()
            .with_user_dir()
            .with_project_dir(std::path::Path::new("."));
        let skills = loader.load_all();
        for skill in skills {
            let hooks = hooks_from_skill(&skill.name, &skill.triggers, &skill.instructions);
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
            feedback_store: Arc::new(FeedbackStore::new(".omo/feedback")),
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
        let _proposal_manager = Arc::new(sisyphus_roadmap::proposal_manager::ProposalManager::new(
            state_store.clone(),
        ));

        let proposal_manager = Arc::new(sisyphus_roadmap::proposal_manager::ProposalManager::new(
            state_store.clone(),
        ));

        let strategic_executor = Arc::new(
            StrategicExecutor::new(
                state_store.clone(),
                dyn_llm_client.clone(),
                cache_config.database_url.clone().into(),
                proposal_manager.clone(),
            )
            .await?,
        );

        router.register("strategic", strategic_executor.clone());

        // ── Tool Registry & Custom Tools ──
        let tool_registry = Arc::new(ToolRegistry::new());

        // Load custom tools (.opencode/tools/*.toml)
        let custom_loader = CustomToolLoader::new(".opencode/tools");
        let custom_defs = custom_loader
            .load_and_register(&tool_registry)
            .await
            .unwrap_or_default();

        // Create CustomToolExecutor for these tools (requires sandbox)
        #[allow(unused_variables)]
        let custom_executor: Option<Arc<dyn ToolExecutor>> = {
            #[cfg(feature = "sandbox")]
            {
                let exec = Arc::new(
                    sisyphus_sandbox::CustomToolExecutor::new(custom_defs).map_err(|e| {
                        sisyphus_types::TypesError::ToolExecutionError {
                            tool_id: "init".into(),
                            details: format!("Failed to create CustomToolExecutor: {e}"),
                        }
                    })?,
                );
                Some(exec as Arc<dyn ToolExecutor>)
            }
            #[cfg(not(feature = "sandbox"))]
            {
                if !custom_defs.is_empty() {
                    warn!("Sandbox disabled: custom tools from .opencode/tools are not available");
                }
                None
            }
        };
        if let Some(exec) = custom_executor {
            router.register("tools", exec);
        }

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
                    mcp_servers.push((prefix.clone(), url.to_string(), tools));
                    router.register(prefix, Arc::new(mcp));
                }
                Err(e) => {
                    warn!("Failed to connect MCP server for '{prefix}' at {url}: {e}");
                }
            }
        }

        let tool_executor: Arc<dyn ToolExecutor> = router.clone();
        let standard_reflector = Arc::new(sisyphus_intel::StandardReflector);
        // B3：循环内各 LLM 阶段统一使用 cognitive_loop.llm_stage_timeout_secs。
        let stage_timeout =
            std::time::Duration::from_secs(config_snapshot.cognitive_loop.llm_stage_timeout_secs);
        let llm_reflector = Arc::new(sisyphus_intel::LlmReflector::with_timeout(
            dyn_llm_client.clone(),
            stage_timeout,
        ));
        let reflector = Arc::new(sisyphus_intel::HybridReflector::new(
            standard_reflector,
            llm_reflector,
        ));
        let topo_evolver = Arc::new(sisyphus_intel::TopologicalSOPEvolver::new());
        let evolver = Arc::new(sisyphus_intel::LlmEvolver::with_timeout(
            dyn_llm_client.clone(),
            topo_evolver,
            tool_executor.clone(),
            stage_timeout,
        ));
        let llm_logical_simulator = Arc::new(
            sisyphus_intel::simulation::LlmLogicalSimulator::with_timeout(
                dyn_llm_client.clone(),
                stage_timeout,
            ),
        );
        let intel = Arc::new(sisyphus_intel::SisyphusIntel::new(
            simulator.clone(),
            llm_logical_simulator,
            reflector,
            evolver,
        ));

        let meta_learner = Arc::new(parking_lot::Mutex::new(
            sisyphus_tools_llm_tools::meta_learner::MetaLearner::new(".omo/meta"),
        ));
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

        let embedding_client = Arc::new(sisyphus_tools_llm::DummyEmbeddingClient);

        // ── SOP Structured Store Init & Migration ──────────────────────────────
        let sop_store = store.sop.clone();

        match sop_store.migrate_from_mfs(&state_store).await {
            Ok(count) => info!(
                count,
                "Successfully migrated {} SOPs to structured storage", count
            ),
            Err(e) => warn!(error = %e, "SOP migration failed (might be already migrated"),
        }

        let workspace_manager = Arc::new(
            sisyphus_tools_agent::workspace_manager::WorkspaceManager::new(state_store.clone()),
        );
        workspace_manager.init().await.map_err(|e| {
            sisyphus_types::TypesError::StateStoreError(format!(
                "Failed to init WorkspaceManager: {e}"
            ))
        })?;
        let kernel = Arc::new(SisyphusKernel::new(
            state_store.clone(),
            sop_store.clone(),
            vector_store.clone(),
            experience_bank.clone(),
            tool_executor,
            intel,
            dyn_llm_client.clone(),
            dyn_llm_client.clone(),
            synthesizer,
            meta_learner,
            consensus.clone(),
            sql_store.clone(),
            embedding_client,
        ));

        // ── 认知循环配置接线（评审 A4）─────────────────────────────────
        // 此前 SisyphusKernel 内部硬编码 CognitiveLoopConfig::default()，
        // 配置文件/环境变量中的 [cognitive_loop] 与 [cognitive_loop.adaptive_depth]
        // 从不生效。此处把用户配置应用到 StrategicManager。
        kernel
            .strategic_manager
            .apply_cognitive_loop_config(config_snapshot.cognitive_loop.clone());

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
        let roadmap_manager = sisyphus_roadmap::RoadmapManager::new(
            state_store.clone(),
            sql_store.clone(),
            proposal_manager.clone(),
        );
        match roadmap_manager.migrate_to_sql(&sql_store).await {
            Ok(count) => info!(
                count,
                "Successfully migrated {} projects to structured storage", count
            ),
            Err(e) => warn!(error = %e, "Roadmap migration failed (might be already migrated)"),
        }

        // ── Skill Registry Init ──────────────────────────────────────────
        let skill_store = Arc::new(SkillStore::new(store.pool()).await.map_err(|e| {
            sisyphus_types::TypesError::StateStoreError(format!("Failed to init SkillStore: {e}"))
        })?);

        let registry_url = {
            let config = shared_config.read().map_err(|e| {
                sisyphus_types::TypesError::ToolExecutionError {
                    tool_id: "init".into(),
                    details: format!("Config lock poisoned: {e}"),
                }
            })?;
            config.registry.registry_url.clone()
        };

        let skill_registry = Arc::new(RemoteSkillRegistry::new(
            registry_url.clone(),
            skill_store.clone(),
            router.clone(),
        ));

        // Skill market: multi-registry search / publish client.
        let skill_market = Arc::new(sisyphus_mem::skill_market::SkillMarket::new(
            sisyphus_mem::skill_market::SkillMarketConfig {
                registries: vec![registry_url],
                ..sisyphus_mem::skill_market::SkillMarketConfig::default()
            },
        ));

        // ── Workspace Management Init ──────────────────────────────────────

        // ── Chaos Config ─────────────────────────────────────────────────
        let (chaos_enabled, chaos_fault_probability) = {
            let config = shared_config.read().map_err(|e| {
                sisyphus_types::TypesError::ToolExecutionError {
                    tool_id: "init".into(),
                    details: format!("Config lock poisoned: {e}"),
                }
            })?;
            (config.chaos.enabled, config.chaos.default_fault_probability)
        };

        let bridge = MySisyphusBridge {
            kernel,
            state_store,
            default_fuel,
            agent_registry,
            tool_registry,
            team_orchestrator: Arc::new(team_orchestrator),
            skill_registry,
            skill_store,
            skill_market,
            workspace_manager,
            experience_bank,
            skill_loader: Arc::new(loader),
            llm_client: llm_client_for_bridge,
            consensus,
            event_bus: tokio::sync::broadcast::channel(1024).0,
            plugin_manager,
            jcode_caller: Arc::new(DebugSocketToolCaller::new(
                shared_config
                    .read()
                    .map_err(|_| sisyphus_types::TypesError::ToolExecutionError {
                        tool_id: "init".into(),
                        details: "Config lock poisoned".into(),
                    })?
                    .jcode
                    .socket_path
                    .clone(),
            )),
            chaos_runner: Arc::new(sisyphus_core::ChaosRunner::new()),
            chaos_enabled,
            chaos_fault_probability,
            chaos_fault_counter: Arc::new(AtomicU64::new(0)),
            chaos_fault_type_counters: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            chaos_auto_fix_triggered: Arc::new(AtomicBool::new(false)),
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

    #[expect(dead_code)]
    pub(crate) async fn get_current_project_id(&self) -> Option<String> {
        self.workspace_manager
            .get_current()
            .await
            .map(|ws| ws.project_id)
    }

    pub(crate) fn publish_event(&self, event: sisyphus_bridge_api::BridgeEvent) {
        let _ = self.event_bus.send(event);
    }

    // ── Chaos Engineering: bridge-level fault injection ───────────────────

    /// Returns the total number of CHAOS_ERROR faults injected.
    pub fn chaos_fault_count(&self) -> u64 {
        self.chaos_fault_counter.load(Ordering::Relaxed)
    }

    /// Returns true if the auto-fix background task has been spawned.
    pub fn chaos_auto_fix_was_triggered(&self) -> bool {
        self.chaos_auto_fix_triggered.load(Ordering::Relaxed)
    }

    /// Simple pseudo-random f64 in [0.0, 1.0) using a static atomic counter.
    fn chaos_rand_f64() -> f64 {
        use std::hash::{Hash, Hasher};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        COUNTER.fetch_add(1, Ordering::Relaxed).hash(&mut hasher);
        (hasher.finish() as f64) / (u64::MAX as f64)
    }

    /// If chaos is enabled, randomly inject a delay or error with the
    /// configured probability.  Returns `Some(ExecuteToolResponse)` when a
    /// fault is injected, or `None` to continue normal execution.
    ///
    /// ## Direction 4: Chaos → Auto-repair pipeline
    ///
    /// Every injected fault is counted.  When the total exceeds 3 the bridge
    /// automatically spawns a background self-programming repair cycle:
    /// 1. Records the fault as a `MetaLearningSample` in the kernel's meta-learner.
    /// 2. Calls `SelfProgrammingManager::resolve_gap(gap)` with a synthetic
    ///    `ToolGap` that describes the chaos-robustness improvement needed.
    pub(crate) async fn maybe_inject_chaos_fault(
        &self,
    ) -> Option<sisyphus_bridge_api::ExecuteToolResponse> {
        if !self.chaos_enabled {
            return None;
        }

        // Probability gate
        if Self::chaos_rand_f64() >= self.chaos_fault_probability {
            return None;
        }

        // 50/50 split between delay injection and error injection
        if Self::chaos_rand_f64() < 0.5 {
            // Inject an asynchronous delay in range [50, 500] ms
            let delay_ms = 50 + (Self::chaos_rand_f64() * 450.0) as u64;
            tracing::info!(delay_ms, "Chaos: injecting delay");
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            None // delay is transparent — allow execution to proceed
        } else {
            // Inject an artificial fault (return error without executing)
            tracing::info!("Chaos: injecting fault");
            let fault_type = "CHAOS_ERROR".to_string();

            // ── Increment fault counters ─────────────────────────────────
            self.chaos_fault_counter.fetch_add(1, Ordering::Relaxed);
            let total = self.chaos_fault_counter.load(Ordering::Relaxed);
            {
                let mut counters = self.chaos_fault_type_counters.lock();
                *counters.entry(fault_type.clone()).or_insert(0) += 1;
            }

            // ── Auto-fix: spawn background repair when faults exceed 3 ──
            if total > 3 {
                self.chaos_auto_fix_triggered.store(true, Ordering::Relaxed);
                let kernel = self.kernel.clone();
                let enabled = self.chaos_enabled;
                let ft_for_spawn = fault_type.clone();
                tokio::spawn(async move {
                    // 1. Record fault sample to MetaLearner
                    let sample = MetaLearningSample {
                        context: serde_json::json!({
                            "fault_type": ft_for_spawn.clone(),
                            "source": "bridge_maybe_inject_chaos_fault",
                        }),
                        strategy: "chaos_auto_fix".into(),
                        params: serde_json::json!({}),
                        success: false,
                        duration_secs: 0.0,
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                    };
                    {
                        let mut ml = kernel.meta_learner.lock();
                        let _ = ml.record_outcome(sample);
                    }

                    // 2. If chaos is still enabled, try to resolve the gap
                    //    via the self-programming manager.
                    if enabled {
                        let spec = ToolSpec {
                            tool_id: "chaos_auto_repair".into(),
                            required_capability: format!(
                                "Handle recurring chaos faults ({}) by implementing \
                                 retry logic, circuit breaker hardening, and error recovery",
                                ft_for_spawn
                            ),
                            expected_input: serde_json::json!({"error": ft_for_spawn}),
                            expected_output: serde_json::json!({"status": "recovered"}),
                            success_criteria: format!(
                                "System handles {} faults without crashing",
                                ft_for_spawn
                            ),
                            test_command: None,
                            workdir: None,
                        };
                        let gap = ToolGap {
                            gap_id: format!(
                                "chaos_fix_{}",
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs()
                            ),
                            required_capability: spec.required_capability.clone(),
                            failing_node: "bridge_execute_tool".into(),
                            suggested_tool_spec: spec,
                        };
                        let result = kernel.strategic_manager.sp_manager.resolve_gap(gap).await;
                        match result {
                            Ok(()) => tracing::info!("Chaos auto-fix resolved successfully"),
                            Err(e) => tracing::warn!("Chaos auto-fix failed: {e}"),
                        }
                    }
                });
            }

            Some(sisyphus_bridge_api::ExecuteToolResponse {
                status: format!("{}: injected fault", fault_type),
                result_json: String::new(),
            })
        }
    }
}

// ── Noop fallbacks (used when sandbox feature is disabled) ────────────────

/// Fallback executor that returns an error for every execution.
/// Used when the `sandbox` feature is disabled to avoid linking wasmtime.
#[cfg(not(feature = "sandbox"))]
struct NoopToolExecutor;

#[cfg(not(feature = "sandbox"))]
#[async_trait]
impl ToolExecutor for NoopToolExecutor {
    async fn execute(
        &self,
        tool_id: &str,
        _args: Vec<u8>,
        _fuel_limit: u64,
    ) -> Result<Vec<u8>, TypesError> {
        Err(TypesError::ToolExecutionError {
            tool_id: tool_id.to_string(),
            details: format!("Sandbox disabled: cannot execute tool '{tool_id}'"),
        })
    }

    async fn list_tools(&self) -> Result<Vec<ToolMetadata>, TypesError> {
        Ok(Vec::new())
    }

    fn as_cache_invalidatable(&self) -> Option<&dyn CacheInvalidatable> {
        None
    }
}

/// Fallback plugin manager that returns empty results for all operations.
/// Used when the `sandbox` feature is disabled.
#[cfg(not(feature = "sandbox"))]
struct NoopPluginManager;

#[cfg(not(feature = "sandbox"))]
#[async_trait]
impl PluginManagerTrait for NoopPluginManager {
    async fn register_plugin(&self, _descriptor: PluginDescriptor) -> Result<(), String> {
        Err("Sandbox disabled: cannot register plugins".to_string())
    }

    fn unload_plugin(&self, _name: &str) -> Result<(), String> {
        Err("Sandbox disabled: cannot unload plugins".to_string())
    }

    fn list_plugins(&self) -> Vec<PluginDescriptor> {
        Vec::new()
    }
}
