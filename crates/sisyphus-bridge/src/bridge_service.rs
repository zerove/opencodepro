//! REST API for Sisyphus Bridge (axum HTTP layer alongside gRPC).
use crate::jcode_caller::DebugSocketToolCaller;
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
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

/// MCP 服务器启动时采集的 resources/prompts 快照（HTTP API 与目录展示用）。
#[derive(Clone, Debug, serde::Serialize)]
pub(crate) struct McpServerCatalog {
    pub server_prefix: String,
    pub label: String,
    pub resources: Vec<sisyphus_tools_mcp::mcp::McpResource>,
    pub resource_templates: Vec<sisyphus_tools_mcp::mcp::McpResourceTemplate>,
    pub prompts: Vec<sisyphus_tools_mcp::mcp::McpPrompt>,
}

/// My Sisyphus Bridge.
pub struct MySisyphusBridge {
    pub(crate) kernel: Arc<SisyphusKernel>,
    pub(crate) event_bus: tokio::sync::broadcast::Sender<sisyphus_bridge_api::BridgeEvent>,
    pub(crate) consensus: Arc<dyn Consensus>,
    pub(crate) state_store: Arc<dyn sisyphus_types::StateStore>,
    /// Concrete L2-backed store. `state_store` is a trait object, which hides
    /// key-listing; the `/api/state` family needs it.
    pub(crate) concrete_store: Arc<sisyphus_mem::store::MfsStore>,
    /// Semantic index backing `/api/vector/search`.
    pub(crate) vector_store: Arc<sisyphus_mem::VectorMemoryStore>,
    pub(crate) embedding_client: Arc<dyn sisyphus_tools_llm::EmbeddingClient>,
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
    /// 默认构造时持有的具体 DynamicLlmClient，用于执行级 token 作用域
    /// （`DynamicLlmClient::scope_usage`）。override 为 `dyn LlmClient` 时为 `None`，
    /// 此时回退到 best-effort 租约+累计差值采集。
    pub(crate) llm_usage_scope_client: Option<Arc<DynamicLlmClient>>,
    /// LLM 任务级用量互斥计数：独占任务（`begin_llm_task` 返回 true）时可
    /// 对同一任务采集 before/after usage 增量；并发任务返回 false，保持 `None`。
    pub(crate) llm_in_flight: Arc<AtomicUsize>,
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
    /// 联邦策略权重聚合视图（v0.4.0 Phase B，ADR-017）。
    ///
    /// `StrategyWeightsHandler` 持有这份共享视图：本地发布写入、入站合并更新。
    /// 挂载在桥接上而非 `RestApiState`，避免改 `RestApiState` 的 3 处构造点。
    pub(crate) strategy_weights:
        Arc<parking_lot::RwLock<sisyphus_types::strategy_weights::FederatedStrategyWeights>>,
    /// 集群 peer 管理器（`None` = 单节点/集群未启用时未创建运行时）。
    pub(crate) peer_manager: Option<Arc<crate::peer::PeerManager>>,
    /// 集群配置快照（只读，用于状态端点）。
    pub(crate) cluster_config: sisyphus_types::ClusterConfig,
    /// ADR-019：控制面协调器（门禁装配时可用；A=gRPC/内部路径）。
    pub(crate) control_plane_coordinator:
        Option<Arc<crate::peer::control_plane::ControlPlaneCoordinator>>,
    pub(crate) control_plane_state: Option<Arc<crate::peer::control_plane::ControlPlaneState>>,
    pub(crate) control_plane_tls_wired: bool,
    /// MCP 服务器 resources/prompts 快照（启动时采集，按 server prefix 聚合）。
    pub(crate) mcp_catalog: Arc<Vec<McpServerCatalog>>,
    /// MCP 执行器按 prefix 索引（用于按需 resources/read、prompts/get 代理）。
    pub(crate) mcp_executors: Arc<std::collections::HashMap<String, Arc<MCPToolExecutor>>>,
}

impl Clone for MySisyphusBridge {
    fn clone(&self) -> Self {
        Self {
            kernel: self.kernel.clone(),
            event_bus: self.event_bus.clone(),
            consensus: self.consensus.clone(),
            state_store: self.state_store.clone(),
            concrete_store: self.concrete_store.clone(),
            vector_store: self.vector_store.clone(),
            embedding_client: self.embedding_client.clone(),
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
            llm_usage_scope_client: self.llm_usage_scope_client.clone(),
            llm_in_flight: self.llm_in_flight.clone(),
            plugin_manager: self.plugin_manager.clone(),
            jcode_caller: self.jcode_caller.clone(),
            chaos_runner: self.chaos_runner.clone(),
            chaos_enabled: self.chaos_enabled,
            chaos_fault_probability: self.chaos_fault_probability,
            chaos_fault_counter: self.chaos_fault_counter.clone(),
            chaos_fault_type_counters: self.chaos_fault_type_counters.clone(),
            chaos_auto_fix_triggered: self.chaos_auto_fix_triggered.clone(),
            strategy_weights: self.strategy_weights.clone(),
            peer_manager: self.peer_manager.clone(),
            cluster_config: self.cluster_config.clone(),
            control_plane_coordinator: self.control_plane_coordinator.clone(),
            control_plane_state: self.control_plane_state.clone(),
            control_plane_tls_wired: self.control_plane_tls_wired,
            mcp_catalog: self.mcp_catalog.clone(),
            mcp_executors: self.mcp_executors.clone(),
        }
    }
}

/// 集群运行时：由 `main` 在构造桥接之前创建，供内核经验库装饰与后续
/// ClusterService / 经验同步处理器共享同一个 peer 管理器。
#[derive(Clone)]
pub struct ClusterRuntime {
    pub peer_manager: Arc<crate::peer::PeerManager>,
    pub node_id: String,
    /// 集群配置快照（用于 `/api/cluster/status` 可观测，不改变行为）。
    pub cluster_config: sisyphus_types::ClusterConfig,
    /// ADR-019：TLS/mTLS 是否实际装配（`tls_enabled` 且加载/接线成功）。
    pub control_plane_tls_wired: bool,
    /// ADR-019：控制面租约状态（`cluster.control_plane.enabled` 时装配）。
    pub control_plane_state: Option<Arc<crate::peer::control_plane::ControlPlaneState>>,
    /// ADR-019：多数派租约协调器（门禁/装配就绪时可用）。
    pub control_plane_coordinator: Option<Arc<crate::peer::control_plane::ControlPlaneCoordinator>>,
}

impl MySisyphusBridge {
    /// ADR-019：多数派租约协调器（`cluster.control_plane.enabled` 且装配就绪时可用）。
    /// A=gRPC/内部路径使用；未启用/未就绪时返回 `None`。
    pub fn control_plane_coordinator(
        &self,
    ) -> Option<&Arc<crate::peer::control_plane::ControlPlaneCoordinator>> {
        self.control_plane_coordinator.as_ref()
    }

    /// ADR-019：控制面租约状态（门禁装配时可用）。
    pub fn control_plane_state(
        &self,
    ) -> Option<&Arc<crate::peer::control_plane::ControlPlaneState>> {
        self.control_plane_state.as_ref()
    }

    /// ADR-019：控制面 TLS/mTLS 是否实际装配。
    pub fn control_plane_tls_wired(&self) -> bool {
        self.control_plane_tls_wired
    }

    /// New.
    ///
    /// `cluster` 传入集群运行时（peer 管理器 + 本节点 ID）时，内核的经验库
    /// 会被 [`PeerBroadcastExperienceBank`] 装饰，本地新增经验在落库成功后自动
    /// 推送给所有 peer；`None` 表示单节点运行，行为与集群之前完全一致。
    pub async fn new(
        shared_config: Arc<RwLock<Config>>,
        llm_client_override: Option<Arc<dyn LlmClient>>,
        cluster: Option<ClusterRuntime>,
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
            SisyphusStore::new(&cache_config.database_url, &config_snapshot).await.map_err(
                |e| {
                    sisyphus_types::TypesError::StateStoreError(format!(
                        "Failed to init SisyphusStore: {e}"
                    ))
                },
            )?,
        );
        let state_store = store.state.clone();

        let vector_store = Arc::new(
            sisyphus_mem::VectorMemoryStore::new(store.pool()).await.map_err(|e| {
                sisyphus_types::TypesError::StateStoreError(format!(
                    "Failed to init VectorMemoryStore: {e}"
                ))
            })?,
        );

        let sim_iterations = max_tasks.max(1) * 64;
        let allow_dummy_embeddings = config_snapshot.llm.embedding.allow_dummy_embeddings;
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
            } else if allow_dummy_embeddings {
                // 允许使用 DummyEmbeddingClient（测试/开发模式）。
                // 此模式下，嵌入向量为文本哈希的伪向量，无语义区分度，
                // 语义经验检索将退化为噪声。
                tracing::warn!(
                    provider = %embedding_provider,
                    "Embedding provider is not \"openai\"; using DummyEmbeddingClient \
                     (hash-based pseudo-vectors, NO semantic quality). \
                     Semantic experience recall is degraded. \
                     Set [llm.embedding] allow_dummy_embeddings = false and \
                     provider = \"openai\" for production use."
                );
                Arc::new(sisyphus_tools_llm::DummyEmbeddingClient)
            } else {
                // B1 修复：禁止静默降级。缺少有效嵌入provider时，必须在启动时失败，
                // 而不是仅警告后继续使用伪嵌入。
                return Err(sisyphus_types::TypesError::ToolExecutionError {
                    tool_id: "init".into(),
                    details: format!(
                        "Embedding provider '{}' is not 'openai' and allow_dummy_embeddings is false. \
                         Configure [llm.embedding] provider='openai' with a valid API key, \
                         or set allow_dummy_embeddings = true only for testing/development.",
                        embedding_provider
                    ),
                });
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
        let loader = SkillLoader::new().with_user_dir().with_project_dir(std::path::Path::new("."));
        let skills = loader.load_all();
        for skill in skills {
            let hooks = hooks_from_skill(&skill.name, &skill.triggers, &skill.instructions);
            registry.register_all(hooks);
        }

        // Create shared DynamicLlmClient for all LLM-dependent components.
        // Keep concrete client when we own it so bridge can use execution-scoped token
        // attribution (`DynamicLlmClient::scope_usage`) instead of global lease+delta.
        let (dyn_llm_client, llm_usage_scope_client): (
            Arc<dyn LlmClient>,
            Option<Arc<DynamicLlmClient>>,
        ) = match llm_client_override {
            Some(client) => (client, None),
            None => {
                let c = Arc::new(DynamicLlmClient::new(shared_config.clone()));
                (c.clone() as Arc<dyn LlmClient>, Some(c))
            },
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

        let strategic_executor = Arc::new(StrategicExecutor::new(
            state_store.clone(),
            sql_store.clone(),
            dyn_llm_client.clone(),
            cache_config.database_url.clone().into(),
            proposal_manager.clone(),
        ));

        router.register("strategic", strategic_executor.clone());

        // ── Tool Registry & Custom Tools ──
        let tool_registry = Arc::new(ToolRegistry::new());

        // Load custom tools (.opencode/tools/*.toml)
        let custom_loader = CustomToolLoader::new(".opencode/tools");
        let custom_defs = custom_loader.load_and_register(&tool_registry).await.unwrap_or_default();

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

        // Collect MCP tool/resource/prompt info for agent registration and catalog
        let mut mcp_servers: Vec<(String, String, Vec<McpTool>)> = Vec::new();
        let mut mcp_catalog: Vec<McpServerCatalog> = Vec::new();
        let mut mcp_executors: std::collections::HashMap<String, Arc<MCPToolExecutor>> =
            std::collections::HashMap::new();

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
                    let mcp = Arc::new(mcp);
                    let tools = mcp.list_tools().await.unwrap_or_default();
                    let (resources, resource_templates) = match mcp.list_resources().await {
                        Ok(r) => (r.resources, r.resource_templates),
                        Err(_) => (Vec::new(), Vec::new()),
                    };
                    let prompts = match mcp.list_prompts().await {
                        Ok(p) => p.prompts,
                        Err(_) => Vec::new(),
                    };
                    info!(
                        "Connected MCP server '{prefix}' via stdio: {command_line} ({} tools, {} resources, {} prompts)",
                        tools.len(),
                        resources.len(),
                        prompts.len()
                    );
                    mcp_catalog.push(McpServerCatalog {
                        server_prefix: prefix.clone(),
                        label: command_line.clone(),
                        resources,
                        resource_templates,
                        prompts,
                    });
                    mcp_servers.push((prefix.clone(), command_line.clone(), tools));
                    mcp_executors.insert(prefix.clone(), mcp.clone());
                    router.register(prefix, mcp);
                },
                Err(e) => {
                    warn!("Failed to start MCP server for prefix '{prefix}': {e}");
                },
            }
        }
        for (prefix, url) in &http_servers {
            match MCPToolExecutor::from_url(url).await {
                Ok(mcp) => {
                    let mcp = Arc::new(mcp);
                    let tools = mcp.list_tools().await.unwrap_or_default();
                    let (resources, resource_templates) = match mcp.list_resources().await {
                        Ok(r) => (r.resources, r.resource_templates),
                        Err(_) => (Vec::new(), Vec::new()),
                    };
                    let prompts = match mcp.list_prompts().await {
                        Ok(p) => p.prompts,
                        Err(_) => Vec::new(),
                    };
                    info!(
                        "Connected MCP server '{prefix}' via HTTP: {url} ({} tools, {} resources, {} prompts)",
                        tools.len(),
                        resources.len(),
                        prompts.len()
                    );
                    mcp_catalog.push(McpServerCatalog {
                        server_prefix: prefix.clone(),
                        label: url.to_string(),
                        resources,
                        resource_templates,
                        prompts,
                    });
                    mcp_servers.push((prefix.clone(), url.to_string(), tools));
                    mcp_executors.insert(prefix.clone(), mcp.clone());
                    router.register(prefix, mcp);
                },
                Err(e) => {
                    warn!("Failed to connect MCP server for '{prefix}' at {url}: {e}");
                },
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
        // 联邦策略权重视图（v0.4.0 Phase B/E，ADR-017）：必须**提前**构造。
        // 此前它只在末尾的 struct literal 里内联创建，synthesizer 拿不到同一个
        // Arc，于是「执行路径消费集群权重」只能是空话——读的是一份永远为空的
        // 视图。现在 handler 写、synthesizer 读的是同一份。
        let strategy_weights = Arc::new(parking_lot::RwLock::new(
            sisyphus_types::strategy_weights::FederatedStrategyWeights::default(),
        ));
        let strategy_selection = config_snapshot.cluster.strategy_selection;
        if strategy_selection.enabled {
            tracing::info!(
                min_uses = strategy_selection.min_uses,
                "Cluster strategy selection enabled: execution path may use federated weights"
            );
        }
        let meta_synthesizer = Arc::new(
            MetaLearningSynthesizer::new(
                Arc::new(sisyphus_intel::GoalDrivenSynthesizer::new(
                    tool_executor.clone(),
                )),
                meta_learner.clone(),
            )
            .with_federated_selection(strategy_weights.clone(), strategy_selection),
        );
        let synthesizer = Arc::new(sisyphus_intel::StrategicSopSynthesizer::new(
            meta_synthesizer,
            simulator.clone(),
        ));

        let consensus = Arc::new(ClusterConsensus::new(
            "node-1".to_string(),
            vec![], // In production, this would be loaded from config
        ));

        // 复用上方按 provider 配置构造的嵌入客户端（kernel 当前未消费
        // 该参数，但签名上应传入配置一致的实例而非无条件 Dummy）。
        let embedding_client = embedding_client.clone();
        let bridge_embedding_client = embedding_client.clone();

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
        // 集群模式下，内核写经验时顺带广播到 peer；接收端走 apply_remote，
        // 不经过该装饰器，因此不会形成广播回环。
        let kernel_experience_bank: Arc<dyn sisyphus_types::experience::ExperienceBank> =
            match cluster.as_ref() {
                Some(runtime) => Arc::new(crate::peer::PeerBroadcastExperienceBank::new(
                    experience_bank.clone(),
                    runtime.peer_manager.clone(),
                )),
                None => experience_bank.clone(),
            };

        let kernel = Arc::new(SisyphusKernel::new(
            state_store.clone(),
            sop_store.clone(),
            vector_store.clone(),
            kernel_experience_bank,
            tool_executor,
            Arc::new(sisyphus_tools_executor::executor::test_cmds::TestExecutor::new()),
            intel,
            dyn_llm_client.clone(),
            dyn_llm_client.clone(),
            Arc::new(sisyphus_tools_llm_tools::expert_reviewer::DefaultExpertReviewer),
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

        // ── 自编程产物目录接线 ────────────────────────────────────────
        // 自编程编译出的 wasm 必须落在沙箱读取的同一个目录，否则行为探针
        // 加载不到刚编译的模块（`WasmCompiler` 的默认值只是 `tools`）。
        kernel
            .strategic_manager
            .sp_manager
            .set_tools_dir(PathBuf::from(&config_snapshot.sandbox.wasm_tools_dir));

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

        let (
            peer_manager,
            cluster_config,
            control_plane_coordinator,
            control_plane_state,
            control_plane_tls_wired,
        ) = match cluster.as_ref() {
            Some(rt) => (
                Some(rt.peer_manager.clone()),
                rt.cluster_config.clone(),
                rt.control_plane_coordinator.clone(),
                rt.control_plane_state.clone(),
                rt.control_plane_tls_wired,
            ),
            None => (None, config_snapshot.cluster.clone(), None, None, false),
        };

        let bridge = MySisyphusBridge {
            kernel,
            state_store,
            concrete_store: store.state.clone(),
            vector_store,
            embedding_client: bridge_embedding_client,
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
            llm_usage_scope_client,
            llm_in_flight: Arc::new(AtomicUsize::new(0)),
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
            strategy_weights,
            peer_manager,
            cluster_config,
            control_plane_coordinator,
            control_plane_state,
            control_plane_tls_wired,
            mcp_catalog: Arc::new(mcp_catalog),
            mcp_executors: Arc::new(mcp_executors),
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

impl MySisyphusBridge {
    /// Kernel.
    pub fn kernel(&self) -> &Arc<SisyphusKernel> {
        &self.kernel
    }

    /// 未被集群广播装饰的底层经验库。
    ///
    /// 经验同步处理器必须拿这个实例而不是内核里的 `dyn ExperienceBank`：
    /// 内核那份在集群模式下被 [`PeerBroadcastExperienceBank`] 包了，远程经验
    /// 经它落库会再次广播，形成回环。
    pub fn experience_bank(&self) -> &Arc<sisyphus_mem::experience_bank::VectorExperienceBank> {
        &self.experience_bank
    }

    /// 优雅关闭 bridge：等待 in-flight LLM 请求完成。
    ///
    /// 推荐由 main.rs 在收到 SIGINT/SIGTERM 后调用。
    /// 默认 5 秒超时（与 HTTP/gRPC graceful shutdown 对齐）。
    pub async fn shutdown(&self, wait: std::time::Duration) -> Result<(), String> {
        // 先把认知追踪队列排空：SOP 执行完毕到进程退出之间仍可能有记录
        // 只待在 writer 队列里，进程一退就再也检索不到跨会话历史。
        // flush 现在会如实报告写入失败/超时，不能静默丢弃：即使后续 LLM
        // shutdown 成功，也要把「认知追踪未持久化」这件事带回给调用方。
        let trace_flush = self.kernel().execution_manager.flush_cognitive_traces().await.err();
        // 委托给 LLM client；DynamicLlmClient 内部会调用
        // provider_registry.shared_http().shutdown(wait).
        let llm = self.llm_client.shutdown(wait).await;
        match (trace_flush, llm) {
            (Some(trace_err), Ok(())) => Err(trace_err.to_string()),
            (Some(trace_err), Err(llm_err)) => Err(format!("{llm_err}; {trace_err}")),
            (None, result) => result,
        }
    }

    /// 返回共享 HTTP 客户端的**实时**统计读取器（供 `/api/metrics` 使用）。
    ///
    /// 刻意只暴露计数快照的读取闭包而非客户端句柄：`LlmClient` 定义在
    /// `sisyphus-types`，而客户端内部包装 `reqwest`，让 trait 泄漏它会把
    /// `reqwest` 引入被全 workspace 依赖的基础 crate（ADR-014 阶段 3）。
    /// None 表示桥接未启用 LLM（开发/CLI 模式）。
    pub fn http_stats_provider(
        &self,
    ) -> Option<Arc<dyn Fn() -> sisyphus_types::HttpClientStats + Send + Sync>> {
        let llm = self.llm_client.clone();
        // No LLM configured means no client to read stats from.
        llm.http_stats()?;
        Some(Arc::new(move || llm.http_stats().unwrap_or_default()))
    }
    pub(crate) fn tool_registry(&self) -> &Arc<ToolRegistry> {
        &self.tool_registry
    }
    pub(crate) fn agent_registry(&self) -> &Arc<AgentRegistry> {
        &self.agent_registry
    }
    pub(crate) fn mcp_catalog(&self) -> &Arc<Vec<McpServerCatalog>> {
        &self.mcp_catalog
    }
    pub(crate) fn mcp_executors(
        &self,
    ) -> &Arc<std::collections::HashMap<String, Arc<MCPToolExecutor>>> {
        &self.mcp_executors
    }
    pub(crate) fn skill_loader(&self) -> &Arc<SkillLoader> {
        &self.skill_loader
    }
    pub(crate) fn default_fuel(&self) -> u64 {
        self.default_fuel
    }

    /// 尝试获取「任务级 LLM 用量观测」独占租约。
    ///
    /// bridge 的 `DynamicLlmClient` 暴露的是**进程累计**计数器且被所有并发请求共享；
    /// 只有在无其他并发观测任务时，`before/after` 差值才能安全归因到当前任务。
    /// 返回 `true` 表示租约已获取（调用方必须在任务结束时调用 [`end_llm_task`]）；
    /// `false` 表示存在并发观测，调用方应将 token 记为 `None`（未知 ≠ 0），
    /// 不得把他人用量归因到本任务。
    ///
    /// 注意：这只是 best-effort 归因闸门，不会阻塞其他 LLM 请求本身；若并发请求
    /// 在租约持有期间发生了未被本闸门计入的 LLM 调用，差值仍可能污染，但该情况
    /// 要求调用方也走本闸门协议（当前 bridge 记录路径已统一接入）。
    pub(crate) fn begin_llm_task(&self) -> bool {
        self.llm_in_flight.compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire).is_ok()
    }

    /// 释放 [`begin_llm_task`] 获取的独占租约。
    pub(crate) fn end_llm_task(&self) {
        self.llm_in_flight.store(0, Ordering::Release);
    }

    /// 读取当前累计 LLM token 用量快照（provider 未上报时为 `None`）。
    pub(crate) fn llm_token_usage_snapshot(&self) -> Option<sisyphus_types::LlmTokenUsage> {
        self.llm_client.token_usage()
    }

    /// 在执行级 token 归因上下文中运行 future，返回 `(结果, usage)`。
    ///
    /// - 若可用具体 `DynamicLlmClient`，优先使用 task-local 执行作用域
    ///   （`DynamicLlmClient::scope_usage`），对作用域内可见的 LLM 调用实现确定性归因，
    ///   不依赖全局累计差值与独占租约。
    /// - 否则回退到 best-effort 租约+累计差值：仅在独占观测租约下采集，
    ///   并发/不可归因时返回 `None`（未知 ≠ 0，不伪造）。
    pub(crate) async fn run_with_usage_scope<Fut, T>(
        &self,
        fut: Fut,
    ) -> (T, Option<sisyphus_types::LlmTokenUsage>)
    where
        Fut: std::future::Future<Output = T>,
    {
        if self.llm_usage_scope_client.is_some() {
            // 执行作用域归因（确定性，作用域内可见调用）。
            return DynamicLlmClient::scope_usage(fut).await;
        }

        // 回退：独占租约 + 全局累计差值（best-effort）。
        let usage_lease = self.begin_llm_task();
        let usage_before = if usage_lease {
            self.llm_token_usage_snapshot()
        } else {
            None
        };
        let result = fut.await;
        let usage = if usage_lease {
            let after = self.llm_token_usage_snapshot();
            self.end_llm_task();
            crate::quality_record::token_usage_delta(usage_before, after)
        } else {
            (None, None)
        };
        let (p, c) = usage;
        let usage = match (p, c) {
            (None, None) => None,
            (Some(prompt), Some(completion)) => Some(sisyphus_types::LlmTokenUsage {
                prompt_tokens: prompt,
                completion_tokens: completion,
                calls: 0,
            }),
            (Some(prompt), None) => Some(sisyphus_types::LlmTokenUsage {
                prompt_tokens: prompt,
                completion_tokens: 0,
                calls: 0,
            }),
            (None, Some(completion)) => Some(sisyphus_types::LlmTokenUsage {
                prompt_tokens: 0,
                completion_tokens: completion,
                calls: 0,
            }),
        };
        (result, usage)
    }

    #[expect(dead_code)]
    pub(crate) async fn get_current_project_id(&self) -> Option<String> {
        self.workspace_manager.get_current().await.map(|ws| ws.project_id)
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

    /// 联邦策略权重聚合视图（v0.4.0 Phase B，ADR-017）。
    ///
    /// 共享视图由 [`crate::peer::StrategyWeightsHandler`] 持有：本地发布写入、
    /// 入站合并更新。暴露给 REST `/api/strategy/weights` 读取。
    pub fn strategy_weights(
        &self,
    ) -> &Arc<parking_lot::RwLock<sisyphus_types::strategy_weights::FederatedStrategyWeights>> {
        &self.strategy_weights
    }

    /// 集群 peer 管理器（Phase 6.1 只读状态端点用；`None` = 单节点）。
    pub fn peer_manager(&self) -> Option<&Arc<crate::peer::PeerManager>> {
        self.peer_manager.as_ref()
    }

    /// 集群配置快照（只读）。
    pub fn cluster_config(&self) -> &sisyphus_types::ClusterConfig {
        &self.cluster_config
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
            //
            // `compare_exchange` 而不是 `store`：chaos 故障是并发的，
            // 两个请求可能同时越过阈值，各自 spawn 一个自编程循环
            // （每次都要 LLM + rustc + 沙箱，成本极高）。
            // 标志位是**闩锁**语义（`chaos_auto_fix_was_triggered` 表示
            // “是否曾触发”），因此任务结束后不复位——一次性安全阀。
            if total > 3
                && self
                    .chaos_auto_fix_triggered
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
                    .is_ok()
            {
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
