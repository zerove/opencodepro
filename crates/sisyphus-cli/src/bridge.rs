use anyhow::{Context, Result};
use std::collections::HashMap;
use tonic::transport::Endpoint;

use sisyphus_bridge_api::sisyphus_bridge_client::SisyphusBridgeClient;
pub(crate) use sisyphus_bridge_api::{
    AgentMetadata, AddMessageRequest, AddMessageResponse, BenchmarkEntry, BreakerMetricsRequest,
    BreakerStatus, DecomposeGoalRequest, ExecuteSopRequest, ExecuteToolRequest,
    ExecuteToolResponse, HealthCheckRequest, HealthCheckResponse, LearningInsightsRequest,
    LearningInsightsResponse, ListAgentsRequest, ListBenchmarksRequest, ListPluginsRequest,
    ListSoPsRequest, ListSkillsRequest, ListToolsRequest, LoadPluginRequest, LoadPluginResponse,
    PluginDescriptor, SkillMetadata, SopInfo, TaskProto, ToolMetadata, UnloadPluginRequest,
    UnloadPluginResponse,
};

pub struct BridgeClient {
    inner: SisyphusBridgeClient<tonic::transport::Channel>,
    token: Option<String>,
}

impl BridgeClient {
    /// Connect to a running Sisyphus bridge daemon.
    /// Reads SISYPHUS_AUTH_TOKEN env var for optional bearer token.
    pub async fn connect(addr: &str) -> Result<Self> {
        let endpoint = Endpoint::new(addr.to_string()).context("Invalid bridge address")?;
        let inner = SisyphusBridgeClient::connect(endpoint)
            .await
            .context(format!("Failed to connect to bridge at {addr}"))?;
        let token = std::env::var("SISYPHUS_AUTH_TOKEN").ok();
        Ok(Self { inner, token })
    }

    /// Inject auth token into a gRPC request if configured.
    fn with_auth<T>(&self, mut req: tonic::Request<T>) -> Result<tonic::Request<T>> {
        if let Some(ref token) = self.token {
            let bearer = format!("Bearer {}", token);
            req.metadata_mut()
                .insert("authorization", bearer.parse().context("Invalid auth token format")?);
        }
        Ok(req)
    }

    /// Execute an SOP (Standard Operating Procedure) by ID.
    pub async fn execute_sop(&mut self, sop_id: &str) -> Result<String> {
        let request = self.with_auth(tonic::Request::new(ExecuteSopRequest {
            sop_id: sop_id.to_string(),
            args: HashMap::new(),
        }))?;
        let response = self
            .inner
            .execute_sop(request)
            .await
            .context("ExecuteSOP RPC failed")?;
        Ok(response.into_inner().result)
    }

    /// Check if the bridge is healthy.
    pub async fn health_check(&mut self) -> Result<HealthCheckResponse> {
        let request = self.with_auth(tonic::Request::new(HealthCheckRequest {}))?;
        let response = self
            .inner
            .health_check(request)
            .await
            .context("HealthCheck RPC failed")?;
        Ok(response.into_inner())
    }

    /// List available SOPs.
    pub async fn list_sops(&mut self) -> Result<Vec<SopInfo>> {
        let request = self.with_auth(tonic::Request::new(ListSoPsRequest {}))?;
        let response = self
            .inner
            .list_so_ps(request)
            .await
            .context("ListSOPs RPC failed")?;
        Ok(response.into_inner().sops)
    }

    /// Execute a built-in or agent tool via the bridge's ExecutorRouter.
    pub async fn execute_tool(
        &mut self,
        tool_id: &str,
        args_json: &str,
    ) -> Result<ExecuteToolResponse> {
        let request = self.with_auth(tonic::Request::new(ExecuteToolRequest {
            tool_id: tool_id.to_string(),
            args_json: args_json.to_string(),
            fuel_limit: 0,
        }))?;
        let response = self
            .inner
            .execute_tool(request)
            .await
            .context(format!("ExecuteTool({tool_id}) RPC failed"))?;
        Ok(response.into_inner())
    }

    /// List benchmark runs from the bridge.
    pub async fn list_benchmarks(&mut self) -> Result<Vec<BenchmarkEntry>> {
        let request = self.with_auth(tonic::Request::new(ListBenchmarksRequest {
            name_filter: String::new(),
        }))?;
        let response = self
            .inner
            .list_benchmarks(request)
            .await
            .context("ListBenchmarks RPC failed")?;
        Ok(response.into_inner().benchmarks)
    }

    /// Get circuit breaker metrics from the bridge.
    pub async fn get_breaker_metrics(
        &mut self,
        model_name: Option<&str>,
    ) -> Result<Vec<BreakerStatus>> {
        let request = self.with_auth(tonic::Request::new(BreakerMetricsRequest {
            model_name: model_name.unwrap_or("").to_string(),
        }))?;
        let response = self
            .inner
            .get_breaker_metrics(request)
            .await
            .context("GetBreakerMetrics RPC failed")?;
        Ok(response.into_inner().breakers)
    }

    /// List available agents.
    pub async fn list_agents(&mut self) -> Result<Vec<AgentMetadata>> {
        let request = self.with_auth(tonic::Request::new(ListAgentsRequest {}))?;
        let response = self
            .inner
            .list_agents(request)
            .await
            .context("ListAgents RPC failed")?;
        Ok(response.into_inner().agents)
    }

    /// List available tools.
    pub async fn list_tools(&mut self) -> Result<Vec<ToolMetadata>> {
        let request = self.with_auth(tonic::Request::new(ListToolsRequest {}))?;
        let response = self
            .inner
            .list_tools(request)
            .await
            .context("ListTools RPC failed")?;
        Ok(response.into_inner().tools)
    }

    /// List installed skills.
    pub async fn list_skills(&mut self) -> Result<Vec<SkillMetadata>> {
        let request = self.with_auth(tonic::Request::new(ListSkillsRequest {}))?;
        let response = self
            .inner
            .list_skills(request)
            .await
            .context("ListSkills RPC failed")?;
        Ok(response.into_inner().skills)
    }


    /// Decompose a goal into tasks.
    pub async fn decompose_goal(
        &mut self,
        goal_id: &str,
        objective: &str,
        context: &str,
    ) -> Result<Vec<TaskProto>> {
        let request = self.with_auth(tonic::Request::new(DecomposeGoalRequest {
            goal_id: goal_id.to_string(),
            objective: objective.to_string(),
            context: context.to_string(),
        }))?;
        let response = self
            .inner
            .decompose_goal(request)
            .await
            .context("DecomposeGoal RPC failed")?;
        let inner = response.into_inner();
        if !inner.error.is_empty() {
            anyhow::bail!("DecomposeGoal failed: {}", inner.error);
        }
        Ok(inner.tasks)
    }

    /// Get meta-learning insights from the bridge.
    pub async fn get_learning_insights(
        &mut self,
        strategy_filter: Option<&str>,
    ) -> Result<LearningInsightsResponse> {
        let request = self.with_auth(tonic::Request::new(LearningInsightsRequest {
            strategy_filter: strategy_filter.unwrap_or("").to_string(),
        }))?;
        let response = self
            .inner
            .get_learning_insights(request)
            .await
            .context("GetLearningInsights RPC failed")?;
        Ok(response.into_inner())
    }

    /// List plugins registered on the bridge.
    pub async fn list_plugins(&mut self) -> Result<Vec<PluginDescriptor>> {
        let request = self.with_auth(tonic::Request::new(ListPluginsRequest {}))?;
        let response = self
            .inner
            .list_plugins(request)
            .await
            .context("ListPlugins RPC failed")?;
        Ok(response.into_inner().plugins)
    }

    /// Load a plugin onto the bridge.
    pub async fn load_plugin(
        &mut self,
        name: &str,
        version: &str,
        wasm_path: &str,
        tool_id: &str,
        fuel_limit: u64,
    ) -> Result<LoadPluginResponse> {
        let request = self.with_auth(tonic::Request::new(LoadPluginRequest {
            name: name.to_string(),
            version: version.to_string(),
            wasm_path: wasm_path.to_string(),
            tool_id: tool_id.to_string(),
            fuel_limit,
        }))?;
        let response = self
            .inner
            .load_plugin(request)
            .await
            .context("LoadPlugin RPC failed")?;
        Ok(response.into_inner())
    }

    /// Add a message to a session and get an AI response.
    pub async fn add_message(
        &mut self,
        session_id: &str,
        role: &str,
        content: &str,
        session_db_path: &str,
    ) -> Result<AddMessageResponse> {
        let request = self.with_auth(tonic::Request::new(AddMessageRequest {
            session_id: session_id.to_string(),
            role: role.to_string(),
            content: content.to_string(),
            session_db_path: session_db_path.to_string(),
        }))?;
        let response = self
            .inner
            .add_message(request)
            .await
            .context("AddMessage RPC failed")?;
        Ok(response.into_inner())
    }

    /// Unload a plugin from the bridge.
    pub async fn unload_plugin(&mut self, name: &str) -> Result<UnloadPluginResponse> {
        let request = self.with_auth(tonic::Request::new(UnloadPluginRequest {
            name: name.to_string(),
        }))?;
        let response = self
            .inner
            .unload_plugin(request)
            .await
            .context("UnloadPlugin RPC failed")?;
        Ok(response.into_inner())
    }
}
