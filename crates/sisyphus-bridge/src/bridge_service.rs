use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use sisyphus_core::{Consensus, SisyphusKernel};
use sisyphus_mem::MfsStore;

use sisyphus_sandbox::{PluginManager, WasmToolExecutor};
use sisyphus_tools::LlmClient;
use sisyphus_tools::{
    AgentRegistry, BuiltinToolExecutor, MCPToolExecutor, SkillLoader, TeamOrchestrator,
    ToolRegistry,
};
use sisyphus_types::{Config, ExecutorRouter, ToolExecutor};
use tracing::{info, warn};

use crate::consensus::NoOpConsensus;
use crate::strategic_executor::StrategicExecutor;
use crate::tools::{register_builtin_tools, register_strategic_tools};

pub struct MySisyphusBridge {
    kernel: Arc<SisyphusKernel>,
    consensus: Arc<dyn Consensus>,
    state_store: Arc<dyn sisyphus_types::StateStore>,
    default_fuel: u64,
    agent_registry: Arc<AgentRegistry>,
    tool_registry: Arc<ToolRegistry>,
    team_orchestrator: Arc<TeamOrchestrator>,
    skill_loader: Arc<SkillLoader>,
    llm_client: Arc<sisyphus_tools::DynamicLlmClient>,
    plugin_manager: Arc<PluginManager>,
}

impl Clone for MySisyphusBridge {
    fn clone(&self) -> Self {
        Self {
            kernel: self.kernel.clone(),
            consensus: self.consensus.clone(),
            state_store: self.state_store.clone(),
            default_fuel: self.default_fuel,
            agent_registry: self.agent_registry.clone(),
            tool_registry: self.tool_registry.clone(),
            team_orchestrator: self.team_orchestrator.clone(),
            skill_loader: self.skill_loader.clone(),
            llm_client: self.llm_client.clone(),
            plugin_manager: self.plugin_manager.clone(),
        }
    }
}

impl MySisyphusBridge {
    pub async fn new(shared_config: Arc<RwLock<Config>>) -> Result<Self, sisyphus_types::TypesError> {
        let config = shared_config.read().map_err(|e| sisyphus_types::TypesError::ToolExecutionError { tool_id: "init".into(), details: format!("Config lock poisoned: {e}") })?;
        let state_store = Arc::new(
            MfsStore::with_capacity(
                &config.cache.sqlite_path,
                NonZeroUsize::new(config.cache.l1_capacity).ok_or_else(|| sisyphus_types::TypesError::ToolExecutionError { tool_id: "init".into(), details: format!("l1_capacity must be > 0, got {}", config.cache.l1_capacity) })?,
            )
            .await
            .map_err(|e| sisyphus_types::TypesError::StateStoreError(format!("Failed to init MFS Store: {e}")))?,
        );

        let sim_iterations = config.executor.max_concurrent_tasks.max(1) * 64;
        let simulator = Arc::new(sisyphus_intel::MonteCarloSimulator::with_state_store(
            sim_iterations,
            state_store.clone(),
        ));

        // Primary executor: WASM (backward-compatible default)
        let wasm_executor = Arc::new(
            WasmToolExecutor::with_config(
                Some(PathBuf::from(&config.sandbox.wasm_tools_dir)),
                Some(config.sandbox.max_memory_pages),
            )
            .map_err(|e| sisyphus_types::TypesError::ToolExecutionError { tool_id: "init".into(), details: format!("Failed to create WasmToolExecutor: {e}") })?,
        );

        // Initialize PluginManager with hot-reload support
        let wasm_executor_inner = wasm_executor.as_ref().clone();
        let plugin_manager = PluginManager::new(
            wasm_executor_inner,
        );

        // Secondary executor: built-in tools
        let mut registry = sisyphus_tools::HookRegistry::new();
        let loader = SkillLoader::new()
            .with_user_dir()
            .with_project_dir(std::path::Path::new("."));
        let skills = loader.load_all();
        for skill in skills {
            let hooks = sisyphus_tools::hook::hooks_from_skill(
                &skill.name,
                &skill.triggers,
                &skill.instructions,
            );
            registry.register_all(hooks);
        }

        // Create shared DynamicLlmClient for all LLM-dependent components
        let dyn_llm_client = Arc::new(sisyphus_tools::DynamicLlmClient::new(shared_config.clone()));
        let llm_client_for_bridge = dyn_llm_client.clone();

        let builtin_executor: Arc<dyn ToolExecutor> = Arc::new(BuiltinToolExecutor {
            registry,
            llm_client: {
                let c: Arc<dyn LlmClient> = dyn_llm_client.clone();
                c
            },
            feedback_store: Arc::new(sisyphus_tools::feedback::FeedbackStore::new(
                ".omo/feedback",
            )),
            bridge_client: None,
        });

        // Build router: WASM = default (backward compat), builtin = "builtin:" prefix
        let mut router = ExecutorRouter::new(wasm_executor.clone());
        router.register("builtin", builtin_executor);

        let strategic_executor = Arc::new(StrategicExecutor::new(
            state_store.clone(),
            dyn_llm_client.clone(),
        ));
        router.register("strategic", strategic_executor.clone());

        // ── Tool Registry & Custom Tools ──
        let tool_registry = Arc::new(ToolRegistry::new());

        // Load custom tools (.opencode/tools/*.toml)
        let custom_loader = sisyphus_tools::custom_tool::CustomToolLoader::new(".opencode/tools");
        let custom_defs = custom_loader.load_and_register(&tool_registry).await.unwrap_or_default();

        // Create CustomToolExecutor for these tools
        let custom_executor = Arc::new(
            sisyphus_sandbox::CustomToolExecutor::new(custom_defs)
                .expect("Failed to create CustomToolExecutor")
        );
        router.register("tools", custom_executor);

        // Collect MCP tool info for agent registration
        let mut mcp_servers: Vec<(String, String, Vec<sisyphus_tools::McpTool>)> = Vec::new();

        // Optional MCP executors
        for (prefix, command_line) in &config.mcp.stdio_servers {
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
        for (prefix, url) in &config.mcp.http_servers {
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

        let tool_executor: Arc<dyn ToolExecutor> = Arc::new(router);
        let reflector = Arc::new(sisyphus_intel::StandardReflector);
        let evolver = Arc::new(sisyphus_intel::StandardEvolver::new());
        let intel = Arc::new(sisyphus_intel::SisyphusIntel::new(simulator, reflector, evolver));

        let synthesizer = Arc::new(sisyphus_intel::GoalDrivenSynthesizer::new(
            tool_executor.clone(),
        ));

        let kernel = Arc::new(SisyphusKernel::new(
            state_store.clone(),
            tool_executor,
            intel,
            dyn_llm_client.clone(),
            synthesizer,
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
                    sisyphus_tools::ToolInfo {
                        name: tool.name.clone(),
                        description: tool.description.clone().unwrap_or_default(),
                        parameters: tool.input_schema.clone(),
                        source: sisyphus_tools::ToolSource::Mcp {
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
            kernel.tool_executor.clone(),
            team_llm,
        );

        Ok(Self {
            kernel,
            state_store,
            default_fuel: config.executor.default_fuel,
            agent_registry,
            tool_registry,
            team_orchestrator: Arc::new(team_orchestrator),
            skill_loader: Arc::new(loader),
            llm_client: llm_client_for_bridge,
            consensus: Arc::new(NoOpConsensus),
            plugin_manager,
        })
    }

}

// ── Accessors for HTTP API ──────────────────────────────────────────────
impl MySisyphusBridge {
    pub(crate) fn kernel(&self) -> &Arc<SisyphusKernel> {
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
}

#[path = "grpc_handlers.rs"]
pub(crate) mod grpc_handlers;
